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


def _call_to_start(call_started_at: float | None, module_imported_at: float | None) -> float | None:
    """呼び出した時刻（手元の PC の time.time()）からコンテナで Python がこのファイルを読み込むまでの秒。"""
    if call_started_at is None or module_imported_at is None:
        return None
    return module_imported_at - call_started_at


_CLOCK_NOTE = (
    "※ 呼び出し→コンテナ起動 = コンテナの時計での読み込み時刻 − 手元の PC の時計での呼び出し時刻。"
    "手元の PC の時計のずれ（数秒）がそのまま入る。"
)


def format_report(video_name: str, runs: list[dict], image: str | None = None) -> str:
    """オーナーがチャットに貼る日本語の結果表を作る。

    runs の要素: {"run": 番号, "client_wall_s": 手元で測った呼び出しの所要秒,
                 "call_started_at": 呼び出した時刻（epoch 秒。無くてもよい）,
                 "server": measure の戻り値（mp4 以外。"module_imported_at" = コンテナでの読み込み時刻）}
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
    ]
    if image:
        lines.append(f"- イメージ: {image}")
    lines += [
        "",
        "| 回 | 種別 | 手元の所要(s) | サーバー処理(s) | 差(手元−サーバー)(s) | コールドスタート推定・送受信除く(s) | モデル読込(s) | 起動→準備完了(s) | 呼び出し→コンテナ起動(s) | フレーム | 見失い | 1フレーム平均(ms) | 推定費用(USD) |",
        "|---|---|---|---|---|---|---|---|---|---|---|---|---|",
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
        # ウォームの回は前の回と同じコンテナなので、呼び出し→コンテナ起動は意味を持たない
        to_start = _call_to_start(r.get("call_started_at"), s.get("module_imported_at")) if cold else None
        lines.append(
            f"| {r['run']} | {'コールド' if cold else 'ウォーム'} | {_f(r['client_wall_s'])} | {_f(s['total_s'])}"
            f" | {_f(overhead)} | {_f(cold_start)} | {_f(s['model_load_s']) if cold else '-'}"
            f" | {_f(s['import_to_ready_s']) if cold else '-'} | {_f(to_start)} | {s['frames']} | {s['lost_frames']}"
            f" | {_f(per_frame)} | {estimate_cost_usd(gpu_s):.4f} |"
        )
    lines += [
        "",
        "コールドスタート推定（送受信を除く）: "
        + (f"{_f(statistics.fmean(corrected))} 秒" if corrected else "-（ウォームの回が無いので出せない）"),
        "= コールドの差 − ウォームの差の平均。判断（≤ 60 秒）にはこの値を使い、モデル読込・起動→準備完了・"
        "呼び出し→コンテナ起動の列も併せて見る（呼び出し→コンテナ起動 = Python が動く前の待ち）。",
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
        "Python が動く前のコンテナ起動分・待機（scaledown_window 60 秒）分・CPU とメモリ分は含まない。"
        "L4 以外の GPU に割り当たった回は単価が違う。",
        _CLOCK_NOTE,
    ]
    return "\n".join(lines)


# 原因の切り分け（probe）で呼ぶ 3 つの小さな関数
PROBE_LABELS = {
    "cpu_cuda_image": "CUDA イメージ・GPU なし",
    "gpu_tiny_image": "最小イメージ・GPU あり",
    "gpu_slim_image": "slim イメージ・GPU あり",
}

# 「≫」とみなす条件: 大きい方が小さい方の 2 倍以上、かつ 20 秒以上の差
_DOMINANCE_RATIO = 2.0
_DOMINANCE_MIN_DIFF_S = 20.0


def _median_start(rows: list[dict], probe: str) -> float | None:
    values = [r["call_to_start_s"] for r in rows if r["probe"] == probe and r["call_to_start_s"] is not None]
    return statistics.median(values) if values else None


def probe_verdict(rows: list[dict]) -> str:
    """GPU なしで CUDA イメージを起動した待ちと、最小イメージで GPU を取った待ち（中央値）を比べた判定の一行。"""
    image_s = _median_start(rows, "cpu_cuda_image")
    gpu_s = _median_start(rows, "gpu_tiny_image")
    if image_s is None or gpu_s is None:
        return "判定: 判定できない（CUDA イメージ・GPU なし と 最小イメージ・GPU あり の両方の値が要る）"
    detail = f"（中央値: GPU の割り当て待ち {gpu_s:.1f} 秒 / CUDA イメージの起動 {image_s:.1f} 秒）"

    def dominates(big: float, small: float) -> bool:
        return big >= small * _DOMINANCE_RATIO and big - small >= _DOMINANCE_MIN_DIFF_S

    if dominates(gpu_s, image_s):
        return "判定: GPU の割り当て待ちが主因" + detail
    if dominates(image_s, gpu_s):
        return "判定: イメージの起動が主因" + detail
    return "判定: どちらが主因とも言えない（差が小さいか、両方が効いている）" + detail


def format_probe_report(rows: list[dict]) -> str:
    """probe の結果表（オーナーがチャットに貼る）。

    rows の要素: {"round", "probe"（PROBE_LABELS のキー）, "call_to_start_s"（None 可）, "wall_s", "gpu_name", "note"}
    """
    lines = [
        "## 原因の切り分け（probe）",
        "",
        "| 回 | 関数 | 呼び出し→起動(s) | 全体(s) | GPU 名 | 備考 |",
        "|---|---|---|---|---|---|",
    ]
    for r in rows:
        lines.append(
            f"| {r['round']} | {PROBE_LABELS.get(r['probe'], r['probe'])} | {_f(r['call_to_start_s'])}"
            f" | {_f(r['wall_s'])} | {r['gpu_name']} | {r['note'] or '-'} |"
        )
    lines += [
        "",
        probe_verdict(rows),
        "",
        "※ CUDA イメージ・GPU なし = 大きいイメージの起動だけ / 最小イメージ・GPU あり = GPU の割り当て待ちだけ /"
        " slim イメージ・GPU あり = 新しい軽いイメージで本番に近い起動。",
        _CLOCK_NOTE.replace("呼び出し→コンテナ起動", "呼び出し→起動"),
    ]
    return "\n".join(lines)
