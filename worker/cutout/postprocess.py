"""マスクの後処理: 体から離れた塊（隣の人の腕の切れ端など）を、前のフレームを手がかりに消す（2026-11-09）。

同じ服の人が重なる動画では、SAM2 が隣の人の腕や肩の一部を拾い、被写体から離れた小さな塊として出ることが多い
（docs/video-cutout-measurements.md）。モデルを大きくしても直らなかったので、出力の手前で消す。

規則（1 フレームごと、モデルの解像度 S×S のロジットで判定する）:
- いちばん大きい塊（体）は必ず残す
- 前のフレームで残した範囲から NEAR_RATIO × S 画素以内に入る塊は残す（速く動く手が体から離れて写っても、
  前のフレームの手の近くにあれば残る）
- それ以外の塊は消す（ロジットを REMOVED_LOGIT 以下にする → alpha はほぼ 0）

追跡（SAM2 の記憶）には使わない。出力の合成の直前にだけかける（間違えて消しても、後のフレームの追跡は変わらない）。
1 フレーム目は触らない（プレビューと出力の 1 フレーム目を一致させる約束。CLAUDE.md の 16）。
"""

from __future__ import annotations

import cv2
import numpy as np

# 前のフレームで残した範囲から、この割合 × モデルの一辺（small は 1024 → 約 61 画素）以内の塊は残す。
# 30fps の踊りで手が 1 フレームに動く量（出力 576 幅で 30〜40 画素 ≒ 1024 の格子で 55〜70 画素）に合わせた
NEAR_RATIO = 0.06
REMOVED_LOGIT = -20.0  # EDGE_SHARPNESS=16 で alpha ≒ 0（sigmoid(-320)）


class DetachedBlobFilter:
    """1 本の動画で 1 つ作り、フレームの順に apply を呼ぶ。"""

    def __init__(self, near_ratio: float = NEAR_RATIO):
        self._near_ratio = near_ratio
        self._prev: np.ndarray | None = None  # 前のフレームで残した範囲（bool, S×S）

    def reset(self, logits: np.ndarray) -> None:
        """利用者が点を打ったフレーム: 消さずに、その範囲を「前のフレームで残した範囲」にする（プレビューと一致させる）。"""
        self._prev = logits > 0

    def apply(self, logits: np.ndarray) -> tuple[np.ndarray, int]:
        """(後処理したロジット, 消した画素数) を返す。logits は [S,S] のロジット（書き換えない）。"""
        fg = logits > 0
        if self._prev is None:
            self._prev = fg  # 1 フレーム目は触らない
            return logits, 0
        n, labels, stats, _ = cv2.connectedComponentsWithStats(fg.astype(np.uint8), connectivity=8)
        if n <= 2:  # 塊が 0 個か 1 個
            self._prev = fg
            return logits, 0
        keep = np.zeros(n, bool)
        keep[1 + int(np.argmax(stats[1:, cv2.CC_STAT_AREA]))] = True
        if self._prev.any():
            radius = self._near_ratio * logits.shape[0]
            # 前のフレームの範囲からの距離（範囲の中は 0）
            dist = cv2.distanceTransform((~self._prev).astype(np.uint8), cv2.DIST_L2, 3)
            keep[np.unique(labels[(dist <= radius) & fg])] = True
        keep[0] = False  # 背景のラベル
        removed = fg & ~keep[labels]
        count = int(removed.sum())
        self._prev = fg & ~removed
        if count == 0:
            return logits, 0
        out = logits.copy()
        out[removed] = np.minimum(out[removed], REMOVED_LOGIT)
        return out, count
