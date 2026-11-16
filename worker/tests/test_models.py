"""cutout/models.py のマスクデコーダの固定（SHA-256）と、Hugging Face から取るグラフの一覧のテスト。"""

import hashlib

import pytest

from cutout.models import GRAPH_NAMES, MASK_DECODER_FILE, MASK_DECODER_SHA256, verify_sha256


def test_verify_sha256_accepts_matching_file(tmp_path):
    path = tmp_path / "mask.onnx"
    path.write_bytes(b"abc")
    verify_sha256(path, hashlib.sha256(b"abc").hexdigest())


def test_verify_sha256_rejects_other_file(tmp_path):
    path = tmp_path / "mask.onnx"
    path.write_bytes(b"abc")
    with pytest.raises(ValueError, match="mask"):
        verify_sha256(path, "0" * 64)


def test_graph_names_exclude_mask_decoder():
    # マスクデコーダは自前で書き出したもの（Modal Volume）を使い、Hugging Face からは取らない
    assert "mask_decoder" not in GRAPH_NAMES
    assert len(GRAPH_NAMES) == 4


def test_mask_decoder_is_pinned_to_the_validated_file():
    # Rust の segmenter_sam2.rs の MASK_DECODER_SHA256 と同じ値（仕様 2026-11-16 §2.1）
    assert MASK_DECODER_FILE == "mask_decoder_with_mask_v1.onnx"
    assert MASK_DECODER_SHA256 == "ba8862fdba8cb0027f7314bfb649fe18c16852a792ad6ca0dc76818aacac6ba6"
