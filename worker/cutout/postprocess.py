"""マスクの後処理: 見えている範囲のうち、いちばん大きい塊（体）だけを残し、それ以外を全部消す（オーナー判断）。

同じ服の人が重なる動画では、SAM2 が隣の人の腕や手の一部を拾い、被写体から離れた小さな塊として出ることが多い
（docs/video-cutout-measurements.md）。モデルを大きくしても直らなかったので、出力の手前で消す。

規則（1 フレームごと、出力の解像度の alpha で判定する）:
- alpha が VISIBLE_ALPHA より大きい画素（= 8bit にして 1 以上になる、目に見える画素）を 8 近傍でつないだ塊のうち、
  いちばん大きいものだけを残す。それ以外の塊の alpha は 0 にする

経緯:
- 2026-11-09: 「前のフレームで残した範囲の近くの塊は残す」規則（モデルの 1024 の格子のロジットで判定）
- 2026-11-11: オーナーの判断で「いちばん大きい塊だけ」に単純化
- 2026-11-11（同日）: 1024 の格子のロジット（> 0）で判定すると、出力の大きさに縮めた後に体から離れた点や切れ端が残った
  （D_man の 21〜37 フレームで最大 163 画素。ロジットは 0 未満でも alpha が半透明で見える所や、細い糸でつながっていた所）。
  そこで最後の alpha（shrink_edge の後）で判定するようにした。プレビュー（Rust の video_cutout/preview/image_math.rs の
  keep_largest_region）でも同じ処理をするので、点を打ったフレームを含む全部のフレームにかけてもプレビューと一致する

分かっている副作用（承知の上）:
- 速く振った手がブレて腕から離れて写ったフレームでは、本人の手も消える
- 2 人以上を選んで切り抜くと、いちばん大きい 1 人しか残らない

追跡（SAM2 の記憶）には使わない。出力の合成の直前にだけかける（消しても、後のフレームの追跡は変わらない）。
"""

from __future__ import annotations

import cv2
import numpy as np

VISIBLE_ALPHA = 0.5 / 255.0  # これより大きい alpha は 8bit にすると 1 以上（目に見える）


def keep_largest_region(alpha: np.ndarray) -> np.ndarray:
    """[H,W] float32 の alpha から、いちばん大きい塊以外を消したものを返す（入力は書き換えない。消す所が無ければ同じ配列）。"""
    visible = alpha > VISIBLE_ALPHA
    n, labels, stats, _ = cv2.connectedComponentsWithStats(visible.astype(np.uint8), connectivity=8)
    if n <= 2:  # 塊が 0 個か 1 個
        return alpha
    largest = 1 + int(np.argmax(stats[1:, cv2.CC_STAT_AREA]))  # 同じ大きさなら先に見つかった（上・左の）塊
    out = alpha.copy()
    out[visible & (labels != largest)] = 0.0
    return out
