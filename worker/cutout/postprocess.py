"""マスクの後処理: いちばん大きい塊（体）だけを残し、それ以外の塊を全部消す（2026-11-11。オーナー判断）。

同じ服の人が重なる動画では、SAM2 が隣の人の腕や手の一部を拾い、被写体から離れた小さな塊として出ることが多い
（docs/video-cutout-measurements.md）。モデルを大きくしても直らなかったので、出力の手前で消す。

規則（1 フレームごと、モデルの解像度 S×S のロジットで判定する）:
- ロジット > 0 の画素を 8 近傍でつないだ塊のうち、いちばん大きいものだけを残す
- それ以外は全部消す（ロジットを REMOVED_LOGIT 以下にする → alpha はほぼ 0）

以前（2026-11-09）は「前のフレームで残した範囲の近くの塊は残す」規則だったが、オーナーの判断で単純なこの規則にした。
分かっている副作用（承知の上）:
- 速く振った手がブレて腕から離れて写ったフレームでは、本人の手も消える
- 2 人以上を選んで切り抜くと、いちばん大きい 1 人しか残らない

追跡（SAM2 の記憶）には使わない。出力の合成の直前にだけかける（消しても、後のフレームの追跡は変わらない）。
利用者が点を打ったフレーム（1 枚目を含む）にはかけない（プレビューと出力を一致させる約束。CLAUDE.md の 16）。
"""

from __future__ import annotations

import cv2
import numpy as np

REMOVED_LOGIT = -20.0  # EDGE_SHARPNESS=16 で alpha ≒ 0（sigmoid(-320)）


def keep_largest_blob(logits: np.ndarray) -> tuple[np.ndarray, int]:
    """(後処理したロジット, 消した画素数) を返す。logits は [S,S] のロジット（書き換えない）。"""
    fg = logits > 0
    n, labels, stats, _ = cv2.connectedComponentsWithStats(fg.astype(np.uint8), connectivity=8)
    if n <= 2:  # 塊が 0 個か 1 個
        return logits, 0
    largest = 1 + int(np.argmax(stats[1:, cv2.CC_STAT_AREA]))
    removed = fg & (labels != largest)
    out = logits.copy()
    out[removed] = np.minimum(out[removed], REMOVED_LOGIT)
    return out, int(removed.sum())
