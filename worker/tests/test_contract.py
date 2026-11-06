import json
from pathlib import Path

import pytest

from cutout.contract import (
    ContractError,
    NormalizedPoint,
    WorkerJobRequest,
    event_done,
    event_downloading,
    event_failed,
    event_finishing,
    event_tracking,
)

CONTRACTS = Path(__file__).parents[2] / "contracts" / "video_cutout"


def test_request_example_parses():
    req = WorkerJobRequest.from_json(json.loads((CONTRACTS / "worker_job_request.json").read_text()))
    assert req.range_ms.start == 1200 and req.range_ms.end == 9200
    assert req.points[0] == NormalizedPoint(0.51, 0.44, "include")
    assert req.background_rgb == (0, 255, 0) and req.max_input_bytes == 209715200


@pytest.mark.parametrize("name,builder", [
    ("downloading", lambda j: event_downloading(j)),
    ("tracking", lambda j: event_tracking(j, 0.45)),
    ("finishing", lambda j: event_finishing(j)),
    ("done", lambda j: event_done(j, 240, 3, 8000)),
    ("failed", lambda j: event_failed(j, "unsupported_video")),
])
def test_events_match_examples(name, builder):
    expected = json.loads((CONTRACTS / f"worker_event_{name}.json").read_text())
    assert builder(expected["job_id"]) == expected


@pytest.mark.parametrize("patch", [
    {"points": []},
    {"points": [{"x": 1.2, "y": 0.5, "label": "include"}]},
    {"points": [{"x": 0.5, "y": 0.5, "label": "maybe"}]},
    {"points": [{"x": 0.5, "y": 0.5, "label": "include"}] * 9},
    {"range_ms": {"start": 0, "end": 10001}},
    {"range_ms": {"start": 5, "end": 5}},
    {"range_ms": {"start": -1, "end": 5}},
    {"background_rgb": [0, 255]},
    {"background_rgb": [0, 255, 256]},
    {"watermark": "yes"},
    {"max_input_bytes": "1"},
])
def test_request_rejects_invalid(patch):
    data = {**json.loads((CONTRACTS / "worker_job_request.json").read_text()), **patch}
    with pytest.raises(ContractError):
        WorkerJobRequest.from_json(data)


def test_request_rejects_missing_key():
    data = json.loads((CONTRACTS / "worker_job_request.json").read_text())
    del data["callback_url"]
    with pytest.raises(ContractError):
        WorkerJobRequest.from_json(data)
