"""scripts/diagnose_logits.py のテスト（実モデルは使わない。トラッカーを偽物に差し替える）。"""

from __future__ import annotations

import csv
import importlib.util
import subprocess
from pathlib import Path

import numpy as np
import pytest

from cutout.tracker import FrameResult
from tests.media import make_video

SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "diagnose_logits.py"


def _load_script():
    spec = importlib.util.spec_from_file_location("diagnose_logits", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_parse_point():
    m = _load_script()
    p = m.parse_point("0.5,0.42")
    assert (p.x, p.y, p.label) == (0.5, 0.42, "include")
    assert m.parse_point("0.1,0.2,exclude").label == "exclude"
    with pytest.raises(Exception):
        m.parse_point("1.5,0.2")


def test_area_stats_counts_each_threshold():
    m = _load_script()
    logits = np.full((512, 512), -10.0, np.float32)
    logits[0, :4] = [1.0, -0.5, -1.5, -3.0]
    # 閾値 0 / -1 / -2 / -4 を超える画素数
    assert m.area_stats(logits) == [1, 2, 3, 4]


def test_shifted_alpha_keeps_slightly_negative_logits():
    """ロジット -1（本番の合成では消える）が、閾値 -2 にずらすと残る。"""
    m = _load_script()
    logits = np.full((512, 512), -1.0, np.float32)
    assert m.alpha_from_logits(logits, 8, 8).max() < 0.01
    assert m.shifted_alpha(logits, 8, 8, -2.0).min() > 0.99


def test_main_writes_three_panel_video_and_csv(tmp_path, monkeypatch):
    m = _load_script()
    src = make_video(tmp_path, fps=10, seconds=1, size=(64, 96), audio=False)

    class FakeTracker:
        def __init__(self, sessions, constants, total):
            self.t = 0

        def process(self, frame, points):
            assert (points is not None) == (self.t == 0)
            self.t += 1
            logits = np.full((512, 512), -1.0, np.float32)
            logits[200:300, 200:300] = 5.0
            return FrameResult(high_res_logits=logits, object_present=True)

    monkeypatch.setattr(m, "fetch_models", lambda *a, **k: None)
    monkeypatch.setattr(m, "load_sessions", lambda *a, **k: None)
    monkeypatch.setattr(m, "load_constants", lambda *a, **k: {})
    monkeypatch.setattr(m, "Sam2VideoTracker", FakeTracker)

    out = tmp_path / "out"
    assert m.main([str(src), "--point", "0.5,0.5", "--out-dir", str(out)]) == 0

    size = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "stream=width,height", "-of", "csv=p=0", str(out / "diagnose-tiny.mp4")],
        capture_output=True, text=True, check=True,
    ).stdout.strip()
    assert size == "192,96"  # 3 枚横並び
    rows = list(csv.reader((out / "diagnose-tiny.csv").open()))
    assert rows[0] == ["frame", "present", "area_gt_0", "area_gt_-1", "area_gt_-2", "area_gt_-4"]
    assert len(rows) == 1 + 10
    assert rows[1][2:] == ["10000", "10000", str(512 * 512), str(512 * 512)]


def test_preflight_names_missing_ffprobe(tmp_path, monkeypatch):
    m = _load_script()
    video = tmp_path / "v.mp4"
    video.write_bytes(b"x")
    monkeypatch.setattr(m.shutil, "which", lambda exe: None if exe == "ffprobe" else "/bin/" + exe)
    msg = m.preflight(video)
    assert "ffprobe が見つかりません" in msg
    assert "winget install Gyan.FFmpeg" in msg


def test_preflight_names_missing_video(tmp_path):
    m = _load_script()
    msg = m.preflight(tmp_path / "無い 動画.mp4")
    assert "動画が見つかりません" in msg and "無い 動画.mp4" in msg
    video = tmp_path / "v.mp4"
    video.write_bytes(b"x")
    assert m.preflight(video) is None


def test_main_stops_before_fetching_models_when_preflight_fails(tmp_path, monkeypatch):
    m = _load_script()
    monkeypatch.setattr(m, "fetch_models", lambda *a, **k: pytest.fail("モデルを取得してはいけない"))
    assert m.main([str(tmp_path / "none.mp4"), "--point", "0.5,0.5"]) == 1
