"""scripts/fetch_preview_models.py と fetch_models(graphs=...) のテスト（ネットワークには出ない）。"""

from __future__ import annotations

import contextlib
import importlib.util
from pathlib import Path

import pytest

from cutout import models

SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "fetch_preview_models.py"


class _FakeResponse:
    def __init__(self, body: bytes):
        self._body = body

    def raise_for_status(self) -> None:
        pass

    def iter_bytes(self):
        yield self._body


@pytest.fixture
def requested(monkeypatch) -> list[str]:
    """httpx.stream を偽物にし、要求された URL を記録する。本文は URL の末尾をバイト列にしたもの。"""
    urls: list[str] = []

    @contextlib.contextmanager
    def fake_stream(method, url, **kwargs):
        assert method == "GET"
        urls.append(url)
        yield _FakeResponse(url.rsplit("/", 1)[-1].encode())

    monkeypatch.setattr(models.httpx, "stream", fake_stream)
    return urls


def _load_script():
    spec = importlib.util.spec_from_file_location("fetch_preview_models", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_fetch_models_with_graphs_fetches_only_those(tmp_path, requested):
    models.fetch_models(tmp_path, "abc123", graphs=["vision_encoder", "mask_decoder"])

    base = models.REPO_URL.format(revision="abc123")
    assert requested == [
        base + "constants.json",
        base + "onnx/vision_encoder.onnx",
        base + "onnx/mask_decoder.onnx",
    ]
    assert (tmp_path / "onnx" / "mask_decoder.onnx").read_bytes() == b"mask_decoder.onnx"
    assert not list(tmp_path.rglob("*.part"))


def test_fetch_models_without_graphs_fetches_all(tmp_path, requested):
    models.fetch_models(tmp_path, "abc123")

    assert len(requested) == 1 + len(models.GRAPH_NAMES)


def test_script_fetches_preview_files_pinned_to_model_revision(tmp_path, requested, capsys):
    script = _load_script()

    assert script.main([str(tmp_path / "out")]) == 0

    base = models.REPO_URL.format(revision=models.MODEL_REVISION)
    assert requested == [base + rel for rel in script.PREVIEW_FILES]
    assert sorted(p.relative_to(tmp_path / "out").as_posix() for p in (tmp_path / "out").rglob("*") if p.is_file()) == [
        "constants.json",
        "onnx/mask_decoder.onnx",
        "onnx/vision_encoder.onnx",
    ]
    assert models.MODEL_REVISION in capsys.readouterr().out


def test_script_skips_existing_files(tmp_path, requested):
    script = _load_script()
    out = tmp_path / "out"
    script.main([str(out)])
    requested.clear()

    script.main([str(out)])

    assert requested == []


def test_script_default_dir_is_repo_models_dir():
    script = _load_script()
    repo_root = Path(__file__).resolve().parents[2]

    assert script.DEFAULT_DIR == repo_root / "models" / "sam2.1-tiny-video"
