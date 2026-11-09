"""ジョブ全体の処理: ダウンロード → デコード → 推論 → 合成 → エンコード → アップロード → 通知。

フレームは 1 枚ずつ流し、全フレームをメモリに溜めない。run_job は例外を外に出さず、
最後に必ず final 通知を 1 回送り、作業ディレクトリを消す。
"""

from __future__ import annotations

import logging
import shutil
from collections.abc import Callable
from pathlib import Path
from typing import Protocol

from cutout.compositor import LogoBadge, alpha_from_logits, composite
from cutout.contract import (
    FailureCode,
    WorkerJobRequest,
    event_done,
    event_downloading,
    event_failed,
    event_finishing,
    event_tracking,
)
from cutout.errors import InputTooLarge
from cutout.notifier import Notifier
from cutout.postprocess import DetachedBlobFilter

# 体から離れた塊を消す後処理（cutout/postprocess.py）を本番でかけるか。D_man での評価が済むまで False
# （本人の手を消さないことを確かめてから True にする。2026-11-09）
REMOVE_DETACHED_BLOBS = False
from cutout.tracker import Sam2VideoTracker
from cutout.video_io import (
    MAX_FRAMES,
    FfmpegError,
    FrameReader,
    Mp4Writer,
    UnsupportedVideo,
    output_geometry,
    probe,
)

logger = logging.getLogger(__name__)

CONTENT_TYPE = "video/mp4"


class Transfer(Protocol):
    def head_size(self, url: str) -> int | None: ...

    def download(self, url: str, dest: Path, max_bytes: int) -> None:
        """url を dest に保存する。max_bytes を超えた時点で止めて InputTooLarge を投げる。"""
        ...

    def upload(self, url: str, src: Path, content_type: str) -> None: ...


class _Failure(Exception):
    """失敗コードが決まっている中断。"""

    def __init__(self, code: FailureCode):
        super().__init__(code)
        self.code = code


def run_job(
    req: WorkerJobRequest,
    transfer: Transfer,
    notifier: Notifier,
    make_tracker: Callable[[int], Sam2VideoTracker],
    logo: LogoBadge,
    workdir: Path,
) -> None:
    try:
        workdir.mkdir(parents=True, exist_ok=True)
        result = _process(req, transfer, notifier, make_tracker, logo, workdir)
    except _Failure as f:
        result = event_failed(req.job_id, f.code)
    except UnsupportedVideo:
        result = event_failed(req.job_id, "unsupported_video")
    except Exception:
        logger.exception("ジョブが想定外のエラーで失敗 (job_id=%s)", req.job_id)
        result = event_failed(req.job_id, "internal_error")
    finally:
        shutil.rmtree(workdir, ignore_errors=True)
    try:
        notifier.final(result)
    except Exception:
        logger.exception("final 通知に失敗 (job_id=%s)", req.job_id)


def _process(req, transfer, notifier, make_tracker, logo, workdir) -> dict:
    notifier.progress(event_downloading(req.job_id))
    size = transfer.head_size(req.input_url)
    if size is not None and size > req.max_input_bytes:
        raise _Failure("too_large")

    src = workdir / "input"
    out = workdir / "output.mp4"
    try:
        transfer.download(req.input_url, src, req.max_input_bytes)
    except InputTooLarge:
        raise _Failure("too_large") from None
    # HttpTransfer は読みながら止めるが、上限を守らない Transfer の実装に備えた二重の確認
    if src.stat().st_size > req.max_input_bytes:
        raise _Failure("too_large")

    info = probe(src)
    start_ms = req.range_ms.start
    if start_ms >= info.duration_ms:
        raise _Failure("range_out_of_bounds")
    end_ms = min(req.range_ms.end, info.duration_ms)  # 末尾を少し超える指定は切り詰める

    width, height, fps = output_geometry(info)
    total = max(1, min(MAX_FRAMES, round((end_ms - start_ms) / 1000 * fps)))
    tracker = make_tracker(total)

    frames = lost = 0
    blob_filter = DetachedBlobFilter()
    with FrameReader(src, start_ms, end_ms, width, height, fps, info.is_hdr) as reader, Mp4Writer(
        out, width, height, fps, src if info.has_audio else None, start_ms, end_ms
    ) as writer:
        while True:
            try:
                frame = next(reader)
            except StopIteration:
                break
            except FfmpegError:
                if frames == 0:
                    raise _Failure("unsupported_video")  # 入力が読めない
                raise
            result = tracker.process(frame, req.prompts[0].points if frames == 0 else None)
            if result.object_present:
                # 体から離れた塊（同じ服の隣の人の切れ端など）を消す。1 枚目は触らない（プレビューと一致させる）
                logits = result.high_res_logits
                if REMOVE_DETACHED_BLOBS:
                    logits, _ = blob_filter.apply(logits)
                alpha = alpha_from_logits(logits, width, height)
                composed = composite(frame, alpha, req.background_rgb)
            else:
                if frames == 0:
                    raise _Failure("subject_not_found")
                lost += 1
                composed = composite(frame, None, req.background_rgb)
            if req.watermark:
                composed = logo.apply(composed)
            writer.write(composed)
            frames += 1
            notifier.progress(event_tracking(req.job_id, min(1.0, frames / total)))
        if frames == 0:
            raise _Failure("range_out_of_bounds")
        notifier.progress(event_finishing(req.job_id))
        writer.close()

    transfer.upload(req.output_url, out, CONTENT_TYPE)
    return event_done(req.job_id, frames, lost, round(frames / fps * 1000))
