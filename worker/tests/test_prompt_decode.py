"""cutout/prompt_decode.py: 点を打ったフレームで前回のマスクを渡して 1 点ずつ積み上げる（仕様 2026-11-16 §2.2）。"""

import numpy as np

from cutout.contract import NormalizedPoint
from cutout.prompt_decode import MASK_INPUT_CLIP, decode_points, decode_tracking

F0 = np.zeros((1, 32, 256, 256), np.float32)
F1 = np.zeros((1, 64, 128, 128), np.float32)
F2 = np.zeros((1, 256, 64, 64), np.float32)
PTS = [
    NormalizedPoint(0.5, 0.25, "include"),
    NormalizedPoint(0.25, 0.75, "include"),
    NormalizedPoint(1.0, 0.0, "exclude"),
]


class RecordingDecoder:
    """受けた入力を記録し、k 回目（1 始まり）の呼び出しで low_res_mask = 100*k を返す偽のデコーダ。"""

    def __init__(self):
        self.calls: list[dict] = []

    def run(self, **feeds):
        self.calls.append(feeds)
        k = len(self.calls)
        return {
            "low_res_mask": np.full((1, 1, 256, 256), 100.0 * k, np.float32),
            "high_res_mask": np.full((1, 1, 1024, 1024), float(k), np.float32),
            "object_score_logits": np.array([[[float(k)]]], np.float32),
        }


def run_points(points):
    dec = RecordingDecoder()
    out = decode_points(dec, F0, F1, F2, points, 1024)
    return dec, out


def test_one_point_calls_decoder_once_without_mask():
    dec, _ = run_points(PTS[:1])
    assert len(dec.calls) == 1
    assert np.asarray(dec.calls[0]["has_mask"]).tolist() == [0.0]
    assert not np.asarray(dec.calls[0]["mask_input"]).any()
    assert np.asarray(dec.calls[0]["mask_input"]).shape == (1, 1, 256, 256)


def test_three_points_chain_prefixes_in_order():
    dec, _ = run_points(PTS)
    assert len(dec.calls) == 3
    assert dec.calls[0]["input_points"].tolist() == [[[[512.0, 256.0]]]]
    assert dec.calls[1]["input_points"].tolist() == [[[[512.0, 256.0], [256.0, 768.0]]]]
    assert dec.calls[2]["input_points"].tolist() == [[[[512.0, 256.0], [256.0, 768.0], [1024.0, 0.0]]]]
    assert [c["input_labels"].tolist() for c in dec.calls] == [[[[1]]], [[[1, 1]]], [[[1, 1, 0]]]]


def test_chain_passes_previous_low_res_clipped():
    dec, _ = run_points(PTS)
    for call in dec.calls[1:]:
        assert np.asarray(call["has_mask"]).tolist() == [1.0]
        assert np.all(np.asarray(call["mask_input"]) == MASK_INPUT_CLIP)
    assert MASK_INPUT_CLIP == 32.0


def test_negative_previous_mask_is_clipped_to_minus_32():
    class NoObjectFirst(RecordingDecoder):
        def run(self, **feeds):
            out = super().run(**feeds)
            if len(self.calls) == 1:
                out["low_res_mask"] = np.full((1, 1, 256, 256), -1024.0, np.float32)
            return out

    dec = NoObjectFirst()
    decode_points(dec, F0, F1, F2, PTS[:2], 1024)
    assert np.all(np.asarray(dec.calls[1]["mask_input"]) == -32.0)


def test_returns_last_decoder_output():
    _, out = run_points(PTS)
    assert np.all(out["low_res_mask"] == 300.0)
    assert float(np.ravel(out["object_score_logits"])[0]) == 3.0


def test_exclude_first_point_still_chains():
    dec, _ = run_points([PTS[2], PTS[0]])
    assert len(dec.calls) == 2
    assert dec.calls[0]["input_labels"].tolist() == [[[0]]]


def test_tracking_uses_pad_point_without_mask():
    dec = RecordingDecoder()
    decode_tracking(dec, F0, F1, F2)
    assert len(dec.calls) == 1
    assert dec.calls[0]["input_labels"].tolist() == [[[-1]]]
    assert dec.calls[0]["input_points"].tolist() == [[[[0.0, 0.0]]]]
    assert np.asarray(dec.calls[0]["has_mask"]).tolist() == [0.0]
    assert not np.asarray(dec.calls[0]["mask_input"]).any()
