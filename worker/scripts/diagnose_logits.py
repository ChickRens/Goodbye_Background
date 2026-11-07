"""診断用: 速く動く手や腕が消える原因を、本番と同じトラッカーをオーナーの PC（CPU）で動かして調べる。

問い: 消えた部分は「SAM2 が見失った（ロジットが大きく負）」のか、
      「ロジットは 0 の少し下で、EDGE_SHARPNESS=16 の硬い合成で切り捨てられた」のか。
前者なら合成をいじっても直らない（案 B / 案 C が必要）。後者なら合成の閾値で取り戻せる。

出力（取得先のフォルダに作る）:
  diagnose-<model>.mp4  左から [本番と同じ合成] [ロジットの色分け + 0 と -2 の等高線] [閾値を -2 にずらした合成]
  diagnose-<model>.csv  フレームごとの object_score と、ロジットが 0 / -1 / -2 / -4 を超える面積（モデルの解像度での画素数）

使い方（worker フォルダで。モデルは全グラフが要る。初回は約 150MB を取得する）:
  uv run python scripts/diagnose_logits.py <元の動画> --point 0.50,0.42 [--point x,y,exclude] [--start-ms 0]
  点は 0..1 の正規化座標（左上が 0,0）。プレビューでタップしたのと同じ所を指定する。
"""

from __future__ import annotations

import argparse
import csv
import shutil
import sys
import time
from pathlib import Path

import cv2
import numpy as np

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "worker"))  # scripts/ から実行しても cutout を import できるように

from cutout.compositor import EDGE_SHARPNESS, alpha_from_logits, composite  # noqa: E402
from cutout.contract import NormalizedPoint  # noqa: E402
from cutout.models import VIDEO_MODELS, fetch_models, load_constants, load_sessions  # noqa: E402
from cutout.tracker import Sam2VideoTracker  # noqa: E402
from cutout.video_io import MAX_FRAMES, FrameReader, Mp4Writer, output_geometry, probe  # noqa: E402

# worker/models/ は git 管理外かつ Docker イメージに入らない（ルートの models/ に置くとイメージが太る）
MODELS_ROOT = ROOT / "worker" / "models"
GREEN = (0, 255, 0)
THRESHOLDS = (0.0, -1.0, -2.0, -4.0)
SHIFTED = -2.0  # 3 枚目のパネルで使う閾値


def parse_point(s: str) -> NormalizedPoint:
    parts = s.split(",")
    if len(parts) not in (2, 3):
        raise argparse.ArgumentTypeError("点は x,y または x,y,exclude で指定する")
    x, y = float(parts[0]), float(parts[1])
    if not (0 <= x <= 1 and 0 <= y <= 1):
        raise argparse.ArgumentTypeError("x,y は 0..1")
    label = "exclude" if len(parts) == 3 and parts[2] == "exclude" else "include"
    return NormalizedPoint(x, y, label)


def area_stats(logits_512: np.ndarray) -> list[int]:
    """ロジットが各閾値を超える画素数（モデルの解像度のまま数える。tiny は 512x512、small 以上は 1024x1024）。"""
    return [int((logits_512 > t).sum()) for t in THRESHOLDS]


def shifted_alpha(logits_512: np.ndarray, width: int, height: int, threshold: float) -> np.ndarray:
    """閾値を threshold にずらした硬い alpha（ロジットに -threshold を足してから本番と同じ式）。"""
    return alpha_from_logits(logits_512 - threshold, width, height)


def heatmap(frame_rgb: np.ndarray, logits_512: np.ndarray) -> np.ndarray:
    """ロジットを [-8, 8] で色分けして元のフレームと半々に重ね、0（白）と -2（黄）の等高線を引く。"""
    h, w = frame_rgb.shape[:2]
    up = cv2.resize(logits_512.astype(np.float32), (w, h), interpolation=cv2.INTER_LINEAR)
    norm = ((np.clip(up, -8.0, 8.0) + 8.0) / 16.0 * 255).astype(np.uint8)
    color = cv2.cvtColor(cv2.applyColorMap(norm, cv2.COLORMAP_JET), cv2.COLOR_BGR2RGB)
    out = (frame_rgb.astype(np.float32) * 0.5 + color.astype(np.float32) * 0.5).astype(np.uint8)
    for t, c in ((0.0, (255, 255, 255)), (SHIFTED, (255, 255, 0))):
        contours, _ = cv2.findContours((up > t).astype(np.uint8), cv2.RETR_LIST, cv2.CHAIN_APPROX_NONE)
        cv2.drawContours(out, contours, -1, c, 1)
    return out


def preflight(video: Path) -> str | None:
    """重い処理の前に、動画と ffmpeg / ffprobe があるか確かめる。問題があれば日本語のメッセージを返す。

    Windows で ffprobe が無いと、Python は「[WinError 2] 指定されたファイルが見つかりません」と出し、
    動画が見つからないように読めてしまうので、先に名指しで確かめる。
    """
    missing = [exe for exe in ("ffmpeg", "ffprobe") if shutil.which(exe) is None]
    if missing:
        return (
            f"{' と '.join(missing)} が見つかりません（PATH に無い）。動画の読み書きに使います。\n"
            "  Windows: winget install Gyan.FFmpeg を実行し、PowerShell を開き直してから、もう一度実行してください。\n"
            "  確認: ffprobe -version が動けば OK"
        )
    if not video.is_file():
        return f"動画が見つかりません: {video}\n  パスに空白や日本語があるときは \"\" で囲んでください"
    return None


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("video", type=Path)
    ap.add_argument("--point", type=parse_point, action="append", required=True)
    ap.add_argument("--start-ms", type=int, default=0)
    ap.add_argument("--model", choices=list(VIDEO_MODELS), default="tiny", help="比べるモデル（既定は本番と同じ tiny）")
    ap.add_argument("--model-dir", type=Path, default=None, help="省略時は worker/models/sam2.1-<model>-video-full")
    ap.add_argument("--frames", type=int, default=None, help="先頭から何フレームだけ処理するか（速さを測るとき）")
    ap.add_argument("--out-dir", type=Path, default=ROOT / "results" / "diagnose")
    args = ap.parse_args(argv)
    problem = preflight(args.video)
    if problem:
        print(problem, file=sys.stderr)
        return 1

    repo, revision = VIDEO_MODELS[args.model]
    model_dir = args.model_dir or MODELS_ROOT / f"sam2.1-{args.model}-video-full"
    fetch_models(model_dir, revision, repo=repo)  # 全グラフ。取得済みは飛ばす
    sessions = load_sessions(model_dir, "cpu")
    constants = load_constants(model_dir)

    info = probe(args.video)
    width, height, fps = output_geometry(info)
    end_ms = min(args.start_ms + 10_000, info.duration_ms)
    total = max(1, min(MAX_FRAMES, round((end_ms - args.start_ms) / 1000 * fps)))
    if args.frames is not None:
        total = max(1, min(total, args.frames))
    tracker = Sam2VideoTracker(sessions, constants, total)

    args.out_dir.mkdir(parents=True, exist_ok=True)
    mp4 = args.out_dir / f"diagnose-{args.model}.mp4"
    csv_path = args.out_dir / f"diagnose-{args.model}.csv"
    print(
        f"{args.model}（{constants.get('image_size')}）{width}x{height} {fps}fps {total} フレーム"
        f" EDGE_SHARPNESS={EDGE_SHARPNESS} 点={args.point}"
    )
    started = time.monotonic()
    with FrameReader(args.video, args.start_ms, end_ms, width, height, fps, info.is_hdr) as reader, Mp4Writer(
        mp4, width * 3, height, fps, None, 0, 0
    ) as writer, csv_path.open("w", newline="") as f:
        out = csv.writer(f)
        out.writerow(["frame", "present"] + [f"area_gt_{t:g}" for t in THRESHOLDS])
        for i, frame in enumerate(reader):
            if i >= total:
                break
            r = tracker.process(frame, args.point if i == 0 else None)
            logits = r.high_res_logits
            if r.object_present:
                now = composite(frame, alpha_from_logits(logits, width, height), GREEN)
                shifted = composite(frame, shifted_alpha(logits, width, height, SHIFTED), GREEN)
            else:
                now = shifted = composite(frame, None, GREEN)
            writer.write(np.concatenate([now, heatmap(frame, logits), shifted], axis=1))
            out.writerow([i, int(r.object_present)] + area_stats(logits))
            if i % 30 == 0:
                print(f"  {i}/{total} フレーム  {time.monotonic() - started:.0f} 秒")
        writer.close()
    elapsed = time.monotonic() - started
    print(f"できました（{elapsed:.0f} 秒、1 フレームあたり {elapsed / max(1, i + 1):.2f} 秒）:\n  {mp4}\n  {csv_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
