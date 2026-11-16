"""ジョブの依頼と通知の型（Rust との契約）。

JSON の形は `contracts/video_cutout/*.json` が唯一の正。Rust 側も同じファイルをテストで読む。
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Literal

Label = Literal["include", "exclude"]
FailureCode = Literal[
    "internal_error",
    "subject_not_found",
    "unsupported_video",
    "range_out_of_bounds",
    "too_large",
]

MAX_POINTS = 8
MAX_PROMPTS = 5  # 点を打てるフレームの数（開始位置を含む）
MAX_RANGE_MS = 10000


class ContractError(ValueError):
    """依頼 JSON が契約に違反している。"""


@dataclass(frozen=True)
class NormalizedPoint:
    """0..1 に正規化したクリック座標（フレームの幅・高さに対する割合）。"""

    x: float
    y: float
    label: Label


@dataclass(frozen=True)
class Prompt:
    """1 フレーム分の点。time_ms は元の動画の時刻（range_ms と同じ基準）。"""

    time_ms: int
    points: list[NormalizedPoint]


@dataclass(frozen=True)
class RangeMs:
    start: int
    end: int


@dataclass(frozen=True)
class WorkerJobRequest:
    job_id: str
    input_url: str
    output_url: str
    range_ms: RangeMs
    # 1 つ目は開始位置。古い依頼（prompts が無く points だけ）は、開始位置の 1 つの prompt として読む
    prompts: list[Prompt]
    background_rgb: tuple[int, int, int]
    watermark: bool
    max_input_bytes: int
    callback_url: str

    @staticmethod
    def from_json(data: Any) -> WorkerJobRequest:
        if not isinstance(data, dict):
            raise ContractError("依頼はオブジェクトでなければならない")
        range_ms = _range(_get(data, "range_ms"))
        if "prompts" in data:
            prompts = _prompts(data["prompts"], range_ms)
        else:
            prompts = [Prompt(range_ms.start, _points(_get(data, "points")))]
        return WorkerJobRequest(
            job_id=_str(data, "job_id"),
            input_url=_str(data, "input_url"),
            output_url=_str(data, "output_url"),
            range_ms=range_ms,
            prompts=prompts,
            background_rgb=_rgb(_get(data, "background_rgb")),
            watermark=_bool(data, "watermark"),
            max_input_bytes=_int(data, "max_input_bytes"),
            callback_url=_str(data, "callback_url"),
        )


def _get(data: dict, key: str) -> Any:
    if key not in data:
        raise ContractError(f"{key} がない")
    return data[key]


def _str(data: dict, key: str) -> str:
    v = _get(data, key)
    if not isinstance(v, str) or not v:
        raise ContractError(f"{key} は空でない文字列でなければならない")
    return v


def _bool(data: dict, key: str) -> bool:
    v = _get(data, key)
    if not isinstance(v, bool):
        raise ContractError(f"{key} は真偽値でなければならない")
    return v


def _is_int(v: Any) -> bool:
    return isinstance(v, int) and not isinstance(v, bool)


def _int(data: dict, key: str) -> int:
    v = _get(data, key)
    if not _is_int(v) or v <= 0:
        raise ContractError(f"{key} は正の整数でなければならない")
    return v


def _range(v: Any) -> RangeMs:
    if not isinstance(v, dict):
        raise ContractError("range_ms はオブジェクトでなければならない")
    start, end = _get(v, "start"), _get(v, "end")
    if not _is_int(start) or not _is_int(end):
        raise ContractError("range_ms.start/end は整数でなければならない")
    if not (0 <= start < end) or end - start > MAX_RANGE_MS:
        raise ContractError("range_ms は 0 <= start < end かつ長さ 10000ms 以下")
    return RangeMs(start, end)


def _points(v: Any) -> list[NormalizedPoint]:
    if not isinstance(v, list) or not (1 <= len(v) <= MAX_POINTS):
        raise ContractError(f"points は 1〜{MAX_POINTS} 個の配列でなければならない")
    out = []
    for p in v:
        if not isinstance(p, dict):
            raise ContractError("point はオブジェクトでなければならない")
        x, y, label = _get(p, "x"), _get(p, "y"), _get(p, "label")
        for c in (x, y):
            if isinstance(c, bool) or not isinstance(c, (int, float)) or not (0.0 <= c <= 1.0):
                raise ContractError("point.x/y は 0..1 の数でなければならない")
        if label not in ("include", "exclude"):
            raise ContractError("point.label は include / exclude のどちらか")
        out.append(NormalizedPoint(float(x), float(y), label))
    return out


def _prompts(v: Any, range_ms: RangeMs) -> list[Prompt]:
    """1〜MAX_PROMPTS 個。1 つ目は range_ms.start、昇順で重複なし、start <= time_ms < end。"""
    if not isinstance(v, list) or not (1 <= len(v) <= MAX_PROMPTS):
        raise ContractError(f"prompts は 1〜{MAX_PROMPTS} 個の配列でなければならない")
    out: list[Prompt] = []
    for p in v:
        if not isinstance(p, dict):
            raise ContractError("prompt はオブジェクトでなければならない")
        t = _get(p, "time_ms")
        if not _is_int(t) or not (range_ms.start <= t < range_ms.end):
            raise ContractError("prompt.time_ms は range_ms の中の整数でなければならない")
        if out and t <= out[-1].time_ms:
            raise ContractError("prompts は time_ms の昇順で重複なし")
        out.append(Prompt(t, _points(_get(p, "points"))))
    if out[0].time_ms != range_ms.start:
        raise ContractError("1 つ目の prompt は range_ms.start でなければならない")
    return out


def _rgb(v: Any) -> tuple[int, int, int]:
    if not isinstance(v, list) or len(v) != 3 or not all(_is_int(c) and 0 <= c <= 255 for c in v):
        raise ContractError("background_rgb は 0..255 の整数 3 つでなければならない")
    return (v[0], v[1], v[2])


def event_downloading(job_id: str) -> dict:
    return {"job_id": job_id, "phase": "downloading"}


def event_tracking(job_id: str, progress: float) -> dict:
    return {"job_id": job_id, "phase": "tracking", "progress": progress}


def event_finishing(job_id: str) -> dict:
    return {"job_id": job_id, "phase": "finishing"}


def event_done(job_id: str, frames: int, lost_frames: int, duration_ms: int) -> dict:
    return {
        "job_id": job_id,
        "phase": "done",
        "frames": frames,
        "lost_frames": lost_frames,
        "duration_ms": duration_ms,
    }


def event_failed(job_id: str, code: FailureCode) -> dict:
    return {"job_id": job_id, "phase": "failed", "code": code}
