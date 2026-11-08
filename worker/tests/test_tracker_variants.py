"""tracker_variants（案 C の比較用の追跡）のテスト。記憶の選び方と、DAM4SAM / SAMURAI の判定の部品を確かめる。

本番の追跡（tracker.py）と policy="sam2" が同じ結果になることは、実モデルの遅いテスト（最後）で確かめる。
"""

import os
from pathlib import Path

import numpy as np
import pytest

from cutout.memory import COND_ROW, MEM_FRAMES, MemoryEntry
from cutout.tracker_variants import (
    BoxKalman,
    VariantTracker,
    _Frame,
    box_iou,
    dam_has_distractor,
    mask_box,
    select_closest_cond,
    xyah_to_xyxy,
    xyxy_to_xyah,
)

CONSTANTS = {
    "image_mean": [0.485, 0.456, 0.406],
    "image_std": [0.229, 0.224, 0.225],
    "memory_temporal_positional_encoding": np.zeros((7, 1, 1, 64)).tolist(),
}


def frame(n_pixels=100, best_iou=0.9, obj_score=1.0, kf_score=None) -> _Frame:
    z = np.zeros((4, 1, 64), np.float32)
    return _Frame(MemoryEntry(z, z), np.zeros(256, np.float32), n_pixels, best_iou, obj_score, kf_score)


def tracker(policy, hist: dict[int, _Frame], drm=()) -> VariantTracker:
    tr = VariantTracker(sessions=None, constants=CONSTANTS, total_frames=300, policy=policy)
    tr._hist = hist
    tr._drm = list(drm)
    return tr


def test_unknown_policy_rejected():
    with pytest.raises(ValueError):
        VariantTracker(sessions=None, constants=CONSTANTS, total_frames=10, policy="xmem")


def test_mask_box_and_iou():
    m = np.zeros((10, 10))
    m[2:5, 3:8] = 1
    assert mask_box(m) == [3.0, 2.0, 7.0, 4.0]
    assert mask_box(np.zeros((4, 4))) == [0.0, 0.0, 0.0, 0.0]
    assert box_iou([0, 0, 2, 2], [0, 0, 2, 2]) == 1.0
    assert box_iou([0, 0, 2, 2], [1, 0, 3, 2]) == pytest.approx(1 / 3)
    assert box_iou([0, 0, 1, 1], [5, 5, 6, 6]) == 0.0


def test_xyah_round_trip():
    b = [10.0, 20.0, 30.0, 60.0]
    assert xyah_to_xyxy(xyxy_to_xyah(b)) == pytest.approx(b)


def test_kalman_follows_constant_motion():
    kf = BoxKalman()
    x, P = kf.initiate(xyxy_to_xyah([0, 0, 10, 20]))
    for step in range(1, 30):
        x, P = kf.predict(x, P)
        x, P = kf.update(x, P, xyxy_to_xyah([2 * step, 0, 2 * step + 10, 20]))
    x, _ = kf.predict(x, P)  # 次のフレームの予測は、さらに右へ約 2 画素
    assert xyah_to_xyxy(x)[0] == pytest.approx(60, abs=1.0)


def test_select_closest_cond_like_dam4sam():
    # フレーム 0・t-1 より前で最も近い DRM・残りを近い順。t-1 の DRM は RAM 側に回すので選ばない
    assert select_closest_cond(50, [0, 10, 30, 49], 4) == [30, 0, 10]
    assert select_closest_cond(50, [0, 5, 10, 20, 30, 40], 4) == [40, 0, 30, 20]
    assert select_closest_cond(3, [0], 4) == [0]


def test_dam_distractor_when_alternative_reaches_outside():
    chosen = np.zeros((20, 20), bool)
    chosen[5:15, 5:10] = True
    inside = chosen.copy()  # 選んだマスクの中だけ → 似た物体なし
    assert not dam_has_distractor(chosen, [inside])
    far = chosen.copy()
    far[5:15, 14:20] = True  # 右に離れた塊（隣の人）まで含む候補 → 箱が広がる
    assert dam_has_distractor(chosen, [inside, far])


@pytest.mark.parametrize("t", [1, 2, 3, 7, 10, 50])
def test_sam2_blocks_match_production_memory_blocks(t):
    from cutout.memory import memory_blocks

    hist = {f: frame() for f in range(0, t)}
    recent = {f: hist[f].entry for f in range(1, t)}
    expected = [(0 if which == "cond" else which, row) for which, row in memory_blocks(t, recent)]
    assert tracker("sam2", hist)._blocks_sam2(t) == expected


def test_samurai_skips_unreliable_frames():
    hist = {f: frame() for f in range(0, 20)}
    hist[18] = frame(best_iou=0.4)  # 確からしさが低い
    hist[17] = frame(obj_score=-0.5)  # 被写体らしくない
    hist[16] = frame(kf_score=-0.1)  # 動きの予測と合わない
    blocks = tracker("samurai", hist)._blocks_samurai(20)
    used = [f for f, _ in blocks[1:]]
    assert 19 in used  # 直前のフレームは常に使う
    assert not {16, 17, 18} & set(used)
    # 条件を満たす直近の 5 フレーム + 直前のフレーム（SAMURAI は valid の後ろ 6 個を使う）
    assert set(used) == {11, 12, 13, 14, 15, 19}
    assert len(blocks) == MEM_FRAMES


def test_dam4sam_uses_drm_as_conditioning_and_strided_ram():
    hist = {f: frame() for f in range(0, 40)}
    tr = tracker("dam4sam", hist, drm=[12])
    blocks, selected, unselected = tr._blocks_dam4sam(40)
    assert selected == [12, 0] and unselected == set()
    rows = dict(blocks)
    assert rows[0] == COND_ROW and rows[12] == COND_ROW  # DRM は条件メモリと同じ時間埋め込み
    ram = sorted(f for f, r in blocks if r != COND_ROW)
    assert ram == [20, 25, 30, 35, 39]  # 直前（39）と 5 フレームおき
    assert len(blocks) == MEM_FRAMES


def test_dam4sam_ram_skips_frames_without_target():
    hist = {f: frame() for f in range(0, 40)}
    hist[39] = frame(n_pixels=0)  # 直前に被写体が見えていない → さかのぼる
    hist[35] = frame(n_pixels=0)
    blocks, _, _ = tracker("dam4sam", hist)._blocks_dam4sam(40)
    ram = {f for f, r in blocks if r != COND_ROW}
    assert 39 not in ram and 38 in ram
    assert 35 not in ram and 30 in ram


@pytest.mark.slow
def test_sam2_policy_matches_production_tracker_on_real_model():
    """実モデル（SAM2_MODEL_DIR）で、policy="sam2" が本番の Sam2VideoTracker と同じ出力になる。"""
    from cutout.contract import NormalizedPoint
    from cutout.models import load_constants, load_sessions
    from cutout.tracker import Sam2VideoTracker
    from cutout.tracker_variants import CANDIDATE_OUTPUTS

    d = os.environ.get("SAM2_MODEL_DIR")
    if not d:
        pytest.skip("SAM2_MODEL_DIR が無い")
    d = Path(d)
    c = load_constants(d)
    rng = np.random.default_rng(0)
    base = np.clip(rng.normal(120, 30, (360, 240, 3)), 0, 255).astype(np.uint8)
    frames = [np.roll(base, 3 * i, axis=1) for i in range(12)]
    pts = [NormalizedPoint(0.5, 0.5, "include")]
    a = Sam2VideoTracker(load_sessions(d, "cpu"), c, 12)
    b = VariantTracker(load_sessions(d, "cpu", decoder_extra_outputs=CANDIDATE_OUTPUTS), c, 12, "sam2")
    for i, f in enumerate(frames):
        ra, rb = a.process(f, pts if i == 0 else None), b.process(f, pts if i == 0 else None)
        assert ra.object_present == rb.object_present
        assert np.abs(ra.high_res_logits - rb.high_res_logits).max() < 1e-4
