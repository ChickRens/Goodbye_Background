import numpy as np
import pytest

from cutout.video_io import FrameReader, Mp4Writer, UnsupportedVideo, VideoInfo, output_geometry, probe
from tests.media import make_video


def info_of(width, height, fps, duration_ms=1000, has_audio=False, is_hdr=False):
    return VideoInfo(duration_ms, fps, width, height, has_audio, is_hdr)


def write_through(src, tmp_path):
    info = probe(src)
    w, h, fps = output_geometry(info)
    out = tmp_path / "out.mp4"
    wr = Mp4Writer(out, w, h, fps, src if info.has_audio else None, 0, info.duration_ms)
    for f in FrameReader(src, 0, info.duration_ms, w, h, fps, info.is_hdr):
        wr.write(f)
    wr.close()
    return out


def test_60fps_is_capped_to_30_and_range_is_respected(tmp_path):
    src = make_video(tmp_path, fps=60, seconds=3, size=(960, 540), audio=True)
    info = probe(src)
    w, h, fps = output_geometry(info)
    assert (w, h, fps) == (960, 540, 30)
    frames = list(FrameReader(src, 500, 2500, w, h, fps, info.is_hdr))
    assert len(frames) == 60 and frames[0].shape == (540, 960, 3)


def test_writer_keeps_audio_and_length(tmp_path):
    src = make_video(tmp_path, fps=30, seconds=3, size=(640, 360), audio=True)
    out = tmp_path / "out.mp4"
    wr = Mp4Writer(out, 640, 360, 30, src, 0, 2000)
    for f in FrameReader(src, 0, 2000, 640, 360, 30, False):
        wr.write(f)
    wr.close()
    o = probe(out)
    assert o.has_audio and abs(o.duration_ms - 2000) <= 100 and o.fps == 30


def test_no_audio_source_gives_video_only(tmp_path):
    src = make_video(tmp_path, fps=30, seconds=1, size=(320, 240), audio=False)
    out = write_through(src, tmp_path)
    assert probe(out).has_audio is False


def test_source_without_audio_stream_but_audio_source_given(tmp_path):
    # audio_source を渡しても音声トラックが無ければ映像のみで成功する（-map 1:a:0?）
    src = make_video(tmp_path, fps=30, seconds=1, size=(320, 240), audio=False)
    out = tmp_path / "out.mp4"
    wr = Mp4Writer(out, 320, 240, 30, src, 0, 1000)
    for f in FrameReader(src, 0, 1000, 320, 240, 30, False):
        wr.write(f)
    wr.close()
    assert probe(out).has_audio is False


def test_odd_and_large_sizes_become_even_and_capped(tmp_path):
    assert output_geometry(info_of(width=721, height=405, fps=30))[:2] == (720, 404)
    assert output_geometry(info_of(width=3840, height=2160, fps=30))[:2] == (1080, 606)
    assert output_geometry(info_of(width=2160, height=3840, fps=30))[:2] == (606, 1080)
    write_through(make_video(tmp_path, fps=30, seconds=1, size=(721, 405), audio=False), tmp_path)


def test_rotation_metadata_gives_portrait(tmp_path):
    src = make_video(tmp_path, fps=30, seconds=1, size=(640, 360), audio=False, rotate=90)
    assert output_geometry(probe(src))[:2] == (360, 640)


def test_hlg_10bit_is_detected_and_tonemapped(tmp_path):
    src = make_video(tmp_path, fps=30, seconds=1, size=(640, 360), audio=False, hlg=True)
    info = probe(src)
    assert info.is_hdr
    f = next(iter(FrameReader(src, 0, 1000, 640, 360, 30, True)))
    assert f.dtype == np.uint8 and 20 < f.mean() < 235


def test_reader_stops_after_301_frames_and_can_close_early(tmp_path):
    src = make_video(tmp_path, fps=30, seconds=12, size=(160, 120), audio=False)
    assert len(list(FrameReader(src, 0, 12000, 160, 120, 30, False))) == 301
    r = FrameReader(src, 0, 12000, 160, 120, 30, False)
    next(iter(r))
    r.close()  # 例外なく止まる


def test_unreadable_file_raises_unsupported(tmp_path):
    bad = tmp_path / "bad.mp4"
    bad.write_bytes(b"not a video")
    with pytest.raises(UnsupportedVideo):
        probe(bad)
