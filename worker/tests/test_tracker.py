"""偽のセッションで Sam2VideoTracker の呼び出し順と入力を確かめる。"""

import numpy as np

from cutout.contract import NormalizedPoint
from cutout.tracker import Sam2VideoTracker

CONSTANTS = {
    "image_mean": [0.485, 0.456, 0.406],
    "image_std": [0.229, 0.224, 0.225],
    "memory_temporal_positional_encoding": np.zeros((7, 1, 1, 64)).tolist(),
}


class _Graph:
    def __init__(self, name, calls, fn):
        self._name, self._calls, self._fn = name, calls, fn

    def run(self, **feeds):
        self._calls.setdefault(self._name, []).append(feeds)
        return self._fn(feeds)


class RecordingSessions:
    """5 つのグラフを偽物に差し替え、受け取った入力を calls に記録する。"""

    def __init__(self, object_score: float, size: int = 512):
        self.calls: dict[str, list[dict]] = {}
        z = np.zeros
        c = self.calls
        f = size // 16  # 特徴マップの一辺（512 → 32、1024 → 64）
        self.vision_encoder = _Graph("vision_encoder", c, lambda _: {
            "feats0": z((1, 32, f * 4, f * 4), np.float32),
            "feats1": z((1, 64, f * 2, f * 2), np.float32),
            "feats2": z((1, 256, f, f), np.float32),
            "feats2_no_mem": z((1, 256, f, f), np.float32),
            "vision_pos_embed": z((1, 256, f, f), np.float32),
        })
        self.memory_attention = _Graph("memory_attention", c, lambda _: {
            "conditioned_feats": z((1, 256, f, f), np.float32),
        })
        self.pointer_tpos = _Graph("pointer_tpos", c, lambda _: {
            "pointer_pos": z((16, 64), np.float32),
        })
        self.mask_decoder = _Graph("mask_decoder", c, lambda _: {
            "object_score_logits": np.array([[object_score]], np.float32),
            "high_res_mask": z((1, 1, size, size), np.float32),
            "low_res_mask": z((1, 1, 256, 256), np.float32),
            "object_pointer": z((1, 256), np.float32),
        })
        self.memory_encoder = _Graph("memory_encoder", c, lambda _: {
            "memory_tokens": z((f * f, 1, 64), np.float32),
            "memory_pos": z((f * f, 1, 64), np.float32),
        })


def rgb():
    return np.zeros((360, 640, 3), np.uint8)


def test_first_frame_uses_points_and_binarize_1():
    s = RecordingSessions(object_score=1.0)
    Sam2VideoTracker(s, CONSTANTS, total_frames=3).process(rgb(), [NormalizedPoint(0.5, 0.25, "include")])
    assert s.calls["mask_decoder"][0]["input_points"].tolist() == [[[[256.0, 128.0]]]]
    assert s.calls["mask_decoder"][0]["input_labels"].tolist() == [[[1]]]
    assert int(np.asarray(s.calls["memory_encoder"][0]["binarize"]).reshape(-1)[0]) == 1
    assert "memory_attention" not in s.calls


def test_exclude_label_is_zero():
    s = RecordingSessions(object_score=1.0)
    pts = [NormalizedPoint(0.5, 0.5, "include"), NormalizedPoint(0.1, 0.2, "exclude")]
    Sam2VideoTracker(s, CONSTANTS, total_frames=1).process(rgb(), pts)
    assert s.calls["mask_decoder"][0]["input_labels"].tolist() == [[[1, 0]]]


def test_second_frame_uses_memory_attention_and_pad_point():
    s = RecordingSessions(object_score=1.0)
    tr = Sam2VideoTracker(s, CONSTANTS, total_frames=3)
    tr.process(rgb(), [NormalizedPoint(0.5, 0.5, "include")])
    tr.process(rgb(), None)
    assert s.calls["memory_attention"][0]["memory"].shape == (7 * 1024 + 64, 1, 64)
    assert s.calls["mask_decoder"][1]["input_labels"].tolist() == [[[-1]]]
    assert int(np.asarray(s.calls["memory_encoder"][1]["binarize"]).reshape(-1)[0]) == 0


def test_lost_when_object_score_not_positive():
    tr = Sam2VideoTracker(RecordingSessions(object_score=0.0), CONSTANTS, total_frames=1)
    assert tr.process(rgb(), [NormalizedPoint(0.5, 0.5, "include")]).object_present is False


def test_result_shape_and_present():
    tr = Sam2VideoTracker(RecordingSessions(object_score=1.0), CONSTANTS, total_frames=1)
    r = tr.process(rgb(), [NormalizedPoint(0.5, 0.5, "include")])
    assert r.object_present is True
    assert r.high_res_logits.shape == (512, 512) and r.high_res_logits.dtype == np.float32


def test_history_is_bounded():
    tr = Sam2VideoTracker(RecordingSessions(object_score=1.0), CONSTANTS, total_frames=40)
    tr.process(rgb(), [NormalizedPoint(0.5, 0.5, "include")])
    for _ in range(39):
        tr.process(rgb(), None)
    assert len(tr._recent) == 6 and len(tr._recent_ptrs) == 15


def test_points_on_later_frame_rejected():
    import pytest

    tr = Sam2VideoTracker(RecordingSessions(object_score=1.0), CONSTANTS, total_frames=2)
    tr.process(rgb(), [NormalizedPoint(0.5, 0.5, "include")])
    with pytest.raises(ValueError):
        tr.process(rgb(), [NormalizedPoint(0.5, 0.5, "include")])


def test_image_size_1024_from_constants():
    """small / base+ / large（constants.json の image_size=1024）も同じコードで動く。"""
    s = RecordingSessions(object_score=1.0, size=1024)
    tr = Sam2VideoTracker(s, {**CONSTANTS, "image_size": 1024}, total_frames=3)
    r0 = tr.process(rgb(), [NormalizedPoint(0.5, 0.25, "include")])
    tr.process(rgb(), None)

    assert s.calls["vision_encoder"][0]["pixel_values"].shape == (1, 3, 1024, 1024)
    assert s.calls["mask_decoder"][0]["input_points"].tolist() == [[[[512.0, 256.0]]]]
    att = s.calls["memory_attention"][0]
    assert att["current_vision_features"].shape == (4096, 1, 256)
    assert att["memory"].shape == (7 * 4096 + 64, 1, 64)
    assert r0.high_res_logits.shape == (1024, 1024)


def test_image_size_defaults_to_512():
    s = RecordingSessions(object_score=1.0)
    Sam2VideoTracker(s, CONSTANTS, total_frames=1).process(rgb(), [NormalizedPoint(0.5, 0.5, "include")])
    assert s.calls["vision_encoder"][0]["pixel_values"].shape == (1, 3, 512, 512)
