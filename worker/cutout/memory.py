"""SAM2 動画トラッキングのメモリバンクとオブジェクトポインタの組み立て（純粋関数）。

条件メモリは常にフレーム 0。`recent` / `recent_ptrs` は絶対フレーム番号をキーにする。
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

import numpy as np

MEM_FRAMES = 7  # 条件メモリ 1 + 直近 6 フレーム
MAX_PTRS = 16
COND_ROW = 6  # 条件メモリの時間埋め込みの行
COND_FRAME = 0


@dataclass(frozen=True)
class MemoryEntry:
    """1 フレーム分のメモリ。tokens / pos はどちらも [F*F, 1, 64]（512 で 1024、1024 で 4096）。"""

    tokens: np.ndarray
    pos: np.ndarray


def memory_blocks(
    t: int, recent: dict[int, MemoryEntry]
) -> list[tuple[Literal["cond"] | int, int]]:
    """(どのメモリか, 時間埋め込みの行) を MEM_FRAMES 個返す。

    足りない分は最新の過去フレーム（無ければ条件メモリ）を、その行ごと複製して埋める。
    """
    blocks: list[tuple[Literal["cond"] | int, int]] = [("cond", COND_ROW)]
    for k in range(1, MEM_FRAMES):
        f = t - k
        if f < 0:
            break
        if f == COND_FRAME:
            continue
        if f in recent:
            blocks.append((f, k - 1))
    newest = blocks[1] if len(blocks) > 1 else blocks[0]
    while len(blocks) < MEM_FRAMES:
        blocks.append(newest)
    return blocks


def assemble_memory(
    t: int,
    cond: MemoryEntry,
    recent: dict[int, MemoryEntry],
    tpos: np.ndarray,
) -> tuple[np.ndarray, np.ndarray]:
    """メモリトークン [7*F*F,64] と、時間埋め込みを足した位置埋め込み [7*F*F,64] を返す。"""
    blocks = memory_blocks(t, recent)
    entries = [(cond if which == "cond" else recent[which], row) for which, row in blocks]
    mem = np.concatenate([e.tokens.reshape(-1, 64) for e, _ in entries])
    pos = np.concatenate([e.pos.reshape(-1, 64) + tpos[row][None] for e, row in entries])
    return mem, pos


def pointer_list(
    t: int,
    total: int,
    cond_ptr: np.ndarray,
    recent_ptrs: dict[int, np.ndarray],
) -> tuple[np.ndarray, np.ndarray]:
    """ポインタ [16,256] と、フレーム差を min(total,16)-1 で割った [16] を返す。"""
    max_ptrs = min(total, MAX_PTRS)
    denom = max(max_ptrs - 1, 1)
    lst = [(cond_ptr, abs(t - COND_FRAME))]
    for d in range(1, max_ptrs):
        f = t - d
        if f < 0:
            break
        if f == COND_FRAME:
            continue
        if f in recent_ptrs:
            lst.append((recent_ptrs[f], d))
    newest = lst[1] if len(lst) > 1 else lst[0]
    while len(lst) < MAX_PTRS:
        lst.append(newest)
    ptrs = np.stack([p for p, _ in lst])
    diffs = np.array([d / denom for _, d in lst], np.float32)
    return ptrs, diffs


def pointer_tokens(ptrs: np.ndarray) -> np.ndarray:
    """[16,256] を 64 次元 4 トークンに分割して [64,64] にする。"""
    return ptrs.reshape(MAX_PTRS * 4, 64)


def pointer_pos_tokens(pointer_pos: np.ndarray) -> np.ndarray:
    """[16,64] の各行を 4 回繰り返して [64,64] にする。"""
    return np.repeat(pointer_pos, 4, axis=0)
