"""
SAM 2.1 (hiera_small) の image encoder をONNXにエクスポートする。

公式の SAM2ImagePredictor.set_image() と同じ出力になるよう、以下をグラフに含める:
  1. ImageNetの mean/std 正規化（入力は 0〜1 の RGB）
  2. high-res特徴量: backbone_fpn[0/1] を mask decoder の conv_s0 / conv_s1 で投影
  3. 最低解像度の特徴量に no_mem_embed を加算

入力 : image               [1, 3, 1024, 1024]  (0〜1 の RGB。正規化はモデル内部で行う)
出力 : image_embeddings    [1, 256, 64, 64]
       high_res_feature_s0 [1, 32, 256, 256]
       high_res_feature_s1 [1, 64, 128, 128]

実行（リポジトリ直下から）:
    python scripts/export_image_encoder.py
"""
import os
import sys

base_dir = os.path.dirname(os.path.abspath(__file__))
repo_root = os.path.dirname(base_dir)
sys.path.append(os.path.join(repo_root, "SAM2"))

import onnx
import onnxruntime as ort
import torch
from sam2.build_sam import build_sam2


class ImageEncoderWrapper(torch.nn.Module):
    def __init__(self, model):
        super().__init__()
        self.image_encoder = model.image_encoder
        self.conv_s0 = model.sam_mask_decoder.conv_s0
        self.conv_s1 = model.sam_mask_decoder.conv_s1
        self.no_mem_embed = model.no_mem_embed  # [1, 1, 256]
        self.register_buffer("mean", torch.tensor([0.485, 0.456, 0.406]).view(1, 3, 1, 1))
        self.register_buffer("std", torch.tensor([0.229, 0.224, 0.225]).view(1, 3, 1, 1))

    def forward(self, image: torch.Tensor):
        image = (image - self.mean) / self.std
        out = self.image_encoder(image)
        fpn = out["backbone_fpn"]

        high_res_s0 = self.conv_s0(fpn[0])  # 256ch -> 32ch
        high_res_s1 = self.conv_s1(fpn[1])  # 256ch -> 64ch
        image_embeddings = out["vision_features"] + self.no_mem_embed.view(1, -1, 1, 1)

        return image_embeddings, high_res_s0, high_res_s1


if __name__ == "__main__":
    print("PyTorch version:", torch.__version__)
    print("ONNX version:", onnx.__version__)
    print("ONNX Runtime version:", ort.__version__)

    os.chdir(repo_root)
    config_path = "configs/sam2.1/sam2.1_hiera_s.yaml"
    model_path = "models/sam2.1_hiera_small.pt"
    onnx_model_path = "sam2.1_hiera_small_image_encoder.onnx"

    model = build_sam2(config_path, model_path, device="cpu")
    model.eval()

    if not getattr(model, "directly_add_no_mem_embed", False):
        print("WARNING: model.directly_add_no_mem_embed が True ではありません。"
              "公式の set_image() の挙動を確認してください。")

    wrapper = ImageEncoderWrapper(model).eval()
    dummy_image = torch.zeros((1, 3, 1024, 1024), dtype=torch.float32)

    print("Exporting image encoder to ONNX...")
    torch.onnx.export(
        wrapper,
        dummy_image,
        onnx_model_path,
        opset_version=18,
        input_names=["image"],
        output_names=["image_embeddings", "high_res_feature_s0", "high_res_feature_s1"],
        dynamo=False,
        optimize=False,
        do_constant_folding=False,
        training=torch.onnx.TrainingMode.EVAL,
        verbose=False,
    )
    print(f"ONNX model exported to {onnx_model_path}")

    onnx.checker.check_model(onnx.load(onnx_model_path))
    print("ONNX model is valid.")

    session = ort.InferenceSession(onnx_model_path)
    outputs = session.run(None, {session.get_inputs()[0].name: dummy_image.numpy()})
    for i, output in enumerate(outputs):
        print(f"  Output {i}: shape={output.shape}, dtype={output.dtype}")
    print("SUCCESS: image encoder exported and inference successful!")