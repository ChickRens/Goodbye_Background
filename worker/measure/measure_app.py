"""Modal（L4）でのコールドスタートと処理時間の計測（Task 1 の使い捨て）。

本番と同じ部品（worker/cutout/ の video_io・tracker・models・compositor）で、
デコード → 推論 → 合成 + ロゴ → エンコード を 1 本通し、工程ごとの時間を返す。
使い方は同じフォルダの README.md を参照。

    modal run worker/measure/measure_app.py --video C:\\path\\to\\clip.mov --x 0.5 --y 0.5
"""

from __future__ import annotations

import json
import sys
import time
from pathlib import Path

import modal

# コンテナ内では「Python がこのファイルを読み込んだ時刻」= コンテナ起動のほぼ直後
_MODULE_IMPORTED_AT = time.time()

HERE = Path(__file__).resolve().parent
CUTOUT_DIR = HERE.parent / "cutout"  # worker/cutout（リポジトリのどこから実行しても解決できるようにする）

MODEL_REPO = "diffusionstudio/sam2.1-tiny-video-onnx-fp16"
MODEL_DIR = Path("/models")
MODEL_SHA_FILE = MODEL_DIR / "MODEL_SHA"

# onnxruntime-gpu は 1.27 から CUDA 13 向けになったので、CUDA 12 + cuDNN 9 向けの最後の版に固定する
ORT_GPU_VERSION = "1.26.0"
CUDA_IMAGE = "nvidia/cuda:12.8.1-cudnn-runtime-ubuntu24.04"


def download_models() -> None:
    """イメージのビルド時に 1 回だけ動く。HF の現在のコミット sha を取り、その sha 固定でモデルを取得する。"""
    import httpx

    from cutout.models import fetch_models

    r = httpx.get(f"https://huggingface.co/api/models/{MODEL_REPO}", timeout=60, follow_redirects=True)
    r.raise_for_status()
    sha = r.json()["sha"]
    fetch_models(MODEL_DIR, sha)
    MODEL_SHA_FILE.write_text(sha)
    print(f"モデルを取得しました: {MODEL_REPO}@{sha}")


image = (
    modal.Image.from_registry(CUDA_IMAGE, add_python="3.12")
    .apt_install("ffmpeg")
    .pip_install(
        f"onnxruntime-gpu=={ORT_GPU_VERSION}",
        # worker/uv.lock と同じ版
        "numpy==2.5.4",
        "opencv-python-headless==5.0.0.93",
        "httpx==0.28.1",
    )
    # ビルド中の download_models が cutout.models を使うので copy=True でイメージに焼き込む
    .add_local_dir(CUTOUT_DIR, remote_path="/root/cutout", copy=True, ignore=["__pycache__", "**/__pycache__"])
    .run_function(download_models)
)

app = modal.App("goodbye-background-measure", image=image)


def _ffmpeg_info() -> tuple[str, bool]:
    import subprocess

    version = subprocess.run(["ffmpeg", "-version"], capture_output=True, text=True).stdout.splitlines()
    filters = subprocess.run(["ffmpeg", "-hide_banner", "-filters"], capture_output=True, text=True).stdout
    has_zscale = any(len(line.split()) > 1 and line.split()[1] == "zscale" for line in filters.splitlines())
    return (version[0] if version else "?"), has_zscale


def _gpu_name() -> str:
    import subprocess

    r = subprocess.run(["nvidia-smi", "--query-gpu=name", "--format=csv,noheader"], capture_output=True, text=True)
    return r.stdout.strip() or "?"


@app.cls(gpu="L4", timeout=600, scaledown_window=60)
class Measure:
    @modal.enter()
    def load(self) -> None:
        t0 = time.time()
        import onnxruntime as ort

        from cutout.models import load_constants, load_sessions

        # device="cuda" は CUDA EP が先頭でなければ例外になる（= CPU に落ちていないことの確認）
        self.sessions = load_sessions(MODEL_DIR, "cuda")
        self.constants = load_constants(MODEL_DIR)
        self.model_load_s = time.time() - t0
        self.import_to_ready_s = time.time() - _MODULE_IMPORTED_AT
        assert "CUDAExecutionProvider" in ort.get_available_providers()
        self.ort_version = ort.__version__
        self.providers = ort.get_available_providers()
        self.model_sha = MODEL_SHA_FILE.read_text().strip()
        self.ffmpeg_version, self.zscale = _ffmpeg_info()
        self.gpu_name = _gpu_name()
        self.calls = 0

    @modal.method()
    def measure(
        self,
        video_bytes: bytes,
        start_ms: int,
        end_ms: int,
        points: list[tuple[float, float, int]],
        background: tuple[int, int, int],
    ) -> dict:
        """1 本処理して工程ごとの時間（ミリ秒のリスト）と出力 mp4 を返す。points の label は 1=前景 / 0=背景。"""
        import tempfile

        from cutout.compositor import LogoBadge, alpha_from_logits, composite
        from cutout.contract import NormalizedPoint
        from cutout.tracker import Sam2VideoTracker
        from cutout.video_io import MAX_FRAMES, FrameReader, Mp4Writer, output_geometry, probe

        cold = self.calls == 0
        self.calls += 1
        t_start = time.perf_counter()
        pts = [NormalizedPoint(x, y, "include" if label == 1 else "exclude") for x, y, label in points]
        logo = LogoBadge.load(Path("/root/cutout/assets/logo.png"))

        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / "input"
            out = Path(tmp) / "output.mp4"
            src.write_bytes(video_bytes)

            t = time.perf_counter()
            info = probe(src)
            probe_ms = (time.perf_counter() - t) * 1000
            end_ms = min(end_ms, info.duration_ms)
            width, height, fps = output_geometry(info)
            total = max(1, min(MAX_FRAMES, round((end_ms - start_ms) / 1000 * fps)))
            tracker = Sam2VideoTracker(self.sessions, self.constants, total)

            decode, track, comp, write = [], [], [], []
            frames = lost = 0
            first_present = None
            with FrameReader(src, start_ms, end_ms, width, height, fps, info.is_hdr) as reader, Mp4Writer(
                out, width, height, fps, src if info.has_audio else None, start_ms, end_ms
            ) as writer:
                while True:
                    t = time.perf_counter()
                    try:
                        frame = next(reader)
                    except StopIteration:
                        break
                    decode.append((time.perf_counter() - t) * 1000)

                    t = time.perf_counter()
                    result = tracker.process(frame, pts if frames == 0 else None)
                    track.append((time.perf_counter() - t) * 1000)
                    if frames == 0:
                        first_present = result.object_present  # 本番ならここで subject_not_found。計測は続ける

                    t = time.perf_counter()
                    if result.object_present:
                        composed = composite(frame, alpha_from_logits(result.high_res_logits, width, height), background)
                    else:
                        lost += 1
                        composed = composite(frame, None, background)
                    composed = logo.apply(composed)
                    comp.append((time.perf_counter() - t) * 1000)

                    t = time.perf_counter()
                    writer.write(composed)
                    write.append((time.perf_counter() - t) * 1000)
                    frames += 1
                t = time.perf_counter()
                writer.close()
                encode_finish_ms = (time.perf_counter() - t) * 1000
            output_mp4 = out.read_bytes()

        return {
            "cold": cold,
            "model_load_s": self.model_load_s,
            "import_to_ready_s": self.import_to_ready_s,
            "total_s": time.perf_counter() - t_start,
            "probe_ms": probe_ms,
            "probe": {
                "fps": info.fps,
                "width": info.width,
                "height": info.height,
                "duration_ms": info.duration_ms,
                "is_hdr": info.is_hdr,
                "has_audio": info.has_audio,
            },
            "output": {"width": width, "height": height, "fps": fps, "expected_frames": total},
            "frames": frames,
            "lost_frames": lost,
            "first_frame_object_present": first_present,
            "raw_ms": {"decode": decode, "track": track, "composite": comp, "encode_write": write},
            "encode_finish_ms": encode_finish_ms,
            "zscale_available": self.zscale,
            "ffmpeg_version": self.ffmpeg_version,
            "onnxruntime_version": self.ort_version,
            "available_providers": self.providers,
            "gpu_name": self.gpu_name,
            "model_sha": self.model_sha,
            "output_mp4": output_mp4,
        }


@app.local_entrypoint()
def main(
    video: str,
    start: float = 0.0,
    end: float = 10.0,
    x: float = 0.5,
    y: float = 0.5,
    runs: int = 2,
    out: str = "measure_out",
) -> None:
    """video を runs 回つづけて処理する（1 回目 = コールド、2 回目以降 = ウォーム）。x, y は 1 枚目の被写体の位置（0〜1）。"""
    sys.path.insert(0, str(HERE))
    from measure_stats import format_report, summarize_ms

    src = Path(video).expanduser().resolve()
    video_bytes = src.read_bytes()
    out_dir = Path(out).expanduser().resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    print(f"{src.name}（{len(video_bytes) / 1e6:.1f} MB）を {runs} 回処理します…")

    measurer = Measure()
    results = []
    for i in range(1, runs + 1):
        t = time.perf_counter()
        server = measurer.measure.remote(
            video_bytes, int(start * 1000), int(end * 1000), [(x, y, 1)], (0, 255, 0)
        )
        wall = time.perf_counter() - t
        mp4 = server.pop("output_mp4")
        (out_dir / f"{src.stem}_run{i}.mp4").write_bytes(mp4)
        server["stages_ms"] = {k: summarize_ms(v) for k, v in server["raw_ms"].items()}
        results.append({"run": i, "client_wall_s": wall, "server": server})
        print(f"  {i} 回目: {wall:.1f} 秒（{'コールド' if server['cold'] else 'ウォーム'}）")

    report = format_report(src.name, results)
    (out_dir / f"{src.stem}_results.json").write_text(
        json.dumps({"video": src.name, "start": start, "end": end, "x": x, "y": y, "runs": results},
                   ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    (out_dir / f"{src.stem}_report.md").write_text(report, encoding="utf-8")
    print()
    print(report)
    print()
    print(f"出力: {out_dir}")
