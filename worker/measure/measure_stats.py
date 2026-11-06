"""計測結果の集計と表の整形（手元の PC 側だけで使う。Modal のコンテナには入れない）。"""

from __future__ import annotations

import statistics

# Modal の L4 の GPU 単価（USD/秒）。CPU・メモリ分は含まない概算。
L4_USD_PER_S = 0.000222

_STAGE_LABELS = [
    ("decode", "デコード"),
    ("track", "推論(tracker)"),
    ("composite", "合成+ロゴ"),
    ("encode_write", "エンコード書込"),
]


def summarize_ms(values: list[float]) -> dict:
    """ミリ秒のリストを件数・平均・中央値・最大にまとめる（小数 1 桁）。"""
    if not values:
        return {"n": 0, "avg": None, "median": None, "max": None}
    return {
        "n": len(values),
        "avg": round(statistics.fmean(values), 1),
        "median": round(statistics.median(values), 1),
        "max": round(max(values), 1),
    }


def estimate_cost_usd(gpu_seconds: float) -> float:
    """GPU を使った秒数からの概算費用（USD）。"""
    return gpu_seconds * L4_USD_PER_S


def _f(v, digits: int = 1) -> str:
    return "-" if v is None else f"{v:.{digits}f}"


def format_report(video_name: str, runs: list[dict]) -> str:
    """オーナーがチャットに貼る日本語の結果表を作る。

    runs の要素: {"run": 番号, "client_wall_s": 手元で測った呼び出しの所要秒, "server": measure の戻り値（mp4 以外）}
    """
    s0 = runs[0]["server"]
    p, o = s0["probe"], s0["output"]
    lines = [
        f"## 計測結果: {video_name}",
        "",
        f"- モデル sha: {s0['model_sha']}",
        f"- GPU: {s0['gpu_name']} / onnxruntime {s0['onnxruntime_version']} / providers: {', '.join(s0['available_providers'])}",
        f"- ffmpeg: {s0['ffmpeg_version']} / zscale: {'あり' if s0['zscale_available'] else 'なし'}",
        f"- 入力: {p['width']}x{p['height']} {p['fps']:.2f}fps HDR={'はい' if p['is_hdr'] else 'いいえ'} 音声={'あり' if p['has_audio'] else 'なし'}",
        f"- 出力: {o['width']}x{o['height']} {o['fps']:.2f}fps",
        "",
        "| 回 | 種別 | 手元の所要(s) | サーバー処理(s) | コールドスタート推定(s) | モデル読込(s) | 起動→準備完了(s) | フレーム | 見失い | 1フレーム平均(ms) | 推定費用(USD) |",
        "|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    for r in runs:
        s = r["server"]
        cold = s["cold"]
        overhead = r["client_wall_s"] - s["total_s"]
        per_frame = s["total_s"] * 1000 / s["frames"] if s["frames"] else None
        # コールドの回はモデル読込もコンテナの GPU 時間に入るので費用に足す
        gpu_s = s["total_s"] + (s["model_load_s"] if cold else 0.0)
        lines.append(
            f"| {r['run']} | {'コールド' if cold else 'ウォーム'} | {_f(r['client_wall_s'])} | {_f(s['total_s'])}"
            f" | {_f(overhead) if cold else '-'} | {_f(s['model_load_s']) if cold else '-'}"
            f" | {_f(s['import_to_ready_s']) if cold else '-'} | {s['frames']} | {s['lost_frames']}"
            f" | {_f(per_frame)} | {estimate_cost_usd(gpu_s):.4f} |"
        )
    lines += [
        "",
        "工程ごとの 1 フレームあたり(ms) 平均 / 中央値 / 最大:",
        "",
        "| 回 | " + " | ".join(label for _, label in _STAGE_LABELS) + " | エンコード終了待ち(ms) |",
        "|---|" + "---|" * (len(_STAGE_LABELS) + 1),
    ]
    for r in runs:
        st = r["server"]["stages_ms"]
        cells = [f"{_f(st[k]['avg'])} / {_f(st[k]['median'])} / {_f(st[k]['max'])}" for k, _ in _STAGE_LABELS]
        lines.append(f"| {r['run']} | " + " | ".join(cells) + f" | {_f(r['server']['encode_finish_ms'])} |")
    lines += [
        "",
        "※ コールドスタート推定 = 手元の所要 − サーバー処理（コンテナ起動・モデル読込・動画の送受信を含む）。",
        f"※ 推定費用は GPU 秒 × L4 単価 ${L4_USD_PER_S}/秒 の概算。待機（scaledown_window 60 秒）分と CPU・メモリ分は含まない。",
    ]
    return "\n".join(lines)
