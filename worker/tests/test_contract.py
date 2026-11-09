import json
from pathlib import Path

import pytest

from cutout.contract import (
    ContractError,
    NormalizedPoint,
    Prompt,
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
    assert req.prompts[0] == Prompt(1200, [NormalizedPoint(0.51, 0.44, "include")])
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
    {"prompts": [{"time_ms": 1200, "points": []}]},
    {"prompts": [{"time_ms": 1200, "points": [{"x": 1.2, "y": 0.5, "label": "include"}]}]},
    {"prompts": [{"time_ms": 1200, "points": [{"x": 0.5, "y": 0.5, "label": "maybe"}]}]},
    {"prompts": [{"time_ms": 1200, "points": [{"x": 0.5, "y": 0.5, "label": "include"}] * 9}]},
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


P = [{"x": 0.5, "y": 0.5, "label": "include"}]


def example() -> dict:
    return json.loads((CONTRACTS / "worker_job_request.json").read_text())


def test_prompts_parsed_from_contract_example():
    req = WorkerJobRequest.from_json(example())
    assert [p.time_ms for p in req.prompts] == [1200, 4300]


def test_legacy_points_become_single_prompt_at_start():
    data = example()
    del data["prompts"]
    req = WorkerJobRequest.from_json(data)
    assert req.prompts == [Prompt(1200, [NormalizedPoint(0.51, 0.44, "include")])]


@pytest.mark.parametrize("prompts", [
    [{"time_ms": 1200 + i * 100, "points": P} for i in range(6)],  # 6 個
    [{"time_ms": 1300, "points": P}],  # 1 つ目が開始位置でない
    [{"time_ms": 1200, "points": P}, {"time_ms": 1200, "points": P}],  # 重複
    [{"time_ms": 1200, "points": P}, {"time_ms": 5000, "points": P}, {"time_ms": 4000, "points": P}],  # 降順
    [{"time_ms": 1200, "points": P}, {"time_ms": 9200, "points": P}],  # 終わり以上
    [{"time_ms": 1200, "points": P}, {"time_ms": 4000.5, "points": P}],  # 整数でない
    [],
])
def test_prompts_rules_rejected(prompts):
    with pytest.raises(ContractError):
        WorkerJobRequest.from_json({**example(), "prompts": prompts})
