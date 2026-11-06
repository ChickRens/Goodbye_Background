import pytest

from measure.measure_stats import L4_USD_PER_S, estimate_cost_usd, format_report, summarize_ms


def test_summarize_ms_returns_avg_median_max():
    s = summarize_ms([10.0, 20.0, 90.0])
    assert s == {"n": 3, "avg": 40.0, "median": 20.0, "max": 90.0}


def test_summarize_ms_even_count_median_is_mean_of_middle():
    assert summarize_ms([1.0, 2.0, 3.0, 4.0])["median"] == 2.5


def test_summarize_ms_empty_is_none():
    assert summarize_ms([]) == {"n": 0, "avg": None, "median": None, "max": None}


def test_estimate_cost_uses_l4_rate():
    assert L4_USD_PER_S == 0.000222
    assert estimate_cost_usd(100.0) == pytest.approx(0.0222)


def _run(index, cold, wall_s, total_s, model_load_s):
    return {
        "run": index,
        "client_wall_s": wall_s,
        "server": {
            "cold": cold,
            "model_load_s": model_load_s,
            "import_to_ready_s": model_load_s + 1.0,
            "total_s": total_s,
            "frames": 300,
            "lost_frames": 2,
            "stages_ms": {
                "decode": summarize_ms([5.0]),
                "track": summarize_ms([40.0, 60.0]),
                "composite": summarize_ms([8.0]),
                "encode_write": summarize_ms([3.0]),
            },
            "encode_finish_ms": 120.0,
            "probe": {"fps": 29.97, "width": 1080, "height": 1920, "is_hdr": True, "has_audio": True},
            "output": {"width": 606, "height": 1080, "fps": 29.97},
            "zscale_available": True,
            "ffmpeg_version": "ffmpeg version 6.1.1",
            "onnxruntime_version": "1.26.0",
            "available_providers": ["CUDAExecutionProvider", "CPUExecutionProvider"],
            "gpu_name": "NVIDIA L4",
            "model_sha": "abc123",
        },
    }


def test_format_report_shows_cold_start_estimate_and_key_numbers():
    text = format_report("clip.mov", [_run(1, True, 50.0, 20.0, 8.0), _run(2, False, 22.0, 20.0, 8.0)])
    assert "clip.mov" in text
    assert "abc123" in text
    assert "コールド" in text and "ウォーム" in text
    # コールドスタートの推定 = クライアントの所要時間 - サーバーの処理時間
    assert "30.0" in text
    # ウォームの回はモデル読み込みを出さない（その呼び出しでは読んでいない）
    warm = next(l for l in text.splitlines() if l.startswith("| 2 | ウォーム"))
    assert "8.0" not in warm
    assert "zscale" in text and "あり" in text
    assert "NVIDIA L4" in text
    # 推定費用: サーバー処理 20 秒 × L4 単価
    assert f"{estimate_cost_usd(20.0):.4f}" in text
