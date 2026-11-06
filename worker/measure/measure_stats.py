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


def _overhead(run: dict) -> float:
    return run["client_wall_s"] - run["server"]["total_s"]


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
        "| 回 | 種別 | 手元の所要(s) | サーバー処理(s) | 差(手元−サーバー)(s) | コールドスタート推定・送受信除く(s) | モデル読込(s) | 起動→準備完了(s) | フレーム | 見失い | 1フレーム平均(ms) | 推定費用(USD) |",
        "|---|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    # 差（手元 − サーバー）には動画の送受信が入る。本番のワーカーは R2 から取るので、
    # ウォームの差（≒ 送受信 + 呼び出しの往復）の平均を引いた値をコールドスタートとみなす
    warm_overheads = [_overhead(r) for r in runs if not r["server"]["cold"]]
    transfer_s = statistics.fmean(warm_overheads) if warm_overheads else None
    corrected = []
    for r in runs:
        s = r["server"]
        cold = s["cold"]
        overhead = _overhead(r)
        cold_start = overhead - transfer_s if cold and transfer_s is not None else None
        if cold_start is not None:
            corrected.append(cold_start)
        per_frame = s["total_s"] * 1000 / s["frames"] if s["frames"] else None
        # コールドの回は Python 起動〜モデル読込（起動→準備完了）もコンテナの GPU 時間に入るので費用に足す
        gpu_s = s["total_s"] + (s["import_to_ready_s"] if cold else 0.0)
        lines.append(
            f"| {r['run']} | {'コールド' if cold else 'ウォーム'} | {_f(r['client_wall_s'])} | {_f(s['total_s'])}"
            f" | {_f(overhead)} | {_f(cold_start)} | {_f(s['model_load_s']) if cold else '-'}"
            f" | {_f(s['import_to_ready_s']) if cold else '-'} | {s['frames']} | {s['lost_frames']}"
            f" | {_f(per_frame)} | {estimate_cost_usd(gpu_s):.4f} |"
        )
    lines += [
        "",
        "コールドスタート推定（送受信を除く）: "
        + (f"{_f(statistics.fmean(corrected))} 秒" if corrected else "-（ウォームの回が無いので出せない）"),
        "= コールドの差 − ウォームの差の平均。判断（≤ 60 秒）にはこの値を使い、モデル読込・起動→準備完了の列も併せて見る。",
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
        "※ 差（手元−サーバー）= コンテナ起動・モデル読込・動画の送受信・呼び出しの往復の合計（ウォームでは送受信と往復だけ）。",
        f"※ 推定費用は GPU 秒 × L4 単価 ${L4_USD_PER_S}/秒 の概算。コールドの回は起動→準備完了の秒を足している。"
        "Python が動く前のコンテナ起動分・待機（scaledown_window 60 秒）分・CPU とメモリ分は含まない。",
    ]
    return "\n".join(lines)
