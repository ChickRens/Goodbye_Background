"""動画ページのプレビュー（Rust の CPU 推論）用に、本番のワーカーと同じモデルを取得する。

- constants.json と onnx/vision_encoder.onnx: Hugging Face（diffusionstudio）から、ワーカーと同じリビジョン
  （cutout.models.MODEL_REVISION）に固定して取得する
- onnx/mask_decoder_with_mask_v1.onnx: 前回のマスクを受け取る自前のマスクデコーダ。Modal Volume から
  `modal volume get` で取得し、SHA-256（cutout.models.MASK_DECODER_SHA256）を照合する
  （仕様 docs/superpowers/specs/2026-11-16-video-prompt-mask-input-design.md。Modal の CLI にログインしておくこと）
既にあるファイルは飛ばす（何度実行してもよい）。古い onnx/mask_decoder.onnx が残っていても使わない。

使い方: cd worker && uv run python scripts/fetch_preview_models.py [取得先]
  取得先の省略時はリポジトリ直下の models/sam2.1-small-video/（サーバーが読む場所。src/video_cutout/config.rs の PREVIEW_MODEL_DIR）
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "worker"))  # scripts/ から実行しても cutout を import できるように

from cutout import models  # noqa: E402

DEFAULT_DIR = ROOT / "models" / "sam2.1-small-video"
PREVIEW_GRAPHS = ["vision_encoder"]
PREVIEW_FILES = ["constants.json"] + [f"onnx/{g}.onnx" for g in PREVIEW_GRAPHS] + [f"onnx/{models.MASK_DECODER_FILE}"]


def fetch_mask_decoder(model_dir: Path, run=subprocess.run) -> Path:
    """自前のマスクデコーダを Modal Volume から取得し、SHA-256 を照合する。既にあって合えば取得しない。"""
    path = model_dir / "onnx" / models.MASK_DECODER_FILE
    if path.exists():
        try:
            models.verify_sha256(path, models.MASK_DECODER_SHA256)
            return path
        except ValueError:
            pass  # 古い・壊れたファイルは取り直す
    path.parent.mkdir(parents=True, exist_ok=True)
    run(
        ["modal", "volume", "get", "--force", models.MODELS_VOLUME, models.MASK_DECODER_VOLUME_PATH, str(path)],
        check=True,
    )
    models.verify_sha256(path, models.MASK_DECODER_SHA256)
    return path


def main(argv: list[str], run=subprocess.run) -> int:
    model_dir = Path(argv[0]).resolve() if argv else DEFAULT_DIR
    models.fetch_models(model_dir, models.MODEL_REVISION, graphs=PREVIEW_GRAPHS)
    fetch_mask_decoder(model_dir, run=run)
    print(f"プレビュー用のモデルを取得しました: revision={models.MODEL_REVISION} / マスクデコーダ {models.MASK_DECODER_FILE}")
    for rel in PREVIEW_FILES:
        path = model_dir / rel
        print(f"  {path}  {path.stat().st_size:,} bytes")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
