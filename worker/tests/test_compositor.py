from pathlib import Path

import cv2
import numpy as np

from cutout.compositor import LogoBadge, alpha_from_logits, composite

LOGO = Path(__file__).resolve().parent.parent / "cutout" / "assets" / "logo.png"


def test_alpha_is_probability_range_and_shape():
    logits = np.full((512, 512), -20, np.float32)
    logits[:, 256:] = 20
    a = alpha_from_logits(logits, 100, 50)
    assert a.shape == (50, 100) and a.dtype == np.float32 and a.min() >= 0 and a.max() <= 1
    assert a[25, 99] > 0.99 and a[25, 0] < 0.01


def test_alpha_logit_zero_is_half():
    a = alpha_from_logits(np.zeros((512, 512), np.float32), 10, 10)
    assert np.allclose(a, 0.5, atol=1e-6)


def test_alpha_is_nearly_binary_for_small_logits():
    assert alpha_from_logits(np.full((512, 512), 0.25, np.float32), 8, 8).min() > 0.97
    assert alpha_from_logits(np.full((512, 512), -0.25, np.float32), 8, 8).max() < 0.03


def test_alpha_step_edge_does_not_shrink():
    logits = np.full((512, 512), -20, np.float32)
    logits[:, 256:] = 20
    a = alpha_from_logits(logits, 100, 50)[25]
    plain = 1.0 / (1.0 + np.exp(-cv2.resize(logits, (100, 50), interpolation=cv2.INTER_LINEAR)))[25]
    cross = int(np.argmax(a >= 0.5))
    cross_plain = int(np.argmax(plain >= 0.5))
    assert abs(cross - 50) <= 1 and abs(cross - cross_plain) <= 1  # 元の境界（列 50）から内側へ縮まない


def test_alpha_extreme_logits_do_not_warn():
    import warnings

    logits = np.full((512, 512), -1024, np.float32)
    logits[:, 256:] = 1024
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        a = alpha_from_logits(logits, 100, 50)
    assert a[25, 0] < 1e-6 and a[25, 99] > 1 - 1e-6


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
