"""
SAM 2.1 (hiera_small) の prompt encoder をONNXにエクスポートする。

公式は「マスク入力がないときは no_mask_embed を使う」。
ONNXにはPythonの条件分岐を持ち込めないので、has_mask (0.0 / 1.0) で2つの結果を混ぜて切り替える。

入力 : point_coords [1, N, 2]  (1024x1024 の入力画像座標系)
       point_labels [1, N]     (1=前景, 0=背景)
       mask         [1, 1, 256, 256]  (前回のマスクのロジット。has_mask=0 のときは無視される)
       has_mask     [1]       (0.0: マスクなし / 1.0: マスクあり)
出力 : sparse_embeddings, dense_embeddings

実行（リポジトリ直下から）:
    python scripts/export_prompt_encoder.py
"""
import os
import sys

base_dir = os.path.dirname(os.path.abspath(__file__))
repo_root = os.path.dirname(base_dir)
sys.path.append(os.path.join(repo_root, "SAM2"))

import numpy as np
import onnx
import onnxruntime as ort
import torch
from sam2.build_sam import build_sam2


class PromptEncoderWrapper(torch.nn.Module):
    def __init__(self, model: torch.nn.Module):
        super().__init__()
        self.prompt_encoder = model.sam_prompt_encoder

    def forward(
        self,
        point_coords: torch.Tensor,
        point_labels: torch.Tensor,
        mask: torch.Tensor,
        has_mask: torch.Tensor,
    ) -> tuple[torch.Tensor, torch.Tensor]:
        sparse, dense_from_mask = self.prompt_encoder(
            points=(point_coords, point_labels),
            boxes=None,
            masks=mask,
        )
        no_mask = self.prompt_encoder.no_mask_embed.weight.reshape(1, -1, 1, 1).expand_as(
            dense_from_mask
        )
        w = has_mask.view(1, 1, 1, 1)
        dense = w * dense_from_mask + (1.0 - w) * no_mask
        return sparse, dense


if __name__ == "__main__":
    print("PyTorch version:", torch.__version__)
    print("ONNX version:", onnx.__version__)
    print("ONNX Runtime version:", ort.__version__)

    os.chdir(repo_root)
    config_path = "configs/sam2.1/sam2.1_hiera_s.yaml"
    model_path = "models/sam2.1_hiera_small.pt"
    onnx_model_path = "sam2.1_hiera_small_prompt_encoder.onnx"

    model = build_sam2(config_path, model_path, device="cpu")
    model.eval()

    wrapper = PromptEncoderWrapper(model).eval()

    # image_pe は prompt encoder 由来。ここで1回だけ保存する（image_pe.bin への変換は別スクリプト）
    image_pe = model.sam_prompt_encoder.get_dense_pe()
    np.save("image_pe.npy", image_pe.detach().cpu().numpy())
    print("Saved image_pe.npy")

    dummy_point_coords = torch.zeros((1, 1, 2), dtype=torch.float32)
    dummy_point_labels = -torch.ones((1, 1), dtype=torch.int64)
    dummy_mask = torch.zeros((1, 1, 256, 256), dtype=torch.float32)
    dummy_has_mask = torch.zeros((1,), dtype=torch.float32)

    print("Exporting prompt encoder to ONNX...")
    torch.onnx.export(
        wrapper,
        (dummy_point_coords, dummy_point_labels, dummy_mask, dummy_has_mask),
        onnx_model_path,
        opset_version=18,
        input_names=["point_coords", "point_labels", "mask", "has_mask"],
        output_names=["sparse_embeddings", "dense_embeddings"],
        dynamic_axes={
            "point_coords": {0: "batch", 1: "num_points"},
            "point_labels": {0: "batch", 1: "num_points"},
            "mask": {0: "batch"},
        },
        dynamo=False,
        do_constant_folding=False,
        training=torch.onnx.TrainingMode.EVAL,
        verbose=False,
    )
    print(f"ONNX model exported to {onnx_model_path}")

    onnx.checker.check_model(onnx.load(onnx_model_path))
    print("ONNX model is valid.")

    session = ort.InferenceSession(onnx_model_path)
    names = [i.name for i in session.get_inputs()]
    feed = {
        names[0]: dummy_point_coords.numpy(),
        names[1]: dummy_point_labels.numpy(),
        names[2]: dummy_mask.numpy(),
        names[3]: dummy_has_mask.numpy(),
    }
    outputs = session.run(None, feed)
    for i, output in enumerate(outputs):
        print(f"  Output {i}: shape={output.shape}, dtype={output.dtype}")
    print("SUCCESS: prompt encoder exported and inference successful!")