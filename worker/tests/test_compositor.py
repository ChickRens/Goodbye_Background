from pathlib import Path

import cv2
import numpy as np

from cutout.compositor import EDGE_SHRINK_PX, LogoBadge, alpha_from_logits, composite, shrink_edge

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


# ---- 縁を縮めて滑らかにする（shrink_edge。2026-11-11）----


def step_alpha(width=100, edge=50):
    a = np.zeros((40, width), np.float32)
    a[:, edge:] = 1.0
    return a


def test_shrink_edge_moves_straight_edge_inward_by_shrink_px():
    out = shrink_edge(step_alpha())[20]
    cross = np.argmax(out >= 0.5)  # 0.5 を超える最初の列
    assert abs(cross - (50 + EDGE_SHRINK_PX)) <= 0.6
    assert out[:45].max() == 0.0 and out[60:].min() == 1.0  # 縁から離れた所は完全に背景・前景のまま


def test_shrink_edge_scales_with_px_scale():
    out = shrink_edge(step_alpha(200, 100), px_scale=2.0)[20]
    assert abs(np.argmax(out >= 0.5) - (100 + 2 * EDGE_SHRINK_PX)) <= 1.1


def test_shrink_edge_ramp_is_about_one_pixel():
    out = shrink_edge(step_alpha())[20]
    partial = ((out > 0.02) & (out < 0.98)).sum()
    assert 1 <= partial <= 2  # 半透明は 1〜2 画素だけ（CapCut のクロマキーで穴が開かない）


def test_shrink_edge_smooths_staircase():
    def crossings(step, period):
        a = np.zeros((60, 100), np.float32)
        for y in range(60):
            a[y, 40 + (y // period) % 2 * step:] = 1.0
        out = shrink_edge(a)
        return np.array([np.argmax(r >= 0.5) for r in out[10:50]])

    fine = crossings(1, 2)  # 1 画素のギザギザ（2 行ごとに出入り）→ まっすぐになる
    assert fine.max() == fine.min()
    coarse = crossings(3, 6)  # 3 画素の段差 → 隣の行との差が 1 画素以下のなだらかな坂になる
    assert np.abs(np.diff(coarse)).max() <= 1


def test_shrink_edge_keeps_empty_and_full():
    assert shrink_edge(np.zeros((20, 20), np.float32)).max() == 0.0
    full = shrink_edge(np.ones((20, 20), np.float32))
    assert full.min() == 1.0 and full.dtype == np.float32
