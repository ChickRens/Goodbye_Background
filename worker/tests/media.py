"""テスト用の動画を ffmpeg の lavfi（testsrc / sine）で作るヘルパー。"""

import subprocess
from pathlib import Path


def _run(cmd: list[str]) -> None:
    r = subprocess.run(cmd, capture_output=True)
    if r.returncode != 0:
        raise RuntimeError(r.stderr.decode(errors="replace")[-2000:])


def make_video(
    tmp_path: Path,
    fps: int,
    seconds: float,
    size: tuple[int, int],
    audio: bool,
    rotate: int = 0,
    hlg: bool = False,
) -> Path:
    """テスト動画を作って返す。size は (幅, 高さ)。

    rotate は表示回転のメタデータ（display matrix）だけを付ける（画素は回さない）。
    hlg は 10bit HEVC + HLG（arib-std-b67）で符号化する。
    """
    w, h = size
    name = f"src_{fps}_{w}x{h}_{int(audio)}_{rotate}_{int(hlg)}.mp4"
    out = tmp_path / name
    base = out if rotate == 0 else tmp_path / ("pre_" + name)

    cmd = ["ffmpeg", "-y", "-v", "error", "-f", "lavfi", "-i", f"testsrc=size={w}x{h}:rate={fps}:duration={seconds}"]
    if audio:
        cmd += ["-f", "lavfi", "-i", f"sine=frequency=440:duration={seconds}"]
    if hlg:
        cmd += [
            "-c:v", "libx265", "-pix_fmt", "yuv420p10le", "-x265-params", "log-level=error",
            "-color_primaries", "bt2020", "-color_trc", "arib-std-b67", "-colorspace", "bt2020nc",
        ]
    else:
        # 奇数サイズは 4:2:0 にできないので 4:4:4 で作る（読み込み側の偶数化テスト用）
        pix_fmt = "yuv420p" if w % 2 == 0 and h % 2 == 0 else "yuv444p"
        cmd += ["-c:v", "libx264", "-pix_fmt", pix_fmt]
    if audio:
        cmd += ["-c:a", "aac", "-shortest"]
    cmd.append(str(base))
    _run(cmd)

    if rotate != 0:
        # 入力オプション -display_rotation はストリームコピーの再多重化で display matrix を書く
        _run(["ffmpeg", "-y", "-v", "error", "-display_rotation", str(rotate), "-i", str(base), "-c", "copy", str(out)])
    return out
