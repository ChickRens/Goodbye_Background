"""SAM2.1-tiny 動画 ONNX モデルの取得と ONNX Runtime セッションの生成。"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Literal

import httpx
import numpy as np
import onnxruntime as ort

# Hugging Face のコミットハッシュに固定する（main は動くため再現性が無い）。Modal での計測に使ったもの。
MODEL_REVISION = "66673b5db39371b7dd7847f4d3bc0d4f4179b79e"
REPO_URL = "https://huggingface.co/diffusionstudio/sam2.1-tiny-video-onnx-fp16/resolve/{revision}/"
GRAPH_NAMES = ["vision_encoder", "mask_decoder", "memory_encoder", "memory_attention", "pointer_tpos"]

_NP_TYPES = {
    "tensor(float)": np.float32,
    "tensor(float16)": np.float16,
    "tensor(int32)": np.int32,
    "tensor(int64)": np.int64,
    "tensor(bool)": np.bool_,
}


def fetch_models(model_dir: Path, revision: str) -> Path:
    """constants.json と onnx/*.onnx を model_dir に取得する。既にあるファイルは飛ばす。"""
    base = REPO_URL.format(revision=revision)
    files = ["constants.json"] + [f"onnx/{g}.onnx" for g in GRAPH_NAMES]
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

    def __init__(self, path: Path, device: Literal["cpu", "cuda"], threads: int | None = None):
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


@dataclass
class Sam2Sessions:
    vision_encoder: OnnxGraph
    mask_decoder: OnnxGraph
    memory_encoder: OnnxGraph
    memory_attention: OnnxGraph
    pointer_tpos: OnnxGraph


def load_sessions(model_dir: Path, device: Literal["cpu", "cuda"]) -> Sam2Sessions:
    """5 グラフを読み込む。device="cuda" で CUDA EP が使えなければ例外。"""
    if device == "cuda" and hasattr(ort, "preload_dlls"):
        ort.preload_dlls()
    return Sam2Sessions(
        **{g: OnnxGraph(model_dir / "onnx" / f"{g}.onnx", device) for g in GRAPH_NAMES}
    )
