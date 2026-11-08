"""Modal（L4）でのコールドスタートと処理時間の計測（Task 1 の使い捨て）。

本番と同じ部品（worker/cutout/ の video_io・tracker・models・compositor）で、
デコード → 推論 → 合成 + ロゴ → エンコード を 1 本通し、工程ごとの時間を返す。
使い方は同じフォルダの README.md を参照。

    modal run worker/measure/measure_app.py::main --video C:\\path\\to\\clip.mov --x 0.5 --y 0.5 --image slim
    modal run worker/measure/measure_app.py::probe --rounds 3   # コールドスタートの原因の切り分け
    modal run worker/measure/measure_app.py::main --video clip.mp4 --image slim --model small   # 案 C: small の時間と費用
    modal run worker/measure/measure_app.py::main --video clip.mp4 --image slim --model small --policy dam4sam   # 記憶の選び方の比較
"""

import json
import sys
import time
from pathlib import Path

import modal

# コンテナ内では「Python がこのファイルを読み込んだ時刻」= コンテナ起動のほぼ直後
_MODULE_IMPORTED_AT = time.time()

HERE = Path(__file__).resolve().parent
CUTOUT_DIR = HERE.parent / "cutout"  # worker/cutout（リポジトリのどこから実行しても解決できるようにする）

# 計測で選べるモデル（cutout.models.VIDEO_MODELS の名前）。どれもリビジョン固定でイメージに焼き込む。
# small は案 C（大きいモデル）の時間と費用を GPU で測るため（2026-11-08。CPU では tiny の約 7 倍だった）
MEASURE_MODELS = ["tiny", "small", "base-plus"]
# 記憶の選び方（cutout.tracker_variants.POLICIES）。sam2 = 本番の追跡（tracker.py）をそのまま使う
MEASURE_POLICIES = ["sam2", "dam4sam", "samurai"]
MODEL_ROOT = Path("/models")


def model_dir(name: str) -> Path:
    return MODEL_ROOT / name

# L4 が埋まっていたら A10G → T4 の順に取る（Modal のフォールバック指定）。実際に付いた GPU 名は結果に出す
GPU_FALLBACK = ["L4", "A10G", "T4"]

# onnxruntime-gpu は 1.27 から CUDA 13 向けになったので、CUDA 12 + cuDNN 9 向けの最後の版に固定する
ORT_GPU_VERSION = "1.26.0"
CUDA_IMAGE = "nvidia/cuda:12.8.1-cudnn-runtime-ubuntu24.04"


def download_models() -> None:
    """イメージのビルド時に 1 回だけ動く。MEASURE_MODELS を、本番と同じくリビジョン固定で取得する。"""
    from cutout.models import VIDEO_MODELS, fetch_models

    for name in MEASURE_MODELS:
        repo, revision = VIDEO_MODELS[name]
        fetch_models(model_dir(name), revision, repo=repo)
        (model_dir(name) / "MODEL_SHA").write_text(f"{repo}@{revision}")
        print(f"モデルを取得しました: {repo}@{revision}")


# worker/uv.lock と同じ版
# onnx は記憶の選び方の比較（dam4sam / samurai）で、マスクデコーダの候補マスクを出力に足すのに使う
_PY_DEPS = ["numpy==2.5.4", "opencv-python-headless==5.0.0.93", "httpx==0.28.1", "onnx==1.23.2"]


def _with_cutout_and_models(img: modal.Image) -> modal.Image:
    # ビルド中の download_models が cutout.models を使うので copy=True でイメージに焼き込む
    return img.add_local_dir(
        CUTOUT_DIR, remote_path="/root/cutout", copy=True, ignore=["__pycache__", "**/__pycache__"]
    ).run_function(download_models)


# 1 回目の計測で使った大きいイメージ（CUDA + cuDNN の公式イメージ。数 GB）
cuda_image = _with_cutout_and_models(
    modal.Image.from_registry(CUDA_IMAGE, add_python="3.12")
    .apt_install("ffmpeg")
    .pip_install(f"onnxruntime-gpu=={ORT_GPU_VERSION}", *_PY_DEPS)
)

# 軽いイメージ: Debian slim + pip の CUDA / cuDNN（onnxruntime-gpu の extras が nvidia-*-cu12 の wheel を入れる）。
# pip で入れた CUDA / cuDNN は cutout.models.load_sessions の ort.preload_dlls() が読み込む
slim_image = _with_cutout_and_models(
    modal.Image.debian_slim(python_version="3.12")
    .apt_install("ffmpeg")
    .pip_install(f"onnxruntime-gpu[cuda,cudnn]=={ORT_GPU_VERSION}", *_PY_DEPS)
)

# GPU の割り当て待ちだけを測るための最小のイメージ
tiny_image = modal.Image.debian_slim(python_version="3.12")

IMAGES = {"cuda": cuda_image, "slim": slim_image}

app = modal.App("goodbye-background-measure")


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


class _Measure:
    """Measure の中身。イメージだけが違う 2 つのクラス（MeasureCuda / MeasureSlim）が継承する。

    model はコンテナごとの引数（MeasureSlim(model="small")）。モデルが違うと別のコンテナになる。
    引数（modal.parameter）は継承したクラス側に書く: Modal はクラス自身の __dict__ にある parameter しか見ない
    （親に書くと「MeasureSlim() takes no arguments」でコンテナが起動しない。2026-11-08 に本番の計測で発生）。
    """

    @modal.enter()
    def load(self) -> None:
        t0 = time.time()
        import onnxruntime as ort

        from cutout.models import load_constants, load_sessions

        # device="cuda" は CUDA EP が先頭でなければ例外になる（= CPU に落ちていないことの確認）
        from cutout.tracker_variants import CANDIDATE_OUTPUTS

        if self.model not in MEASURE_MODELS:
            raise ValueError(f"model は {' / '.join(MEASURE_MODELS)} のどれか: {self.model!r}")
        if self.policy not in MEASURE_POLICIES:
            raise ValueError(f"policy は {' / '.join(MEASURE_POLICIES)} のどれか: {self.policy!r}")
        # sam2 は本番と同じ読み込み方（候補マスクの出力を足さない = 本番と同じ速さ）
        extra = None if self.policy == "sam2" else CANDIDATE_OUTPUTS
        self.sessions = load_sessions(model_dir(self.model), "cuda", decoder_extra_outputs=extra)
        self.constants = load_constants(model_dir(self.model))
        self.model_load_s = time.time() - t0
        self.import_to_ready_s = time.time() - _MODULE_IMPORTED_AT
        self.ort_version = ort.__version__
        self.providers = ort.get_available_providers()
        self.model_sha = (model_dir(self.model) / "MODEL_SHA").read_text().strip()
        self.ffmpeg_version, self.zscale = _ffmpeg_info()
        if not self.zscale:
            # HDR のトーンマップ（video_io._HDR_TONEMAP）に要る。無いと HDR 動画のデコードが途中で失敗する
            raise RuntimeError(f"ffmpeg に zscale が無い: {self.ffmpeg_version}")
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
        from cutout.tracker_variants import VariantTracker
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
            if self.policy == "sam2":
                tracker = Sam2VideoTracker(self.sessions, self.constants, total)
            else:
                tracker = VariantTracker(self.sessions, self.constants, total, self.policy)

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
            "policy": self.policy,
            # DAM4SAM: DRM に足したフレーム / SAMURAI: IoU 最大以外の候補を選んだ回数（sam2 は無し）
            "variant_stats": getattr(tracker, "stats", None),
            "module_imported_at": _MODULE_IMPORTED_AT,
            "output_mp4": output_mp4,
        }


@app.cls(image=cuda_image, gpu=GPU_FALLBACK, timeout=600, scaledown_window=60)
class MeasureCuda(_Measure):
    model: str = modal.parameter(default="tiny")
    policy: str = modal.parameter(default="sam2")


@app.cls(image=slim_image, gpu=GPU_FALLBACK, timeout=600, scaledown_window=60)
class MeasureSlim(_Measure):
    model: str = modal.parameter(default="tiny")
    policy: str = modal.parameter(default="sam2")


MEASURE_CLASSES = {"cuda": MeasureCuda, "slim": MeasureSlim}


# ---- 原因の切り分け（probe）----
# コンテナが確実に落ちてから次の回を呼べるよう、待機は Modal の最小値（2 秒）にする
_PROBE_SCALEDOWN_S = 2
PROBE_ROUND_INTERVAL_S = 90


def _probe_load_models() -> dict:
    """GPU 上で ONNX のセッションを CUDA で作る（CUDA / cuDNN のライブラリを実際に読ませる）。"""
    imported_at = _MODULE_IMPORTED_AT  # 読み込みの前に控える（呼び出し→起動にモデル読込を混ぜない）
    t0 = time.time()
    from cutout.models import load_sessions

    # device="cuda" は CUDA EP が先頭でなければ例外になる（= CUDA / cuDNN が読めていることの確認）
    load_sessions(model_dir("tiny"), "cuda")
    model_load_s = time.time() - t0
    ffmpeg_version, zscale = _ffmpeg_info()
    note = f"モデル読込 {model_load_s:.1f} 秒 / zscale {'あり' if zscale else 'なし'} / {ffmpeg_version}"
    return {"imported_at": imported_at, "gpu_name": _gpu_name(), "note": note, "model_load_s": model_load_s}


@app.function(image=tiny_image, gpu=GPU_FALLBACK, timeout=300, scaledown_window=_PROBE_SCALEDOWN_S)
def probe_gpu_tiny_image() -> dict:
    """最小のイメージで GPU を取る（= GPU の割り当て待ちだけを見る）。"""
    return {"imported_at": _MODULE_IMPORTED_AT, "gpu_name": _gpu_name(), "note": ""}


@app.function(image=cuda_image, gpu=GPU_FALLBACK, timeout=300, scaledown_window=_PROBE_SCALEDOWN_S)
def probe_gpu_cuda_image() -> dict:
    """1 回目の計測と同じ CUDA イメージで GPU を取り、モデルを CUDA で読む（最小イメージとの差 = このイメージの上乗せ）。"""
    return _probe_load_models()


@app.function(image=slim_image, gpu=GPU_FALLBACK, timeout=300, scaledown_window=_PROBE_SCALEDOWN_S)
def probe_gpu_slim_image() -> dict:
    """軽いイメージで GPU を取り、モデルを CUDA で読む（= 軽いイメージで本番どおり動くことの確認と、その上乗せ）。"""
    return _probe_load_models()


@app.function(image=cuda_image, timeout=300, scaledown_window=_PROBE_SCALEDOWN_S)
def probe_cpu_cuda_image() -> dict:
    """CUDA イメージを GPU なしで起動する（参考値。CUDA のライブラリを読まないので判定には使わない）。"""
    return {"imported_at": _MODULE_IMPORTED_AT, "gpu_name": "-", "note": "参考（CUDA のライブラリを読まない）"}


# 呼ぶ順番。GPU ありの 3 つを先に、参考の CPU を最後に
PROBES = {
    "gpu_tiny_image": probe_gpu_tiny_image,
    "gpu_cuda_image": probe_gpu_cuda_image,
    "gpu_slim_image": probe_gpu_slim_image,
    "cpu_cuda_image": probe_cpu_cuda_image,
}


@app.local_entrypoint()
def probe(rounds: int = 3) -> None:
    """4 つの小さな関数を順に呼び、呼び出し→コンテナ起動の秒を比べる。回の間は 90 秒あけてコンテナを落とす。"""
    sys.path.insert(0, str(HERE))
    from measure_stats import format_probe_report

    rows = []
    for rnd in range(1, rounds + 1):
        if rnd > 1:
            print(f"  コンテナが落ちるのを {PROBE_ROUND_INTERVAL_S} 秒待ちます…")
            time.sleep(PROBE_ROUND_INTERVAL_S)
        for key, fn in PROBES.items():
            print(f"  {rnd}/{rounds} 回目 {key}: 呼び出し中…（コールドなので数分かかることがある）", flush=True)
            call_started_at = time.time()
            t = time.perf_counter()
            try:
                res = fn.remote()
                to_start = res["imported_at"] - call_started_at
                gpu_name, note, model_load_s = res["gpu_name"], res["note"], res.get("model_load_s")
            except Exception as e:  # 1 つ失敗しても表は出す
                to_start, gpu_name, note, model_load_s = None, "-", f"失敗: {type(e).__name__}: {e}", None
            wall = time.perf_counter() - t
            rows.append({
                "round": rnd, "probe": key, "call_to_start_s": to_start, "wall_s": wall,
                "gpu_name": gpu_name, "note": note, "model_load_s": model_load_s,
            })
            start_text = "-" if to_start is None else f"{to_start:.1f}"
            print(f"  {rnd}/{rounds} 回目 {key}: 終了 全体 {wall:.1f} 秒 / 呼び出し→起動 {start_text} 秒 / {gpu_name}", flush=True)

    print()
    print(format_probe_report(rows))


@app.local_entrypoint()
def main(
    video: str,
    start: float = 0.0,
    end: float = 10.0,
    x: float = 0.5,
    y: float = 0.5,
    runs: int = 2,
    out: str = "measure_out",
    image: str = "cuda",
    model: str = "tiny",
    policy: str = "sam2",
) -> None:
    """video を runs 回つづけて処理する（1 回目 = コールド、2 回目以降 = ウォーム）。x, y は 1 枚目の被写体の位置（0〜1）。

    image: cuda = 1 回目の計測と同じ CUDA イメージ / slim = 軽いイメージ（未検証なので既定は cuda）
    model: tiny = 本番 / small = 案 C（入力 1024）
    """
    if image not in MEASURE_CLASSES:
        raise SystemExit(f"--image は {' / '.join(MEASURE_CLASSES)} のどれか: {image!r}")
    if model not in MEASURE_MODELS:
        raise SystemExit(f"--model は {' / '.join(MEASURE_MODELS)} のどれか: {model!r}")
    if policy not in MEASURE_POLICIES:
        raise SystemExit(f"--policy は {' / '.join(MEASURE_POLICIES)} のどれか: {policy!r}")
    label = f"{model}_{policy}"
    sys.path.insert(0, str(HERE))
    from measure_stats import format_report, summarize_ms

    src = Path(video).expanduser().resolve()
    video_bytes = src.read_bytes()
    out_dir = Path(out).expanduser().resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    print(f"{src.name}（{len(video_bytes) / 1e6:.1f} MB）を {runs} 回処理します（イメージ: {image}、モデル: {model}、記憶: {policy}）…")

    measurer = MEASURE_CLASSES[image](model=model, policy=policy)
    results = []
    for i in range(1, runs + 1):
        call_started_at = time.time()
        t = time.perf_counter()
        server = measurer.measure.remote(
            video_bytes, int(start * 1000), int(end * 1000), [(x, y, 1)], (0, 255, 0)
        )
        wall = time.perf_counter() - t
        mp4 = server.pop("output_mp4")
        (out_dir / f"{src.stem}_{label}_run{i}.mp4").write_bytes(mp4)
        server["stages_ms"] = {k: summarize_ms(v) for k, v in server["raw_ms"].items()}
        results.append({"run": i, "client_wall_s": wall, "call_started_at": call_started_at, "server": server})
        print(f"  {i} 回目: {wall:.1f} 秒（{'コールド' if server['cold'] else 'ウォーム'}）/ {server['variant_stats']}")

    report = format_report(src.name, results, image=f"{image} / モデル: {model} / 記憶: {policy}")
    (out_dir / f"{src.stem}_{label}_results.json").write_text(
        json.dumps({"video": src.name, "image": image, "model": model, "policy": policy, "start": start, "end": end, "x": x, "y": y, "runs": results},
                   ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    (out_dir / f"{src.stem}_{label}_report.md").write_text(report, encoding="utf-8")
    print()
    print(report)
    print()
    print(f"出力: {out_dir}")
