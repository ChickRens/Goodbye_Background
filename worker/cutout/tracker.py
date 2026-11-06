"""SAM2 動画トラッカー。ONNX の 5 グラフでフレームを 1 枚ずつ順に処理する。

手順（仕様 §6.4）:
  1 枚目: 画像エンコード → クリック点でマスクデコード → binarize=1 でメモリ化（条件メモリ）
  2 枚目以降: 画像エンコード → メモリ注意で条件付け → ダミー点でデコード → binarize=0 でメモリ化
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Protocol

import cv2
import numpy as np

from cutout.contract import NormalizedPoint
from cutout.memory import (
    MAX_PTRS,
    MEM_FRAMES,
    MemoryEntry,
    assemble_memory,
    pointer_list,
    pointer_pos_tokens,
    pointer_tokens,
)

S = 512  # モデル入力の一辺
F = 32  # 特徴マップの一辺
RECENT_FRAMES = MEM_FRAMES - 1  # 保持する直近メモリ数（6）
RECENT_PTRS = MAX_PTRS - 1  # 保持する直近ポインタ数（15）


class Graph(Protocol):
    def run(self, **feeds) -> dict: ...


class Sam2Sessions(Protocol):
    """5 つのグラフ。テストでは偽物を差し込める。"""

    vision_encoder: Graph
    mask_decoder: Graph
    memory_encoder: Graph
    memory_attention: Graph
    pointer_tpos: Graph


@dataclass(frozen=True)
class FrameResult:
    high_res_logits: np.ndarray  # [512,512] float32 のロジット
    object_present: bool  # object_score_logits > 0


def to_pixel_values(frame_rgb: np.ndarray, mean, std) -> np.ndarray:
    """RGB フレームを 512x512 に縮め、正規化して [1,3,512,512] にする。"""
    x = cv2.resize(frame_rgb, (S, S), interpolation=cv2.INTER_LINEAR)
    x = (x.astype(np.float32) / 255.0 - np.array(mean, np.float32)) / np.array(std, np.float32)
    return x.transpose(2, 0, 1)[None]


def chw_to_tokens(feats: np.ndarray) -> np.ndarray:
    """[1,C,32,32] を [1024,1,C] に並べ替える。"""
    c = feats.shape[1]
    return feats.reshape(c, F * F).T.reshape(F * F, 1, c)


class Sam2VideoTracker:
    def __init__(self, sessions: Sam2Sessions, constants: dict, total_frames: int):
        self._s = sessions
        self._mean = constants["image_mean"]
        self._std = constants["image_std"]
        self._tpos = np.asarray(
            constants["memory_temporal_positional_encoding"], np.float32
        ).reshape(MEM_FRAMES, 64)
        self._total = total_frames
        self._t = 0
        self._cond: MemoryEntry | None = None
        self._cond_ptr: np.ndarray | None = None
        self._recent: dict[int, MemoryEntry] = {}
        self._recent_ptrs: dict[int, np.ndarray] = {}
        # 直近フレームの low-res ロジット [256,256]（移植の検証用）
        self.last_low_res: np.ndarray | None = None

    def process(
        self, frame_rgb: np.ndarray, points: list[NormalizedPoint] | None
    ) -> FrameResult:
        """1 フレーム処理する。1 枚目だけ points を渡し、2 枚目以降は None。"""
        t = self._t
        first = t == 0
        if first and not points:
            raise ValueError("1 枚目にはクリック点が必要")
        enc = self._s.vision_encoder.run(
            pixel_values=to_pixel_values(frame_rgb, self._mean, self._std)
        )
        if first:
            feats_cond = enc["feats2_no_mem"]
            coords = np.array([[[[p.x * S, p.y * S] for p in points]]], np.float32)
            labels = np.array([[[1 if p.label == "include" else 0 for p in points]]], np.int32)
        else:
            mem, pos = assemble_memory(t, self._cond, self._recent, self._tpos)
            ptrs, diffs = pointer_list(t, self._total, self._cond_ptr, self._recent_ptrs)
            ppos = self._s.pointer_tpos.run(normalized_diffs=diffs)["pointer_pos"]
            mem = np.concatenate([mem, pointer_tokens(ptrs)])
            pos = np.concatenate([pos, pointer_pos_tokens(ppos)])
            feats_cond = self._s.memory_attention.run(
                current_vision_features=chw_to_tokens(enc["feats2"]),
                current_vision_position_embeddings=chw_to_tokens(enc["vision_pos_embed"]),
                memory=mem[:, None, :],
                memory_pos=pos[:, None, :],
            )["conditioned_feats"]
            coords = np.zeros((1, 1, 1, 2), np.float32)
            labels = np.full((1, 1, 1), -1, np.int32)
        dec = self._s.mask_decoder.run(
            feats0=enc["feats0"],
            feats1=enc["feats1"],
            feats2_cond=feats_cond,
            input_points=coords,
            input_labels=labels,
        )
        score = float(np.asarray(dec["object_score_logits"]).reshape(-1)[0])
        me = self._s.memory_encoder.run(
            feats2=enc["feats2"],
            high_res_mask=dec["high_res_mask"],
            object_score_logits=[[score]],
            binarize=1 if first else 0,
        )
        entry = MemoryEntry(tokens=me["memory_tokens"], pos=me["memory_pos"])
        ptr = np.asarray(dec["object_pointer"]).reshape(-1)[:256]
        if first:
            self._cond, self._cond_ptr = entry, ptr
        else:
            self._recent[t] = entry
            self._recent_ptrs[t] = ptr
            self._recent.pop(t - RECENT_FRAMES, None)
            self._recent_ptrs.pop(t - RECENT_PTRS, None)
        self.last_low_res = np.asarray(dec["low_res_mask"], np.float32).reshape(256, 256)
        self._t += 1
        high = np.asarray(dec["high_res_mask"], np.float32).reshape(S, S)
        return FrameResult(high_res_logits=high, object_present=score > 0)
