"""Modal の Web エンドポイントのうち、Modal に依存しない部分（受け付けと作業ディレクトリ）。"""

from __future__ import annotations

import logging
import re
from collections.abc import Callable
from pathlib import Path
from typing import Any

from cutout.contract import ContractError, WorkerJobRequest

logger = logging.getLogger(__name__)

WORK_ROOT = Path("/tmp")
_UNSAFE = re.compile(r"[^A-Za-z0-9_-]")


def accept(body: Any, spawn: Callable[[dict], None]) -> tuple[int, dict]:
    """依頼を検証し、正しければ spawn(body) して (202, {"accepted": True})。契約違反なら spawn せずに 400。

    body は JSON を解析した値（解析できなかった場合は None）。spawn には受け取った body をそのまま渡す。
    エラーの本文には ContractError のメッセージ（キー名と条件だけで、URL は含まない）を返す。
    """
    try:
        WorkerJobRequest.from_json(body)
    except ContractError as e:
        logger.warning("契約違反の依頼を拒否: %s", e)
        return 400, {"accepted": False, "error": str(e)}
    spawn(body)
    return 202, {"accepted": True}


def job_workdir(job_id: str) -> Path:
    """ジョブの作業ディレクトリ /tmp/{job_id}。job_id は UUID の想定だが、/tmp の外に出ないよう記号は _ に置き換える。"""
    name = _UNSAFE.sub("_", job_id)
    return WORK_ROOT / name
