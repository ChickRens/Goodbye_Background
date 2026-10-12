"""
公式 SAM2ImagePredictor と、自前ONNXパイプラインの出力を比較して
エクスポートの正しさを検証するスクリプト。

使い方（リポジトリ直下で実行。scripts/ に置く想定）:
    pip install pillow
    python scripts/verify_onnx_iou.py --image test.jpg \
        --point 320,240,1 --point 100,80,0

    --point は x,y,label（元画像のピクセル座標。label: 1=前景, 0=背景）。複数指定可。
    省略時は画像中央の前景1点。

    mean/std 正規化がONNXの内部か外部かは自動判定する（--norm baked / external で上書き可）。

検証は2段階:
  段階A) 公式と「同じ入力テンソル」をONNXに入れる → エクスポート自体の正しさ
  段階B) Rust側と同じ前処理（リサイズして /255 のみ）でONNXに入れる → 実運用に近い結果
"""
import argparse
import datetime
import os
import sys

repo_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.append(os.path.join(repo_root, "SAM2"))
os.chdir(repo_root)

import numpy as np
import onnxruntime as ort
import torch
from PIL import Image, ImageOps
from sam2.build_sam import build_sam2
from sam2.sam2_image_predictor import SAM2ImagePredictor

CONFIG = "configs/sam2.1/sam2.1_hiera_s.yaml"
CHECKPOINT = "models/sam2.1_hiera_small.pt"
MODEL_DIR = "models"
SIZE = 1024
MEAN = np.array([0.485, 0.456, 0.406], dtype=np.float32).reshape(1, 3, 1, 1)
STD = np.array([0.229, 0.224, 0.225], dtype=np.float32).reshape(1, 3, 1, 1)


def parse_point(text):
    x, y, label = text.split(",")
    return float(x), float(y), int(label)


def iou(a, b):
    """ロジット(>0 を前景)同士のIoU。"""
    a, b = a > 0, b > 0
    union = np.logical_or(a, b).sum()
    return 1.0 if union == 0 else float(np.logical_and(a, b).sum() / union)


def report(name, actual, expected):
    diff = np.abs(actual - expected)
    print(f"  {name:<22} shape={actual.shape}  max|diff|={diff.max():.5f}  mean|diff|={diff.mean():.6f}")


def rust_like_input(image):
    """Rust側と同じ前処理: 1024x1024にリサイズ（三角フィルタ相当）して /255 するだけ。正規化なし。"""
    resized = image.resize((SIZE, SIZE), Image.BILINEAR)
    x = np.asarray(resized, dtype=np.float32) / 255.0
    return x.transpose(2, 0, 1)[None]  # [1,3,1024,1024]


class OnnxPipeline:
    def __init__(self):
        self.enc = ort.InferenceSession(f"{MODEL_DIR}/sam2.1_hiera_small_image_encoder.onnx")
        self.prm = ort.InferenceSession(f"{MODEL_DIR}/sam2.1_hiera_small_prompt_encoder.onnx")
        self.dec = ort.InferenceSession(f"{MODEL_DIR}/sam2.1_hiera_small_mask_decoder.onnx")
        self.image_pe = np.fromfile(f"{MODEL_DIR}/image_pe.bin", dtype=np.float32).reshape(1, 256, 64, 64)

        # どのファイルを読み込んだかを表示（古いモデルを読んでいないかの確認用）
        print("読み込んだONNX:")
        for name in ("image_encoder", "prompt_encoder", "mask_decoder"):
            path = f"{MODEL_DIR}/sam2.1_hiera_small_{name}.onnx"
            mtime = datetime.datetime.fromtimestamp(os.path.getmtime(path)).strftime("%Y-%m-%d %H:%M:%S")
            print(f"  {path}  (更新: {mtime})")
        print("  prompt encoder の入力:", [i.name for i in self.prm.get_inputs()])

    def encode(self, x):
        return self.enc.run(
            ["image_embeddings", "high_res_feature_s0", "high_res_feature_s1"],
            {"image": x.astype(np.float32)},
        )

    def prompt(self, coords_1024, labels):
        # マスク入力なし（初回クリック相当）。
        # 再エクスポート後のモデルは has_mask=0 で no_mask_embed を使う（旧モデルには has_mask 入力がない）
        feed = {
            "point_coords": coords_1024.astype(np.float32)[None],
            "point_labels": labels.astype(np.int64)[None],
            "mask": np.zeros((1, 1, 256, 256), dtype=np.float32),
        }
        if "has_mask" in {i.name for i in self.prm.get_inputs()}:
            feed["has_mask"] = np.zeros(1, dtype=np.float32)
        return self.prm.run(["sparse_embeddings", "dense_embeddings"], feed)

    def decode(self, emb, s0, s1, sparse, dense):
        out = self.dec.run(
            ["masks"],
            {
                "image_embeddings": emb,
                "image_pe": self.image_pe,
                "sparse_prompt_embeddings": sparse,
                "dense_prompt_embeddings": dense,
                "high_res_feature_s0": s0,
                "high_res_feature_s1": s1,
            },
        )
        return out[0][0, 0]  # [256,256] ロジット


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--image", required=True)
    ap.add_argument("--point", action="append", type=parse_point)
    ap.add_argument("--norm", choices=["auto", "baked", "external"], default="auto",
                    help="mean/std 正規化の位置。baked=ONNX内部（入力は0〜1）、external=ONNX外部（入力は正規化済み）。"
                         "auto は公式の出力に近い方を自動判定")
    ap.add_argument("--baked-norm", action="store_true", help="--norm baked の別名")
    args = ap.parse_args()

    image = ImageOps.exif_transpose(Image.open(args.image)).convert("RGB")
    w, h = image.size
    points = args.point or [(w / 2, h / 2, 1)]
    coords = np.array([[x, y] for x, y, _ in points], dtype=np.float32)
    labels = np.array([lab for _, _, lab in points], dtype=np.int32)
    coords_1024 = coords * np.array([SIZE / w, SIZE / h], dtype=np.float32)

    print(f"image: {w}x{h}, points: {points}")

    # ---- 公式（PyTorch）----
    model = build_sam2(CONFIG, CHECKPOINT, device="cpu")
    predictor = SAM2ImagePredictor(model)
    image_np = np.array(image)  # 書き込み可能なコピー（警告対策）
    predictor.set_image(image_np)

    _, _, ref_low = predictor.predict(
        point_coords=coords,
        point_labels=labels,
        multimask_output=False,
        return_logits=True,
    )
    ref_low = ref_low[0]  # [256,256] ロジット

    ref_embed = predictor._features["image_embed"].detach().cpu().numpy()
    ref_s0, ref_s1 = [f.detach().cpu().numpy() for f in predictor._features["high_res_feats"]]

    with torch.no_grad():
        ref_sparse, ref_dense = model.sam_prompt_encoder(
            points=(
                torch.from_numpy(coords_1024)[None],
                torch.from_numpy(labels.astype(np.int64))[None],
            ),
            boxes=None,
            masks=None,  # 公式は、マスク入力なしのとき no_mask_embed を使う
        )
    ref_sparse, ref_dense = ref_sparse.detach().cpu().numpy(), ref_dense.detach().cpu().numpy()

    # 公式が実際に使った入力テンソル（正規化済み）
    official_in = predictor._transforms(image_np).unsqueeze(0).detach().cpu().numpy()
    x_raw = official_in * STD + MEAN  # 正規化前（0〜1）に戻したもの

    onnx = OnnxPipeline()

    norm = "baked" if args.baked_norm else args.norm
    if norm == "auto":
        d_external = np.abs(onnx.encode(official_in)[0] - ref_embed).max()
        d_baked = np.abs(onnx.encode(x_raw)[0] - ref_embed).max()
        norm = "baked" if d_baked < d_external else "external"
        print(f"正規化の位置を自動判定: {norm} "
              f"(image_embeddings 最大差 external={d_external:.4f}, baked={d_baked:.4f})")
    x_ref = x_raw if norm == "baked" else official_in

    # ---- 段階A: 公式と同じ入力 ----
    print("\n[段階A] 公式と同じ入力テンソルをONNXに入力（エクスポート自体の検証）")
    emb, s0, s1 = onnx.encode(x_ref)
    sparse, dense = onnx.prompt(coords_1024, labels)
    report("image_embeddings", emb, ref_embed)
    report("high_res_feature_s0", s0, ref_s0)
    report("high_res_feature_s1", s1, ref_s1)
    report("sparse_embeddings", sparse, ref_sparse)
    report("dense_embeddings", dense, ref_dense)
    low_a = onnx.decode(emb, s0, s1, sparse, dense)
    print(f"  => 低解像度マスク IoU = {iou(low_a, ref_low):.4f}")

    # ---- 段階B: Rust相当の前処理 ----
    print("\n[段階B] Rust側と同じ前処理（/255のみ）でONNXに入力（実運用に近い結果）")
    emb, s0, s1 = onnx.encode(rust_like_input(image))
    low_b = onnx.decode(emb, s0, s1, sparse, dense)
    print(f"  => 低解像度マスク IoU = {iou(low_b, ref_low):.4f}")


if __name__ == "__main__":
    main()