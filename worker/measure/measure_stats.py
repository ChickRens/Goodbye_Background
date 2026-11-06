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
    "手元の PC の時計のずれ（数秒）がそのまま入る（気になるなら計測前に管理者の PowerShell で `w32tm /resync`）。"
    "2 つの値の差で比べれば、ずれは打ち消される。"
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
        "※ 計測の呼び出し→コンテナ起動 には、手元の PC からの動画のアップロード時間が入ることがある（probe の値は動画を送らないので入らない）。",
        _CLOCK_NOTE,
    ]
    return "\n".join(lines)


# 原因の切り分け（probe）で呼ぶ 4 つの小さな関数（この順に呼ぶ）
PROBE_LABELS = {
    "gpu_tiny_image": "最小イメージ・GPU あり",
    "gpu_cuda_image": "CUDA イメージ・GPU あり",
    "gpu_slim_image": "slim イメージ・GPU あり",
    "cpu_cuda_image": "CUDA イメージ・GPU なし（参考）",
}

# 判定のしきい値: どちらも これ未満なら「小さい」。主因 = もう一方の 2 倍以上
_SMALL_S = 30.0
_DOMINANCE_RATIO = 2.0


def _median(rows: list[dict], probe: str, key: str) -> float | None:
    values = [r[key] for r in rows if r["probe"] == probe and r.get(key) is not None]
    return statistics.median(values) if values else None


def probe_breakdown(rows: list[dict]) -> dict:
    """呼び出し→起動の中央値から、GPU の割り当て待ちと各イメージの上乗せ（秒）を出す。

    GPU の割り当て待ち ≒ 最小イメージ・GPU あり。イメージの上乗せ ≒ そのイメージ・GPU あり − 最小イメージ・GPU あり。
    差を取るので、手元の PC の時計のずれは打ち消される。
    """
    tiny = _median(rows, "gpu_tiny_image", "call_to_start_s")
    cuda = _median(rows, "gpu_cuda_image", "call_to_start_s")
    slim = _median(rows, "gpu_slim_image", "call_to_start_s")
    return {
        "gpu_wait_s": tiny,
        "cuda_extra_s": cuda - tiny if cuda is not None and tiny is not None else None,
        "slim_extra_s": slim - tiny if slim is not None and tiny is not None else None,
        "cuda_model_load_s": _median(rows, "gpu_cuda_image", "model_load_s"),
        "slim_model_load_s": _median(rows, "gpu_slim_image", "model_load_s"),
    }


def probe_verdict(rows: list[dict]) -> str:
    """GPU の割り当て待ちと CUDA イメージの上乗せを比べた判定（複数行）。"""
    b = probe_breakdown(rows)
    lines = [
        f"GPU の割り当て待ち（tiny）: {_f(b['gpu_wait_s'])} 秒",
        f"CUDA イメージの上乗せ: {_f(b['cuda_extra_s'])} 秒（別にモデル読込 {_f(b['cuda_model_load_s'])} 秒）",
        f"slim イメージの上乗せ: {_f(b['slim_extra_s'])} 秒（別にモデル読込 {_f(b['slim_model_load_s'])} 秒）",
    ]
    gpu, image = b["gpu_wait_s"], b["cuda_extra_s"]
    if gpu is None or image is None:
        verdict = "判定できない（最小イメージ・GPU あり と CUDA イメージ・GPU あり の両方の値が要る）"
    elif gpu < _SMALL_S and image < _SMALL_S:
        verdict = "どちらも小さい（今回は速かった）"
    elif gpu >= image * _DOMINANCE_RATIO:
        verdict = "GPU の割り当て待ちが主因"
    elif image >= gpu * _DOMINANCE_RATIO:
        verdict = "イメージの起動が主因"
    else:
        verdict = "両方が効いている（どちらが主因とも言えない）"
    lines.append(f"判定: {verdict}")
    return "\n".join(lines)


def format_probe_report(rows: list[dict]) -> str:
    """probe の結果表（オーナーがチャットに貼る）。

    rows の要素: {"round", "probe"（PROBE_LABELS のキー）, "call_to_start_s"（None 可）, "wall_s", "gpu_name", "note",
                 "model_load_s"（GPU ありの CUDA / slim のみ。無くてもよい）}
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
        "※ 最小イメージ・GPU あり = GPU の割り当て待ちだけ。CUDA / slim イメージ・GPU あり − それ = イメージの起動の上乗せ"
        "（どれも呼び出し→起動の中央値）。モデル読込（CUDA / cuDNN のライブラリを実際に読む時間）は別に出している。",
        "※ CUDA イメージ・GPU なし は参考値（CUDA のライブラリを読まないので、イメージの重さが出きらないことがある）。判定には使わない。",
        "※ 上乗せは差なので、手元の PC の時計のずれは打ち消される。",
        _CLOCK_NOTE.replace("呼び出し→コンテナ起動", "呼び出し→起動"),
    ]
    return "\n".join(lines)
