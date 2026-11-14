"""Modal のアダプタのうち、Modal に依存しない受け付け処理（cutout.accept）のテスト。"""

import json
from pathlib import Path

from cutout.accept import accept, job_workdir

CONTRACTS = Path(__file__).parents[2] / "contracts" / "video_cutout"


def example_request() -> dict:
    return json.loads((CONTRACTS / "worker_job_request.json").read_text())


def test_accept_valid_request_spawns_and_returns_202():
    spawned = []
    assert accept(example_request(), spawned.append) == (202, {"accepted": True}) and len(spawned) == 1


def test_accept_passes_body_unchanged_to_spawn():
    spawned = []
    body = example_request()
    accept(body, spawned.append)
    assert spawned == [body]


def test_accept_invalid_request_returns_400_without_spawn():
    spawned = []
    status, _ = accept({**example_request(), "prompts": []}, spawned.append)
    assert status == 400 and spawned == []


def test_accept_non_object_returns_400_without_spawn():
    spawned = []
    status, payload = accept(None, spawned.append)
    assert status == 400 and payload["accepted"] is False and spawned == []


def test_accept_error_does_not_echo_urls():
    # URL の値そのものが不正（文字列でない）でも、エラーの本文に値を返さない
    bad_url = {"href": "https://acct.r2.cloudflarestorage.com/in?X-Amz-Signature=secret-sig"}
    status, payload = accept({**example_request(), "input_url": bad_url}, lambda b: None)
    assert status == 400 and "secret-sig" not in json.dumps(payload) and "cloudflarestorage" not in json.dumps(payload)


def test_job_workdir_is_under_tmp_for_uuid():
    assert job_workdir("6f1c2b9e-0d4a-4c1e-9a57-3f2e8b1d7c60") == Path("/tmp/6f1c2b9e-0d4a-4c1e-9a57-3f2e8b1d7c60")


def test_job_workdir_cannot_escape_tmp():
    for job_id in ["..", "../root", "a/b", "/etc"]:
        d = job_workdir(job_id)
        assert d.parent == Path("/tmp") and d.name not in ("", ".", "..")
