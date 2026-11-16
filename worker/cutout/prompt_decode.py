"""マスクデコーダの呼び方（点を打ったフレーム・追跡中のフレーム）。

点を打ったフレームでは、点を打った順に 1 点ずつ「前回のマスク」を渡して積み上げる（公式の SAM2 の動画 predictor の
add_new_points_or_box と同じ。画像の切り抜きとも同じ考え方）。点をまとめて 1 回で渡すと、離れた点を 1 つの物として
解釈して人物が消えることがあった（仕様 docs/superpowers/specs/2026-11-16-video-prompt-mask-input-design.md）。

Rust のプレビュー（src/video_cutout/preview/segmenter_sam2.rs の decode）も同じ順で同じ計算をする。変えるときは両方直す。
"""

from __future__ import annotations

import numpy as np

from cutout.contract import NormalizedPoint

MASK_INPUT_CLIP = 32.0  # 前回の low_res_mask を -32〜32 に丸めて渡す（公式と同じ。被写体なしの -1024 も -32 になる）
_LOW_RES = (1, 1, 256, 256)


def _run(decoder, feats0, feats1, feats2_cond, coords, labels, prev: np.ndarray | None) -> dict:
    if prev is None:
        mask_input, has_mask = np.zeros(_LOW_RES, np.float32), np.array([0.0], np.float32)
    else:
        mask_input = np.clip(np.asarray(prev, np.float32).reshape(_LOW_RES), -MASK_INPUT_CLIP, MASK_INPUT_CLIP)
        has_mask = np.array([1.0], np.float32)
    return decoder.run(
        feats0=feats0, feats1=feats1, feats2_cond=feats2_cond,
        input_points=coords, input_labels=labels, mask_input=mask_input, has_mask=has_mask,
    )


def decode_points(decoder, feats0, feats1, feats2_cond, points: list[NormalizedPoint], size: int) -> dict:
    """点の列の先頭 1 個, 2 個, … n 個で順にデコードし、前回の low_res_mask を次に渡す。最後の出力を返す。"""
    if not points:
        raise ValueError("点が必要")
    out: dict | None = None
    for i in range(1, len(points) + 1):
        head = points[:i]
        coords = np.array([[[[p.x * size, p.y * size] for p in head]]], np.float32)
        labels = np.array([[[1 if p.label == "include" else 0 for p in head]]], np.int32)
        out = _run(decoder, feats0, feats1, feats2_cond, coords, labels, None if out is None else out["low_res_mask"])
    return out


def decode_tracking(decoder, feats0, feats1, feats2_cond) -> dict:
    """追跡中のフレーム: ダミー点（label -1）1 つ・前回のマスクなし。"""
    coords = np.zeros((1, 1, 1, 2), np.float32)
    labels = np.full((1, 1, 1), -1, np.int32)
    return _run(decoder, feats0, feats1, feats2_cond, coords, labels, None)
