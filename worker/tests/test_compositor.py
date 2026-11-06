from pathlib import Path

import cv2
import numpy as np

from cutout.compositor import LogoBadge, alpha_from_logits, composite

LOGO = Path(__file__).resolve().parent.parent / "cutout" / "assets" / "logo.png"


def alpha_without_erode(logits, width, height):
    """erode を省いた同じ計算（比較用）。"""
    up = cv2.resize(logits, (width, height), interpolation=cv2.INTER_LINEAR)
    return 1.0 / (1.0 + np.exp(-up))


def test_alpha_is_probability_and_shrinks_edge():
    logits = np.full((512, 512), -20, np.float32)
    logits[:, 256:] = 20
    a = alpha_from_logits(logits, 100, 50)
    assert a.shape == (50, 100) and a.min() >= 0 and a.max() <= 1
    assert a[25, 99] > 0.99 and a[25, 0] < 0.01
    assert a[25, 50] < alpha_without_erode(logits, 100, 50)[25, 50]  # 縁が内側に削れている


def test_composite_lost_frame_is_full_background():
    out = composite(np.full((4, 4, 3), 200, np.uint8), None, (0, 255, 0))
    assert np.all(out == [0, 255, 0])


def test_composite_blends():
    out = composite(np.full((1, 1, 3), 100, np.uint8), np.array([[0.5]], np.float32), (0, 0, 0))
    assert out[0, 0, 0] == 50


def test_logo_badge_position_and_opacity():
    badge = LogoBadge.load(LOGO)
    out = badge.apply(np.zeros((1080, 1920, 3), np.uint8) + np.array([0, 255, 0], np.uint8))
    size, margin = round(1080 * 0.08), round(1080 * 0.02)
    cx, cy = 1920 - margin - size // 2, 1080 - margin - size // 2
    assert tuple(out[cy, cx]) != (0, 255, 0)  # バッジの中心は背景色ではない
    assert tuple(out[1080 - margin - size - 2, cx]) == (0, 255, 0)  # バッジの外は元のまま
