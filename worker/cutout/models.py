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
# 同じ形式（5 グラフ・同じ constants.json の項目）で書き出された大きいモデル。違いは入力の解像度と画像エンコーダの大きさ。
# 案 C（大きいモデル）の比較用。名前 → (Hugging Face のリポジトリ, コミットハッシュ)。2026-11-07 に API で確認
VIDEO_MODELS = {
    "tiny": ("diffusionstudio/sam2.1-tiny-video-onnx-fp16", MODEL_REVISION),  # 512。vision_encoder 58MB
    "small": ("diffusionstudio/sam2.1-small-video-onnx-fp16", "927ecec6e6ae2727d0af7720eefdb07f2efe4689"),  # 1024。84MB
    "base-plus": ("diffusionstudio/sam2.1-base-plus-video-onnx-fp16", "386b83e09a774fe2782d86fd1b8cb5aaf718aa9d"),  # 1024。156MB
    "large": ("diffusionstudio/sam2.1-large-video-onnx-fp16", "98dbd222d9b3f75d15fe63a5c2a22b0b0029eea1"),  # 1024。447MB
}
GRAPH_NAMES = ["vision_encoder", "mask_decoder", "memory_encoder", "memory_attention", "pointer_tpos"]

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
    repo を省略すると tiny（REPO_URL）。大きいモデルは VIDEO_MODELS のリポジトリを渡す。
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
        extra_outputs: list[str] | None = None,
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
        model = str(path) if not extra_outputs else with_extra_outputs(path, extra_outputs)
        self._s = ort.InferenceSession(model, so, providers=providers)
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


def with_extra_outputs(path: Path, names: list[str]) -> bytes:
    """グラフの途中のテンソルを出力に足した ONNX（バイト列）を返す。名前が無ければ例外。

    案 C の比較用（tracker_variants.py）。マスクデコーダが内部で計算している候補のマスク 4 つと
    その IoU の予測を取り出すのに使う。onnx パッケージは開発用・計測用の依存（本番のワーカーは使わない）。
    """
    import onnx

    model = onnx.load(str(path))
    produced = {o for n in model.graph.node for o in n.output}
    missing = [n for n in names if n not in produced]
    if missing:
        raise ValueError(f"{path.name} に {missing} が無い（書き出し方の違うモデル）")
    for n in names:
        model.graph.output.append(onnx.helper.make_empty_tensor_value_info(n))
    return model.SerializeToString()


@dataclass
class Sam2Sessions:
    vision_encoder: OnnxGraph
    mask_decoder: OnnxGraph
    memory_encoder: OnnxGraph
    memory_attention: OnnxGraph
    pointer_tpos: OnnxGraph


def load_sessions(
    model_dir: Path, device: Literal["cpu", "cuda"], decoder_extra_outputs: list[str] | None = None
) -> Sam2Sessions:
    """5 グラフを読み込む。device="cuda" で CUDA EP が使えなければ例外。

    decoder_extra_outputs: マスクデコーダに足す出力（tracker_variants.CANDIDATE_OUTPUTS）。本番は使わない。
    """
    if device == "cuda" and hasattr(ort, "preload_dlls"):
        ort.preload_dlls()
    return Sam2Sessions(
        **{
            g: OnnxGraph(
                model_dir / "onnx" / f"{g}.onnx",
                device,
                extra_outputs=decoder_extra_outputs if g == "mask_decoder" else None,
            )
            for g in GRAPH_NAMES
        }
    )
