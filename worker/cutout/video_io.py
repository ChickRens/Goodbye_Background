"""ffmpeg / ffprobe をサブプロセスで呼ぶ動画の読み書き。

フレームは RGB uint8 の生データ（rawvideo / rgb24）でパイプ越しにやり取りする。
"""

import json
import logging
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path

import numpy as np

logger = logging.getLogger(__name__)

MAX_LONG_SIDE = 1080
MAX_FPS = 30
MAX_FRAMES = 301  # 10 秒 @30fps + 1 枚（トラッカーの上限に合わせる）
_HDR_TRANSFERS = {"arib-std-b67", "smpte2084"}  # HLG / PQ
_HDR_TONEMAP = "zscale=t=linear,tonemap=hable,zscale=t=bt709:m=bt709:p=bt709"
_STDERR_TAIL = 2000


class UnsupportedVideo(Exception):
    """映像ストリームが無い、または読めない動画。"""


class FfmpegError(RuntimeError):
    """ffmpeg が異常終了した。メッセージに stderr の末尾を含める。"""


@dataclass(frozen=True)
class VideoInfo:
    """width / height は回転メタデータ適用後の「表示サイズ」。"""

    duration_ms: int
    fps: float
    width: int
    height: int
    has_audio: bool
    is_hdr: bool


def _parse_rate(s: str | None) -> float:
    if not s or "/" not in s:
        return 0.0
    num, den = s.split("/", 1)
    try:
        return float(num) / float(den) if float(den) else 0.0
    except ValueError:
        return 0.0


def _rotation(stream: dict) -> int:
    for sd in stream.get("side_data_list", []):
        if "rotation" in sd:
            return int(round(float(sd["rotation"])))
    rot = stream.get("tags", {}).get("rotate")
    return int(rot) if rot is not None else 0


def probe(path: Path) -> VideoInfo:
    """ffprobe で動画情報を取る。映像が無い・読めないなら UnsupportedVideo。"""
    r = subprocess.run(
        ["ffprobe", "-v", "error", "-print_format", "json", "-show_streams", "-show_format", str(path)],
        capture_output=True,
    )
    if r.returncode != 0:
        raise UnsupportedVideo(r.stderr.decode(errors="replace")[-_STDERR_TAIL:])
    try:
        data = json.loads(r.stdout)
    except json.JSONDecodeError as e:
        raise UnsupportedVideo(f"ffprobe の出力を解釈できない: {e}") from e

    streams = data.get("streams", [])
    video = next((s for s in streams if s.get("codec_type") == "video"), None)
    if video is None or not video.get("width") or not video.get("height"):
        raise UnsupportedVideo("映像ストリームが無い")

    width, height = int(video["width"]), int(video["height"])
    if abs(_rotation(video)) % 180 == 90:
        width, height = height, width

    fps = _parse_rate(video.get("avg_frame_rate")) or _parse_rate(video.get("r_frame_rate"))
    if fps <= 0:
        raise UnsupportedVideo("フレームレートを取得できない")

    duration = video.get("duration") or data.get("format", {}).get("duration")
    try:
        duration_ms = int(round(float(duration) * 1000))
    except (TypeError, ValueError) as e:
        raise UnsupportedVideo("長さを取得できない") from e

    return VideoInfo(
        duration_ms=duration_ms,
        fps=fps,
        width=width,
        height=height,
        has_audio=any(s.get("codec_type") == "audio" for s in streams),
        is_hdr=video.get("color_transfer") in _HDR_TRANSFERS,
    )


def output_geometry(info: VideoInfo) -> tuple[int, int, float]:
    """出力の (幅, 高さ, fps)。長辺 1080 以下に縮小（拡大はしない）→ 各辺を偶数に切り捨て、fps は min(元, 30)。

    info.width / height は回転適用後の表示サイズなので、ここで回転は考えない。
    """
    long_side = max(info.width, info.height)
    if long_side > MAX_LONG_SIDE:
        # 整数演算で長辺をちょうど MAX_LONG_SIDE にする
        w = info.width * MAX_LONG_SIDE // long_side
        h = info.height * MAX_LONG_SIDE // long_side
    else:
        w, h = info.width, info.height
    w = max(2, w // 2 * 2)
    h = max(2, h // 2 * 2)
    return w, h, min(info.fps, MAX_FPS)


def _fail(proc: subprocess.Popen, stderr_file, what: str) -> FfmpegError:
    stderr_file.seek(0)
    tail = stderr_file.read().decode(errors="replace")[-_STDERR_TAIL:]
    logger.error("%s が失敗 (code=%s): %s", what, proc.returncode, tail)
    return FfmpegError(f"{what} が失敗 (code={proc.returncode}): {tail}")


def _terminate(proc: subprocess.Popen) -> None:
    if proc.poll() is None:
        proc.kill()
    proc.wait()


class FrameReader:
    """指定区間のフレームを RGB uint8 [H,W,3] で順に返すイテレータ。MAX_FRAMES 枚で止まる。"""

    def __init__(self, path: Path, start_ms: int, end_ms: int, width: int, height: int, fps: float, is_hdr: bool):
        self._w, self._h = width, height
        filters = [f"fps={fps}"]
        if is_hdr:
            filters.append(_HDR_TONEMAP)
        filters.append(f"scale={width}:{height}")
        cmd = [
            "ffmpeg", "-v", "error", "-nostdin",
            "-ss", f"{start_ms / 1000:.3f}", "-to", f"{end_ms / 1000:.3f}",
            "-i", str(path),
            "-an", "-vf", ",".join(filters),
            "-frames:v", str(MAX_FRAMES),
            "-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1",
        ]
        self._stderr = tempfile.TemporaryFile()
        self._proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=self._stderr)
        self._count = 0
        self._closed = False

    def __iter__(self):
        return self

    def __next__(self) -> np.ndarray:
        if self._closed or self._count >= MAX_FRAMES:
            self.close()
            raise StopIteration
        size = self._w * self._h * 3
        buf = self._proc.stdout.read(size)
        if len(buf) < size:
            # EOF。ffmpeg の終了コードが異常なら stderr 付きで例外にする
            self._proc.wait()
            err = _fail(self._proc, self._stderr, "ffmpeg（読み込み）") if self._proc.returncode else None
            self.close()
            if err:
                raise err
            raise StopIteration
        self._count += 1
        return np.frombuffer(buf, np.uint8).reshape(self._h, self._w, 3).copy()

    def close(self) -> None:
        """途中で止めたときも ffmpeg を残さない。"""
        if self._closed:
            return
        self._closed = True
        if self._proc.stdout:
            self._proc.stdout.close()
        _terminate(self._proc)
        self._stderr.close()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()

    def __del__(self):
        try:
            self.close()
        except Exception:
            pass


class Mp4Writer:
    """RGB フレームを H.264 / yuv420p / faststart の mp4 に書き出す。音声は audio_source の同区間を AAC で付ける。"""

    def __init__(
        self,
        out_path: Path,
        width: int,
        height: int,
        fps: float,
        audio_source: Path | None,
        start_ms: int,
        end_ms: int,
    ):
        self._expected = width * height * 3
        cmd = [
            "ffmpeg", "-v", "error", "-nostdin", "-y",
            "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{width}x{height}", "-r", str(fps), "-i", "pipe:0",
        ]
        if audio_source is not None:
            cmd += ["-ss", f"{start_ms / 1000:.3f}", "-to", f"{end_ms / 1000:.3f}", "-i", str(audio_source)]
        cmd += ["-map", "0:v:0"]
        if audio_source is not None:
            # 音声トラックが無い入力でもエラーにしない（? 付き）
            cmd += ["-map", "1:a:0?", "-c:a", "aac", "-shortest"]
        cmd += [
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-pix_fmt", "yuv420p",
            "-movflags", "+faststart", str(out_path),
        ]
        self._stderr = tempfile.TemporaryFile()
        self._proc = subprocess.Popen(cmd, stdin=subprocess.PIPE, stderr=self._stderr)
        self._closed = False

    def write(self, frame_rgb: np.ndarray) -> None:
        data = np.ascontiguousarray(frame_rgb, dtype=np.uint8).tobytes()
        if len(data) != self._expected:
            raise ValueError(f"フレームサイズが違う: {len(data)} != {self._expected}")
        try:
            self._proc.stdin.write(data)
        except BrokenPipeError as e:
            self._proc.wait()
            raise _fail(self._proc, self._stderr, "ffmpeg（書き出し）") from e

    def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        try:
            self._proc.stdin.close()
        except BrokenPipeError:
            pass
        self._proc.wait()
        try:
            if self._proc.returncode:
                raise _fail(self._proc, self._stderr, "ffmpeg（書き出し）")
        finally:
            self._stderr.close()

    def __enter__(self):
        return self

    def __exit__(self, exc_type, *exc):
        if exc_type is not None:
            # 元の例外を隠さない。ffmpeg が既に死んでいても必ず回収する
            self._closed = True
            try:
                self._proc.stdin.close()
            except (BrokenPipeError, OSError):
                pass
            _terminate(self._proc)
            self._stderr.close()
        else:
            self.close()
