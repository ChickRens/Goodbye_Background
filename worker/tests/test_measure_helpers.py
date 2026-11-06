import pytest

from measure.measure_stats import (
    L4_USD_PER_S,
    PROBE_LABELS,
    estimate_cost_usd,
    format_probe_report,
    format_report,
    probe_verdict,
    summarize_ms,
)


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
    cold = next(l.split(" | ") for l in text.splitlines() if l.startswith("| 1 | コールド"))
    warm = next(l.split(" | ") for l in text.splitlines() if l.startswith("| 2 | ウォーム"))
    # 列: 回 | 種別 | 手元 | サーバー | 送受信込みの差 | 送受信除く推定 | モデル読込 | 起動→準備完了 | ...
    assert cold[4] == "30.0" and warm[4] == "2.0"  # 差（手元 − サーバー）は全ての回で出す
    # 送受信を除いたコールドスタート = コールドの差 − ウォームの差の平均 = 30 − 2
    assert cold[5] == "28.0" and warm[5] == "-"
    assert "コールドスタート推定（送受信を除く）: 28.0 秒" in text
    # ウォームの回はモデル読み込みを出さない（その呼び出しでは読んでいない）
    assert cold[6] == "8.0" and warm[6] == "-"
    assert "zscale" in text and "あり" in text
    assert "NVIDIA L4" in text
    # 推定費用: ウォームはサーバー処理 20 秒、コールドは + 起動→準備完了 9 秒
    assert warm[-1] == f"{estimate_cost_usd(20.0):.4f} |"
    assert cold[-1] == f"{estimate_cost_usd(29.0):.4f} |"


def test_format_report_without_warm_run_cannot_correct_cold_start():
    text = format_report("clip.mov", [_run(1, True, 50.0, 20.0, 8.0)])
    cold = next(l.split(" | ") for l in text.splitlines() if l.startswith("| 1 | コールド"))
    assert cold[4] == "30.0" and cold[5] == "-"
    assert "コールドスタート推定（送受信を除く）: -" in text


def _cold_line(text):
    return next(l.split(" | ") for l in text.splitlines() if l.startswith("| 1 | コールド"))


def test_format_report_shows_call_to_container_start_for_cold_run_only():
    cold = _run(1, True, 50.0, 20.0, 8.0)
    cold["call_started_at"] = 1000.0
    cold["server"]["module_imported_at"] = 1012.5
    warm = _run(2, False, 22.0, 20.0, 8.0)
    warm["call_started_at"] = 1100.0
    warm["server"]["module_imported_at"] = 1012.5  # 同じコンテナ（ウォーム）なので意味が無い
    text = format_report("clip.mov", [cold, warm], image="slim")
    header = next(l for l in text.splitlines() if l.startswith("| 回 |")).split(" | ")
    col = header.index("呼び出し→コンテナ起動(s)")
    assert _cold_line(text)[col] == "12.5"
    warm_line = next(l.split(" | ") for l in text.splitlines() if l.startswith("| 2 | ウォーム"))
    assert warm_line[col] == "-"
    # モデル読込・起動→準備完了の列も残す
    assert "モデル読込(s)" in header and "起動→準備完了(s)" in header
    assert "イメージ: slim" in text
    assert "時計" in text  # 手元の PC の時計のずれの注記


def test_format_report_without_epochs_shows_dash():
    text = format_report("clip.mov", [_run(1, True, 50.0, 20.0, 8.0)])
    header = next(l for l in text.splitlines() if l.startswith("| 回 |")).split(" | ")
    assert _cold_line(text)[header.index("呼び出し→コンテナ起動(s)")] == "-"


def _probe(round_, probe, start, wall=None, gpu="-", note=""):
    return {"round": round_, "probe": probe, "call_to_start_s": start,
            "wall_s": (start + 1.0) if wall is None and start is not None else wall,
            "gpu_name": gpu, "note": note}


def test_format_probe_report_has_row_per_round_and_probe():
    rows = [
        _probe(1, "cpu_cuda_image", 5.0),
        _probe(1, "gpu_tiny_image", 200.0, gpu="NVIDIA L4"),
        _probe(1, "gpu_slim_image", 210.0, gpu="NVIDIA A10G", note="モデル読込 1.2 秒"),
    ]
    text = format_probe_report(rows)
    lines = [l for l in text.splitlines() if l.startswith("| 1 |")]
    assert len(lines) == 3
    tiny = next(l.split(" | ") for l in lines if PROBE_LABELS["gpu_tiny_image"] in l)
    assert tiny[2] == "200.0" and tiny[3] == "201.0" and tiny[4] == "NVIDIA L4"
    assert "モデル読込 1.2 秒" in text
    assert "判定" in text
    assert "時計" in text


def test_format_probe_report_failed_probe_shows_dash_and_note():
    text = format_probe_report([_probe(1, "gpu_slim_image", None, wall=3.0, note="失敗: boom")])
    row = next(l.split(" | ") for l in text.splitlines() if l.startswith("| 1 |"))
    assert row[2] == "-" and row[3] == "3.0" and "失敗: boom" in row[-1]


def test_probe_verdict_gpu_wait_dominates():
    rows = [_probe(r, "cpu_cuda_image", 8.0) for r in (1, 2, 3)] + [
        _probe(r, "gpu_tiny_image", s) for r, s in ((1, 150.0), (2, 300.0), (3, 90.0))
    ]
    assert "GPU の割り当て待ちが主因" in probe_verdict(rows)


def test_probe_verdict_image_start_dominates():
    rows = [_probe(1, "cpu_cuda_image", 200.0), _probe(1, "gpu_tiny_image", 10.0)]
    assert "イメージの起動が主因" in probe_verdict(rows)


def test_probe_verdict_similar_is_inconclusive():
    rows = [_probe(1, "cpu_cuda_image", 100.0), _probe(1, "gpu_tiny_image", 120.0)]
    v = probe_verdict(rows)
    assert "主因" not in v.replace("どちらが主因とも言えない", "")


def test_probe_verdict_missing_data():
    assert "判定できない" in probe_verdict([_probe(1, "cpu_cuda_image", None)])
