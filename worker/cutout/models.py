"""SAM2.1 動画 ONNX モデル（本番は small）の取得と ONNX Runtime セッションの生成。"""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Literal

import httpx
import numpy as np
import onnxruntime as ort

# Hugging Face のコミットハッシュに固定する（main は動くため再現性が無い）。
TINY_REVISION = "66673b5db39371b7dd7847f4d3bc0d4f4179b79e"  # 2026-11-08 まで本番だったもの
SMALL_REVISION = "927ecec6e6ae2727d0af7720eefdb07f2efe4689"
# 本番のモデル（GPU ワーカーと Rust のプレビューの両方）。2026-11-08 に tiny → small（オーナー判断。
# docs/video-cutout-measurements.md の案 C。速く動く腕が消えにくくなる。GPU の処理時間は約 2.8 倍）
MODEL_REVISION = SMALL_REVISION
REPO_URL = "https://huggingface.co/diffusionstudio/sam2.1-small-video-onnx-fp16/resolve/{revision}/"
# 同じ形式（5 グラフ・同じ constants.json の項目）で書き出された大きいモデル。違いは入力の解像度と画像エンコーダの大きさ。
# 案 C（大きいモデル）の比較用。名前 → (Hugging Face のリポジトリ, コミットハッシュ)。2026-11-07 に API で確認
VIDEO_MODELS = {
    "tiny": ("diffusionstudio/sam2.1-tiny-video-onnx-fp16", TINY_REVISION),  # 512。vision_encoder 58MB
    "small": ("diffusionstudio/sam2.1-small-video-onnx-fp16", SMALL_REVISION),  # 1024。84MB（本番）
    "base-plus": ("diffusionstudio/sam2.1-base-plus-video-onnx-fp16", "386b83e09a774fe2782d86fd1b8cb5aaf718aa9d"),  # 1024。156MB
    "large": ("diffusionstudio/sam2.1-large-video-onnx-fp16", "98dbd222d9b3f75d15fe63a5c2a22b0b0029eea1"),  # 1024。447MB
}
# Hugging Face（diffusionstudio）から取るグラフ。マスクデコーダは自前で書き出したもの（下の MASK_DECODER_*）を使う
GRAPH_NAMES = ["vision_encoder", "memory_encoder", "memory_attention", "pointer_tpos"]

# 前回のマスク（mask_input / has_mask）を受け取るマスクデコーダ。worker/scripts/export_mask_decoder_with_mask.py で書き出し、
# Modal Volume に置く（仕様 docs/superpowers/specs/2026-11-16-video-prompt-mask-input-design.md）。
# プレビュー（Rust の src/video_cutout/preview/segmenter_sam2.rs の MASK_DECODER_SHA256）と同じファイルであることを SHA-256 で固定する
MASK_DECODER_FILE = "mask_decoder_with_mask_v1.onnx"
MASK_DECODER_SHA256 = "ba8862fdba8cb0027f7314bfb649fe18c16852a792ad6ca0dc76818aacac6ba6"
MODELS_VOLUME = "goodbye-background-models"
MASK_DECODER_VOLUME_PATH = f"sam2/{MASK_DECODER_FILE}"

_NP_TYPES = {
    "tensor(float)": np.float32,
    "tensor(float16)": np.float16,
    "tensor(int32)": np.int32,
    "tensor(int64)": np.int64,
    "tensor(bool)": np.bool_,
}


def fetch_models(
    model_dir: Path, revision: str, graphs: list[str] | None = None, repo: str | None = None
) -> Path:
    """constants.json と onnx/*.onnx を model_dir に取得する。既にあるファイルは飛ばす。

    graphs を渡すとそのグラフだけ取得する（Rust のプレビューは vision_encoder と mask_decoder だけ使う）。
    省略時は GRAPH_NAMES の全部（ワーカー用）。
    repo を省略すると本番のモデル（REPO_URL。small）。ほかのモデルは VIDEO_MODELS のリポジトリを渡す。
    """
    if repo is None:
        base = REPO_URL.format(revision=revision)
    else:
        base = f"https://huggingface.co/{repo}/resolve/{revision}/"
    files = ["constants.json"] + [f"onnx/{g}.onnx" for g in (GRAPH_NAMES if graphs is None else graphs)]
    for rel in files:
        dst = model_dir / rel
        if dst.exists() and dst.stat().st_size > 0:
            continue
        dst.parent.mkdir(parents=True, exist_ok=True)
        tmp = dst.with_suffix(dst.suffix + ".part")
        with httpx.stream("GET", base + rel, follow_redirects=True, timeout=300) as r:
            r.raise_for_status()
            with tmp.open("wb") as f:
                for chunk in r.iter_bytes():
                    f.write(chunk)
        tmp.rename(dst)
    return model_dir


def load_constants(model_dir: Path) -> dict:
    return json.loads((model_dir / "constants.json").read_text())


class OnnxGraph:
    """1 つの ONNX グラフ。入力の dtype をモデルに合わせ、fp16 出力は float32 にして返す。"""

    def __init__(
        self,
        path: Path,
        device: Literal["cpu", "cuda"],
        threads: int | None = None,
    ):
        so = ort.SessionOptions()
        if threads:
            so.intra_op_num_threads = threads
        so.inter_op_num_threads = 1
        so.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
        providers = (
            ["CUDAExecutionProvider", "CPUExecutionProvider"] if device == "cuda" else ["CPUExecutionProvider"]
        )
        self.name = path.stem
        self._s = ort.InferenceSession(str(path), so, providers=providers)
        if device == "cuda" and self._s.get_providers()[0] != "CUDAExecutionProvider":
            raise RuntimeError(f"{self.name}: CUDA を使えない（使えたのは {self._s.get_providers()}）")
        self._inputs = {i.name: i for i in self._s.get_inputs()}
        self._out_names = [o.name for o in self._s.get_outputs()]

    def run(self, **feeds) -> dict:
        fixed = {}
        for k, v in feeds.items():
            meta = self._inputs[k]
            arr = np.asarray(v, dtype=_NP_TYPES[meta.type])
            if all(isinstance(d, int) for d in meta.shape) and int(np.prod(meta.shape)) == arr.size:
                arr = arr.reshape(meta.shape)  # 例: binarize のスカラー形状
            fixed[k] = arr
        out = self._s.run(self._out_names, fixed)
        return {n: (o.astype(np.float32) if o.dtype == np.float16 else o) for n, o in zip(self._out_names, out)}


def verify_sha256(path: Path, expected: str) -> None:
    """path の SHA-256 が expected でなければ ValueError（別のファイルを黙って使わないため）。"""
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    actual = h.hexdigest()
    if actual != expected:
        raise ValueError(f"{path.name} の SHA-256 が違う（実際 {actual}、期待 {expected}）")


@dataclass
class Sam2Sessions:
    vision_encoder: OnnxGraph
    mask_decoder: OnnxGraph
    memory_encoder: OnnxGraph
    memory_attention: OnnxGraph
    pointer_tpos: OnnxGraph


def load_sessions(model_dir: Path, device: Literal["cpu", "cuda"], mask_decoder_path: Path) -> Sam2Sessions:
    """5 グラフを読み込む。device="cuda" で CUDA EP が使えなければ例外。

    model_dir の onnx/ から GRAPH_NAMES の 4 つ、mask_decoder_path からマスクデコーダ（MASK_DECODER_SHA256 と照合してから）。
    """
    verify_sha256(mask_decoder_path, MASK_DECODER_SHA256)
    if device == "cuda" and hasattr(ort, "preload_dlls"):
        ort.preload_dlls()
    graphs = {g: OnnxGraph(model_dir / "onnx" / f"{g}.onnx", device) for g in GRAPH_NAMES}
    return Sam2Sessions(mask_decoder=OnnxGraph(mask_decoder_path, device), **graphs)
