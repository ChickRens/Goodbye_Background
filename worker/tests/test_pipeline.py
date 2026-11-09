"""run_job のテスト。ONNX は使わず FakeTracker で推論の契約だけを再現する。"""

import shutil
import subprocess
import warnings
from pathlib import Path

import numpy as np
import pytest

from cutout.compositor import LogoBadge, alpha_from_logits
from cutout.contract import NormalizedPoint, RangeMs, WorkerJobRequest
from cutout.errors import InputTooLarge
from cutout.pipeline import run_job
from cutout.tracker import FrameResult
from cutout.video_io import probe
from tests.media import make_video

JOB = "job-1"
LOGO = Path(__file__).parent.parent / "cutout" / "assets" / "logo.png"


class FakeNotifier:
    def __init__(self):
        self.events: list[dict] = []
        self.finals: list[dict] = []

    def progress(self, event):
        self.events.append(event)

    def final(self, event):
        self.events.append(event)
        self.finals.append(event)
        return True


class FakeTransfer:
    def __init__(self, src: Path | None, head_size: int | None, out_dir: Path, download_raises_too_large=False):
        self._src, self._head, self._out_dir = src, head_size, out_dir
        self._download_raises_too_large = download_raises_too_large
        self.max_bytes: int | None = None
        self.uploaded: Path | None = None
        self.content_type: str | None = None
        self.downloaded = False

    def head_size(self, url):
        return self._head

    def download(self, url, dest, max_bytes):
        self.downloaded = True
        self.max_bytes = max_bytes
        if self._download_raises_too_large:
            raise InputTooLarge()
        shutil.copy(self._src, dest)

    def upload(self, url, src, content_type):
        self.uploaded = self._out_dir / "uploaded.mp4"
        shutil.copy(src, self.uploaded)
        self.content_type = content_type


class FakeTracker:
    def __init__(self, present_pattern, raises):
        self._pattern, self._raises, self._i = present_pattern, raises, 0

    def process(self, frame_rgb, points):
        if self._raises:
            raise RuntimeError("boom")
        present = self._pattern[self._i] if self._i < len(self._pattern) else True
        # 1 枚目だけ points が渡される契約
        assert (points is not None) == (self._i == 0)
        self._i += 1
        return FrameResult(np.full((512, 512), 10.0, np.float32), present)


def request(range_ms, watermark=False, max_bytes=200 * 1024 * 1024) -> WorkerJobRequest:
    return WorkerJobRequest(
        job_id=JOB, input_url="http://in", output_url="http://out", range_ms=RangeMs(*range_ms),
        points=[NormalizedPoint(0.5, 0.5, "include")], background_rgb=(0, 255, 0),
        watermark=watermark, max_input_bytes=max_bytes, callback_url="http://cb",
    )


def run(tmp_path, video=None, range_ms=(0, 1000), seconds=2, head_size=1000, first_present=True,
        present_pattern=None, tracker_raises=False, watermark=False):
    if video is None:
        video = make_video(tmp_path, fps=30, seconds=seconds, size=(320, 240), audio=True)
    pattern = present_pattern if present_pattern is not None else [first_present]
    totals: list[int] = []

    def make_tracker(total):
        totals.append(total)
        return FakeTracker(pattern, tracker_raises)

    transfer = FakeTransfer(video, head_size, tmp_path)
    notifier = FakeNotifier()
    run_job(request(range_ms, watermark), transfer, notifier, make_tracker, LogoBadge.load(LOGO), tmp_path / "work")
    assert len(notifier.finals) == 1
    transfer.totals = totals
    return notifier.events, transfer


def final(result):
    ev = result[0] if isinstance(result, tuple) else result
    return ev[-1]["phase"], ev[-1].get("code")


def bytes_file(tmp_path, data: bytes) -> Path:
    p = tmp_path / "bad.mp4"
    p.write_bytes(data)
    return p


def test_happy_path_emits_phases_and_uploads(tmp_path):
    ev, tr = run(tmp_path, range_ms=(0, 1000))
    phases = [e["phase"] for e in ev]
    assert phases[0] == "downloading"
    assert ev[-1] == {"job_id": JOB, "phase": "done", "frames": 30, "lost_frames": 0, "duration_ms": 1000}
    assert "finishing" in phases and phases.index("finishing") > phases.index("tracking")
    assert probe(tr.uploaded).has_audio
    assert tr.content_type == "video/mp4"
    assert tr.totals == [30]
    assert not (tmp_path / "work").exists()


def test_watermark_changes_output_frames(tmp_path):
    _, plain = run(tmp_path, range_ms=(0, 100))
    plain_bytes = plain.uploaded.read_bytes()
    _, marked = run(tmp_path, range_ms=(0, 100), watermark=True)
    assert marked.uploaded.read_bytes() != plain_bytes


def test_too_large(tmp_path):
    ev, tr = run(tmp_path, head_size=300 * 1024 * 1024)
    assert final(ev) == ("failed", "too_large")
    assert not tr.downloaded


def test_too_large_checked_after_download_when_head_unknown(tmp_path):
    video = make_video(tmp_path, fps=30, seconds=1, size=(320, 240), audio=False)
    transfer = FakeTransfer(video, None, tmp_path)
    notifier = FakeNotifier()
    req = request((0, 500), max_bytes=100)
    run_job(req, transfer, notifier, lambda t: FakeTracker([True], False), LogoBadge.load(LOGO), tmp_path / "work")
    assert final(notifier.events) == ("failed", "too_large")
    assert transfer.uploaded is None


def test_download_passes_max_bytes(tmp_path):
    _, tr = run(tmp_path, range_ms=(0, 100))
    assert tr.max_bytes == 200 * 1024 * 1024


def test_too_large_when_download_stops_midway(tmp_path):
    video = make_video(tmp_path, fps=30, seconds=1, size=(320, 240), audio=False)
    transfer = FakeTransfer(video, None, tmp_path, download_raises_too_large=True)
    notifier = FakeNotifier()
    run_job(request((0, 500)), transfer, notifier, lambda t: FakeTracker([True], False), LogoBadge.load(LOGO), tmp_path / "work")
    assert final(notifier.events) == ("failed", "too_large")
    assert transfer.uploaded is None


def test_unsupported(tmp_path):
    assert final(run(tmp_path, video=bytes_file(tmp_path, b"xx"))) == ("failed", "unsupported_video")


def test_start_beyond_duration(tmp_path):
    assert final(run(tmp_path, range_ms=(5000, 6000), seconds=2)) == ("failed", "range_out_of_bounds")


def test_end_slightly_beyond_duration_is_clamped(tmp_path):  # Review Focus 1
    ev, _ = run(tmp_path, range_ms=(1000, 2040), seconds=2)
    assert ev[-1]["phase"] == "done" and abs(ev[-1]["duration_ms"] - 1000) <= 40


def test_subject_not_found_on_first_frame(tmp_path):
    ev, tr = run(tmp_path, first_present=False)
    assert final(ev) == ("failed", "subject_not_found")
    assert tr.uploaded is None


def test_unexpected_exception_is_internal_error_and_workdir_removed(tmp_path):
    ev, _ = run(tmp_path, tracker_raises=True)
    assert final((ev, None)) == ("failed", "internal_error") and not (tmp_path / "work").exists()


def test_lost_frames_are_counted(tmp_path):
    ev, _ = run(tmp_path, present_pattern=[True, False, True], range_ms=(0, 100), seconds=1)
    assert ev[-1]["frames"] == 3 and ev[-1]["lost_frames"] == 1


def test_very_negative_logits_give_zero_alpha_without_warning():
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        alpha = alpha_from_logits(np.full((512, 512), -1024.0, np.float32), 64, 48)
    assert float(alpha.max()) < 1e-6


class BlobTracker:
    """体（中央）は毎フレーム、左上の離れた塊は 2 枚目以降だけ出すトラッカー。"""

    def __init__(self):
        self._i = 0

    def process(self, frame_rgb, points):
        a = np.full((512, 512), -10.0, np.float32)
        a[100:400, 200:300] = 10.0  # 体
        if self._i > 0:
            a[10:60, 10:60] = 10.0  # 体から遠くに急に出た塊（隣の人の切れ端）
        self._i += 1
        return FrameResult(a, True)


def test_detached_blob_is_removed_after_first_frame(tmp_path, monkeypatch):
    import cutout.pipeline as pipeline

    monkeypatch.setattr(pipeline, "REMOVE_DETACHED_BLOBS", True)
    video = make_video(tmp_path, fps=30, seconds=1, size=(512, 512), audio=False)
    transfer = FakeTransfer(video, 1000, tmp_path)
    run_job(request((0, 1000)), transfer, FakeNotifier(), lambda total: BlobTracker(), LogoBadge.load(LOGO), tmp_path / "w")
    raw = subprocess.run(
        ["ffmpeg", "-v", "error", "-i", str(transfer.uploaded), "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
        capture_output=True, check=True,
    ).stdout
    frames = np.frombuffer(raw, np.uint8).reshape(-1, 512, 512, 3).astype(int)
    def green(region):
        r, g, b = region.reshape(-1, 3).mean(axis=0)
        return g > 200 and r < 60 and b < 60
    assert green(frames[5, 15:55, 15:55])  # 離れた塊は消えて背景（緑）
    assert not green(frames[5, 150:350, 220:280])  # 体は残る
