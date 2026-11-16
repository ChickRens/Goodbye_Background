"""動画切り抜きワーカーの Modal アダプタ（本番）。

- Web エンドポイント `web`（POST /、Modal のプロキシ認証が必須）: 依頼 JSON を検証し、正しければ
  GPU の `Worker.process` を spawn して 202 {"accepted": true}、契約違反なら 400。
  Rust の `MODAL_ENDPOINT_URL` はこのエンドポイントの URL。
- `Worker.process`: cutout.pipeline.run_job で 1 本処理する。結果は Rust の callback_url に通知する。

デプロイの手順は同じフォルダの README.md を参照（`modal deploy worker/modal_app.py`）。
このファイルの Modal に依存しない部分は cutout/accept.py・cutout/transfer.py にあり、そちらをテストする。
"""

from __future__ import annotations

from pathlib import Path

import modal

HERE = Path(__file__).resolve().parent
CUTOUT_DIR = HERE / "cutout"  # worker/cutout（リポジトリのどこから deploy しても解決できるようにする）
REMOTE_CUTOUT_DIR = "/root/cutout"
MODEL_DIR = Path("/models")
LOGO_PATH = Path(REMOTE_CUTOUT_DIR) / "assets" / "logo.png"

APP_NAME = "goodbye-background-video"
SECRET_NAME = "goodbye-background-video"
SECRET_KEY = "WORKER_CALLBACK_SECRET"

# L4 が埋まっていたら A10G → T4 の順に取る。L4 だけだと割り当て待ちで数分待たされた（docs/video-cutout-measurements.md）
GPU_FALLBACK = ["L4", "A10G", "T4"]

# onnxruntime-gpu は 1.27 から CUDA 13 向けになったので、CUDA 12 + cuDNN 9 向けの最後の版に固定する
ORT_GPU_VERSION = "1.26.0"
# worker/uv.lock と同じ版
# onnx は DAM4SAM（cutout.tracker_variants）が、マスクデコーダの候補マスクを出力に足すのに使う
_PY_DEPS = ["numpy==2.5.4", "opencv-python-headless==5.0.0.93", "httpx==0.28.1", "onnx==1.23.2"]
_IGNORE = ["__pycache__", "**/__pycache__"]


def download_models() -> None:
    """イメージのビルド時に 1 回だけ動く。cutout.models.MODEL_REVISION に固定したモデルを取得する。"""
    from cutout.models import MODEL_REVISION, fetch_models

    fetch_models(MODEL_DIR, MODEL_REVISION)
    print(f"モデルを取得しました: revision={MODEL_REVISION}")


# GPU の処理用: Debian slim + ffmpeg + pip の CUDA / cuDNN（計測で実ジョブを確認した「slim」イメージ）。
# pip で入れた CUDA / cuDNN は cutout.models.load_sessions の ort.preload_dlls() が読み込む。
# ビルド中の download_models が cutout.models を使うので、cutout は copy=True でイメージに焼き込む
gpu_image = (
    modal.Image.debian_slim(python_version="3.12")
    .apt_install("ffmpeg")
    .pip_install(f"onnxruntime-gpu[cuda,cudnn]=={ORT_GPU_VERSION}", *_PY_DEPS)
    .add_local_dir(CUTOUT_DIR, remote_path=REMOTE_CUTOUT_DIR, copy=True, ignore=_IGNORE)
    .run_function(download_models)
)

# 受け付け用: 軽い CPU のイメージ（fastapi_endpoint には FastAPI が要る）。使うのは cutout.accept / contract だけ
web_image = (
    modal.Image.debian_slim(python_version="3.12")
    .pip_install("fastapi[standard]==0.162.0")
    .add_local_dir(CUTOUT_DIR, remote_path=REMOTE_CUTOUT_DIR, ignore=_IGNORE)
)

with web_image.imports():
    from fastapi import Request  # web の引数の型（FastAPI がコンテナ内で解決する）

app = modal.App(APP_NAME)


@app.cls(
    image=gpu_image,
    gpu=GPU_FALLBACK,
    timeout=300,
    retries=0,
    scaledown_window=60,
    secrets=[modal.Secret.from_name(SECRET_NAME, required_keys=[SECRET_KEY])],
)
class Worker:
    @modal.enter()
    def load(self) -> None:
        """コンテナの起動時に 1 回だけ: モデル（CUDA）・ロゴ・送受信・通知の秘密を用意する。"""
        import os

        from cutout.compositor import LogoBadge
        from cutout.logging_setup import configure_logging
        from cutout.models import load_constants, load_sessions
        from cutout.tracker_variants import CANDIDATE_OUTPUTS
        from cutout.transfer import HttpTransfer

        configure_logging()  # httpx が署名付き URL を INFO で出さないようにする
        # device="cuda" は CUDA EP が先頭でなければ例外になる（= CPU に落ちて遅くなるのを防ぐ）
        # DAM4SAM は選ばれなかった候補マスクも使うので、マスクデコーダに出力を足して読む
        self.sessions = load_sessions(MODEL_DIR, "cuda", decoder_extra_outputs=CANDIDATE_OUTPUTS)
        self.constants = load_constants(MODEL_DIR)
        self.logo = LogoBadge.load(LOGO_PATH)
        self.transfer = HttpTransfer()
        # 前後の空白・改行は落とす（Rust 側の config も trim する。貼り付けで改行が混ざると署名が合わなくなるため）
        self.secret = os.environ[SECRET_KEY].strip().encode()

    @modal.method()
    def process(self, body: dict) -> None:
        """1 本処理する。body は accept で検証済みの依頼 JSON。run_job は例外を出さず、必ず final を通知する。"""
        import time

        from cutout.accept import job_workdir
        from cutout.contract import WorkerJobRequest
        from cutout.notifier import Notifier, httpx_post
        from cutout.pipeline import run_job
        from cutout.tracker_variants import PRODUCTION_POLICY, VariantTracker

        req = WorkerJobRequest.from_json(body)
        # 時計は UNIX 秒（署名のタイムスタンプに使う。time.monotonic ではない）
        notifier = Notifier(req.callback_url, self.secret, post=httpx_post, clock=time.time, sleep=time.sleep)
        run_job(
            req,
            self.transfer,
            notifier,
            lambda total: VariantTracker(self.sessions, self.constants, total, PRODUCTION_POLICY),
            self.logo,
            job_workdir(req.job_id),
        )


@app.function(image=web_image, timeout=60)
@modal.fastapi_endpoint(method="POST", requires_proxy_auth=True)
async def web(request: Request):
    """POST /: 依頼を受け付けて GPU の処理を spawn する（処理の完了は待たない）。"""
    import asyncio
    import json

    from fastapi.responses import JSONResponse

    from cutout.accept import accept

    try:
        body = json.loads(await request.body())
    except ValueError:  # JSON として読めない本文は契約違反（accept が 400 にする）
        body = None
    # spawn は Modal の同期 API なので、イベントループを塞がないよう別スレッドで呼ぶ
    status, payload = await asyncio.to_thread(accept, body, Worker().process.spawn)
    return JSONResponse(payload, status_code=status)
