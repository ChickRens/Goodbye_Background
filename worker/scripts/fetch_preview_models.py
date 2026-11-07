"""動画ページのプレビュー（Rust の CPU 推論）用に、SAM2.1 tiny のモデルを取得する。

ワーカーと同じリビジョン（cutout.models.MODEL_REVISION）に固定し、プレビューで使う
constants.json と onnx/vision_encoder.onnx と onnx/mask_decoder.onnx だけを取得する。
既にあるファイルは飛ばす（何度実行してもよい）。

使い方: cd worker && uv run python scripts/fetch_preview_models.py [取得先]
  取得先の省略時はリポジトリ直下の models/sam2.1-tiny-video/（サーバーが読む場所）
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "worker"))  # scripts/ から実行しても cutout を import できるように

from cutout.models import MODEL_REVISION, fetch_models  # noqa: E402

DEFAULT_DIR = ROOT / "models" / "sam2.1-tiny-video"
PREVIEW_GRAPHS = ["vision_encoder", "mask_decoder"]
PREVIEW_FILES = ["constants.json"] + [f"onnx/{g}.onnx" for g in PREVIEW_GRAPHS]


def main(argv: list[str]) -> int:
    model_dir = Path(argv[0]).resolve() if argv else DEFAULT_DIR
    fetch_models(model_dir, MODEL_REVISION, graphs=PREVIEW_GRAPHS)
    print(f"プレビュー用のモデルを取得しました: revision={MODEL_REVISION}")
    for rel in PREVIEW_FILES:
        path = model_dir / rel
        print(f"  {path}  {path.stat().st_size:,} bytes")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
