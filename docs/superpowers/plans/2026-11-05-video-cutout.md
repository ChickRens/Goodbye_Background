# 動画切り抜き（MVP）実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** スマホから送った 10 秒以内の動画を、最初のタップだけでグリーンバック MP4 にして返す機能を、既存の画像切り抜きアプリに追加する。

**Architecture:** Rust（axum, Cloud Run 1 台）はジョブ・回数・署名付き URL だけを扱い、動画の中身には触れない。
動画は Cloudflare R2 を経由し、GPU 推論・合成・エンコードは Modal（L4）上の Python ワーカーが行う。
ワーカーの中核（`worker/cutout/`）は Modal を知らず、Rust 側も `ModalCutoutWorker` の 1 ファイル以外は Python を知らない。

**Tech Stack:** Rust 2024 / axum 0.8 / tokio / sqlx（SQLite・PostgreSQL）/ rusty-s3 / reqwest / hmac + sha2 / async-trait、
Python 3.12 / uv / onnxruntime(-gpu) / numpy / opencv-python-headless / httpx / pytest / Modal、ffmpeg（zscale 入り）、Vanilla JS。

**Spec:** `docs/superpowers/specs/2026-11-05-video-cutout-design.md`（以下「仕様」。§番号は仕様の節）

## Global Constraints

- コードの置き場所: Rust は `src/video_cutout/`、既存の画像機能は `src/image_cutout/`、ワーカーは `worker/`、契約の JSON は `contracts/video_cutout/`。
- 既存の約束事（`CLAUDE.md`「設計の約束事」）をすべて守る: handler → `App` → UseCase → Service/Port、`Arc<dyn Trait>`、trait と実装はファイルを分ける、エラーは `common/macros.rs` のマクロ、新ユースケースは `ApplicationError` に variant、コメント・ドキュメントは日本語。
- 例外（仕様 §4.4）: `VideoObjectStorage` / `CutoutWorker` / `DailyQuota` は `async-trait` の非同期 trait。動画のハンドラは `spawn_blocking` を使わない。
- クリップの上限 10,000ms、1 ジョブの点は 1〜8 個、x・y は 0.0〜1.0。
- 1 日の回数は 3 回。キーは `client-id` と IP の 2 系統で、日本時間（UTC+9 固定）の日付ごと。
- アップロード上限 209,715,200 バイト（200MB）。Content-Type は `video/mp4` か `video/quicktime`。
- 署名付き URL の有効期限: アップロード PUT 15 分、ワーカー用 GET/PUT 30 分、ダウンロード GET 10 分。
- 期限（仕様 §5.2）: `Created` 30 分、`Queued` 5 分、処理中は最後の通知から 3 分、`Done`/`Failed` 30 分。
- R2 のキー: 入力 `video-jobs/{job_id}/input`、出力 `video-jobs/{job_id}/output.mp4`。
- 背景色: `green` = (0,255,0)、`blue` = (0,0,255)、`magenta` = (255,0,255)。
- 出力: H.264 / yuv420p / MP4（`+faststart`）、長辺 1080px 以下の偶数サイズ、fps は min(元, 30)、音声は AAC（無ければ映像のみ）。
- フレームは 301 枚で打ち切る。
- 回数を返す失敗コード: `dispatch_failed`, `timeout`, `internal_error`, `subject_not_found`, `unsupported_video`。返さない: `range_out_of_bounds`, `too_large`。
- HMAC: `X-Signature-Timestamp`（UNIX 秒）、`X-Signature` = `hex(HMAC-SHA256(secret, "{timestamp}.{body}"))`、許容ずれ 300 秒、定数時間比較。
- 環境変数: `R2_ACCOUNT_ID`, `R2_BUCKET`, `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, `MODAL_ENDPOINT_URL`, `MODAL_KEY`, `MODAL_SECRET`, `WORKER_CALLBACK_SECRET`, `PUBLIC_BASE_URL`, `TRUSTED_PROXY_HOPS`（既定 0）。1 つでも欠けたら動画ルートは 503 `VIDEO_CUTOUT_DISABLED`。
- Cloud Run は最大 1 台・リクエストベースの CPU。R2 の削除はレスポンス前に await する。`tokio::spawn` に後処理を任せない。
- マイグレーションは `migrations/sqlite/` と `migrations/postgres/` の両方に**新しいファイル**として追加する。適用済みのファイルは変更しない。
- コミットメッセージにモデル名を書かない。各コミットの末尾に session のトレーラーを付ける（実行時のシステム指示に従う）。

## Review Focus

1. **選んだ範囲の終端が、動画の実際の長さをわずかに超える**（ブラウザの `duration` の丸め）→ `range_out_of_bounds` にせず、終端を長さに切り詰めて処理する。失敗にするのは `start_ms ≥ 長さ` のときだけ。→ Task 8 のテスト。
2. **音声トラックの無い動画**（画面収録など）→ 映像だけの MP4 を出す。→ Task 6 のテスト。
3. **奇数の解像度や小さな動画**（例: 721×405）→ 偶数サイズに直してエンコードが失敗しない。→ Task 6 のテスト。
4. **アップロードが終わる前に送信ボタンを押す** → `/start` は 409 `UPLOAD_NOT_FOUND` を返し、回数を消費しない。フロントはアップロード完了まで送信ボタンを無効にする。→ Task 13 のテストと Task 17 の確認。
5. **残り 1 回のときにジョブを 2 つ作り、両方を送信する** → 2 つ目の `/start` は 429 `QUOTA_EXCEEDED`、ジョブは `Created` のまま残る。→ Task 13 のテスト。

---

## ファイル構成

```
src/image_cutout/{image_io,editing,image,point,session}/   … Task 0 で移動（中身は不変）
src/api/handler/image_cutout/*                               … Task 0 で移動
src/api/handler/video_cutout/{create_job,start_job,get_job,report_event}/{handler,request,response}.rs
src/presentation/client_ip.rs                                 … IP の取り出し
src/video_cutout/
  mod.rs
  config.rs                     VideoCutoutConfig（環境変数の読み込み）
  clock.rs / clock_impl.rs      VideoClock（UNIX ミリ秒）と RealVideoClock / FakeVideoClock
  job/  id.rs  clip_range.rs  point.rs  background.rs  phase.rs  video_job.rs  error.rs
        repository.rs / repository_impl.rs   locks.rs
  quota/  daily_quota.rs / daily_quota_sqlite.rs / daily_quota_postgres.rs  jst_day.rs  error.rs
  storage/  video_object_storage.rs / r2_video_object_storage.rs  error.rs
  worker/   cutout_worker.rs / modal_cutout_worker.rs  request.rs  error.rs
  signature.rs                  HMAC の作成と検証
  create_job/ start_job/ get_job/ report_event/ delete_expired_jobs/   {input,output,error,usecase,mod}.rs
  testing.rs                    #[cfg(test)] の偽物（FakeStorage, FakeWorker, InMemoryQuota）
migrations/{sqlite,postgres}/20261106000000_video_quota_usage.sql
contracts/video_cutout/worker_job_request.json, worker_event_{downloading,tracking,finishing,done,failed}.json
worker/  pyproject.toml  cutout/{contract,tracker,compositor,video_io,notifier,pipeline}.py  cutout/assets/logo.png
         scripts/make_logo.py  modal_app.py  measure/  tests/
frontend/video.html  frontend/js/video/{page,range,preview,upload,job}.js  frontend/js/api.js  frontend/video-favicon.png
docs/video-cutout-measurements.md
```

---

### Task 0: 既存の画像機能を `src/image_cutout/` に移す（挙動は変えない）

**Files:**
- Move: `src/{image_io,editing,image,point,session}/` → `src/image_cutout/`（`git mv`）
- Move: `src/api/handler/{upload,get_preview,segment,undo,redo,save,get_completed}/` → `src/api/handler/image_cutout/`
- Create: `src/image_cutout/mod.rs`, `src/api/handler/image_cutout/mod.rs`
- Modify: `src/main.rs`（`mod` 宣言）、`crate::editing::` などを参照する全ファイル、`CLAUDE.md`（ディレクトリ構成）、`docs/ARCHITECTURE.md`（パス）

**Interfaces:**
- Produces: 以後のタスクは `crate::image_cutout::...` と `crate::api::handler::image_cutout::...` を使う。

- [ ] **Step 1: 移動前のテスト結果を記録する**

Run: `cargo test 2>&1 | grep -E "^test result|passed|failed" > /tmp/before.txt; cat /tmp/before.txt`
Expected: 全テスト PASS（件数を控える）。ONNX Runtime のリンクができない環境では `CLAUDE.md` の `ORT_LIB_LOCATION` の手順を使う。

- [ ] **Step 2: `git mv` で移動し、`use` のパスを一括で直す**

`crate::editing` → `crate::image_cutout::editing` のように、5 つのモジュールと 7 つのハンドラのパスを置き換える。ロジックは 1 行も変えない。

- [ ] **Step 3: 型チェックとテスト**

Run: `cargo check --all-targets && cargo test`
Expected: 警告は移動前と同じ 15 件、テスト件数と結果が Step 1 と一致。

- [ ] **Step 4: `CLAUDE.md` と `docs/ARCHITECTURE.md` のパスと構成図を更新してコミット**

```bash
git add -A && git commit -m "refactor: 画像切り抜きのモジュールを src/image_cutout/ に移動（挙動は不変）"
```

---

### Task 1: Modal でのコールドスタートと処理時間の計測（先に進むかの判断）

オーナーの作業が必要: Modal のアカウント（Starter、月の予算上限 $30）と、`modal token new` によるログイン。
このタスクのコードは計測専用（`worker/measure/`）で、後のタスクには流用しない。

**Files:**
- Create: `worker/measure/measure_app.py`, `worker/measure/README.md`, `docs/video-cutout-measurements.md`

- [ ] **Step 1: 計測用の Modal アプリを書く**

イメージは仕様 §6.6 と同じ（Debian + `zscale` 入りの ffmpeg + `onnxruntime-gpu` + numpy + opencv-python-headless、モデル 5 つと `constants.json` を `diffusionstudio/sam2.1-tiny-video-onnx-fp16` からリビジョン固定でビルド時に取得）。
関数 `measure(video_bytes: bytes, points: list[tuple[float,float,int]]) -> dict` は、`@modal.enter` でのセッション作成時間、1 フレームごとの時間、ffmpeg のデコード+エンコード込みの合計時間を返す。トラッキングは `bench.py` の `track` をそのまま貼り付けてよい（使い捨て）。

- [ ] **Step 2: 計測する**

Run: `modal run worker/measure/measure_app.py --video <iPhone の 10秒 30fps 1080p HDR 動画>` を、コールド（デプロイ直後）とウォーム（直後にもう 1 回）で各 3 回。
Expected: コールドスタート（コンテナ起動 + モデル読み込み）、1 フレーム平均、1 本の合計、Modal の請求画面での 1 本あたりの費用、HDR の色が褪せていないか、が取れる。

- [ ] **Step 3: 結果を `docs/video-cutout-measurements.md` に表で残し、判断する**

判断基準: コールドスタート ≤ 60 秒なら Modal で続行。超えたら Modal のメモリスナップショットを試して再計測し、それでも超えるなら作業を止めてオーナーに報告する（RunPod への切り替えは計画の変更として扱う）。
Modal のクレジットが外向き転送やストレージを含むかも、請求画面で確認して記録する。

- [ ] **Step 4: コミット**

```bash
git add worker/measure docs/video-cutout-measurements.md && git commit -m "spike: Modal L4 でのコールドスタートと処理時間を計測"
```

---

### Task 2: ワーカーの土台と契約（JSON の例と Python の型）

**Files:**
- Create: `worker/pyproject.toml`, `worker/cutout/__init__.py`, `worker/cutout/contract.py`, `worker/tests/test_contract.py`
- Create: `contracts/video_cutout/worker_job_request.json`, `contracts/video_cutout/worker_event_{downloading,tracking,finishing,done,failed}.json`
- Modify: `.dockerignore`（`worker` と `contracts` を追加）、`.gitignore`（`worker/.venv`, `worker/models`）

**Interfaces:**
- Produces（`cutout.contract`）:
  - `NormalizedPoint(x: float, y: float, label: Literal["include","exclude"])`
  - `WorkerJobRequest(job_id: str, input_url: str, output_url: str, range_ms: RangeMs(start:int,end:int), points: list[NormalizedPoint], background_rgb: tuple[int,int,int], watermark: bool, max_input_bytes: int, callback_url: str)`、`WorkerJobRequest.from_json(dict) -> WorkerJobRequest`（不正なら `ContractError`）
  - `FailureCode = Literal["internal_error","subject_not_found","unsupported_video","range_out_of_bounds","too_large"]`
  - `event_downloading(job_id) / event_tracking(job_id, progress: float) / event_finishing(job_id) / event_done(job_id, frames:int, lost_frames:int, duration_ms:int) / event_failed(job_id, code: FailureCode) -> dict`
- JSON の形は仕様 §6.2 のとおり。`contracts/` の各ファイルはその例をそのまま置く（`job_id` は `"6f1c2b9e-0d4a-4c1e-9a57-3f2e8b1d7c60"` に統一）。

- [ ] **Step 1: 失敗するテストを書く**

```python
CONTRACTS = Path(__file__).parents[2] / "contracts" / "video_cutout"

def test_request_example_parses():
    req = WorkerJobRequest.from_json(json.loads((CONTRACTS / "worker_job_request.json").read_text()))
    assert req.range_ms.start == 1200 and req.range_ms.end == 9200
    assert req.points[0] == NormalizedPoint(0.51, 0.44, "include")
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

@pytest.mark.parametrize("patch", [{"points": []}, {"points": [{"x": 1.2, "y": 0.5, "label": "include"}]},
                                   {"range_ms": {"start": 0, "end": 10001}}, {"background_rgb": [0, 255]}])
def test_request_rejects_invalid(patch):
    data = {**json.loads((CONTRACTS / "worker_job_request.json").read_text()), **patch}
    with pytest.raises(ContractError):
        WorkerJobRequest.from_json(data)
```

- [ ] **Step 2: 失敗を確認** — Run: `cd worker && uv run pytest tests/test_contract.py -v` / Expected: FAIL（`cutout.contract` が無い）
- [ ] **Step 3: `pyproject.toml`（Python 3.12、依存: numpy, opencv-python-headless, onnxruntime, httpx、dev: pytest）と `contract.py` を実装し、`contracts/` の 6 ファイルを作る**
- [ ] **Step 4: 成功を確認** — Run: `cd worker && uv run pytest tests/test_contract.py -v` / Expected: PASS
- [ ] **Step 5: コミット** — `git add worker contracts .dockerignore .gitignore && git commit -m "feat(worker): ワーカーの土台と Rust との契約 JSON を追加"`

---

### Task 3: メモリバンクとポインタの組み立て（純粋関数）

**Files:**
- Create: `worker/cutout/memory.py`, `worker/tests/test_memory.py`

**Interfaces:**
- Produces（`cutout.memory`）。定数 `MEM_FRAMES = 7`, `MAX_PTRS = 16`, `F2 = 1024`, `COND_ROW = 6`。
  - `MemoryEntry(tokens: np.ndarray[1024,1,64], pos: np.ndarray[1024,1,64])`
  - `memory_blocks(t: int, recent: dict[int, MemoryEntry]) -> list[tuple[Literal["cond"] | int, int]]` — `(どのメモリか, 時間埋め込みの行)` を 7 個返す。条件メモリはフレーム 0。
  - `assemble_memory(t, cond: MemoryEntry, recent: dict[int, MemoryEntry], tpos: np.ndarray[7,64]) -> tuple[np.ndarray[7168,64], np.ndarray[7168,64]]`
  - `pointer_list(t: int, total: int, cond_ptr: np.ndarray[256], recent_ptrs: dict[int, np.ndarray[256]]) -> tuple[np.ndarray[16,256], np.ndarray[16] float32]`
  - `pointer_tokens(ptrs: np.ndarray[16,256]) -> np.ndarray[64,64]`、`pointer_pos_tokens(pointer_pos: np.ndarray[16,64]) -> np.ndarray[64,64]`（各行を 4 回繰り返す）
- 規則は仕様 §6.4。`bench.py` の `assemble_memory` / `pointer_list` と同じ挙動にする（複製するのは「最新の過去フレーム」、無ければ条件メモリ。複製したブロックの行は複製元の行）。

- [ ] **Step 1: 失敗するテストを書く**

```python
def test_blocks_frame1_pads_with_cond_when_no_recent():
    assert memory_blocks(1, {}) == [("cond", 6)] * 7

def test_blocks_frame3_uses_recent_rows_and_pads_with_newest():
    assert memory_blocks(3, {1: e(), 2: e()}) == [("cond", 6), (2, 0), (1, 1)] + [(2, 0)] * 4

def test_blocks_frame10_uses_six_previous():
    recent = {f: e() for f in range(3, 10)}
    assert memory_blocks(10, recent) == [("cond", 6), (9, 0), (8, 1), (7, 2), (6, 3), (5, 4), (4, 5)]

def test_assemble_memory_adds_row_embedding():
    tpos = np.arange(7 * 64, dtype=np.float32).reshape(7, 64)
    mem, pos = assemble_memory(1, entry(value=1.0), {}, tpos)
    assert mem.shape == (7168, 64) and np.all(mem == 1.0)
    assert np.allclose(pos[0], 1.0 + tpos[6])

def test_pointer_list_normalizes_by_min_total_16_minus_1():
    ptrs, diffs = pointer_list(3, 100, p(0), {1: p(1), 2: p(2)})
    assert ptrs.shape == (16, 256)
    assert np.allclose(diffs[:3], [3 / 15, 1 / 15, 2 / 15]) and np.allclose(diffs[3:], 1 / 15)

def test_pointer_list_short_video_denominator():
    _, diffs = pointer_list(2, 5, p(0), {1: p(1)})
    assert np.allclose(diffs[:2], [2 / 4, 1 / 4])

def test_pointer_tokens_split_and_repeat():
    ptrs = np.arange(16 * 256, dtype=np.float32).reshape(16, 256)
    assert np.array_equal(pointer_tokens(ptrs)[1], ptrs[0, 64:128])
    pp = np.arange(16 * 64, dtype=np.float32).reshape(16, 64)
    assert np.array_equal(pointer_pos_tokens(pp)[0:4], np.repeat(pp[:1], 4, axis=0))
```
（`e()`, `entry(value)`, `p(v)` はテスト内の小さなヘルパー。`p(v)` は全要素が v の 256 次元。）

- [ ] **Step 2: 失敗を確認** — Run: `cd worker && uv run pytest tests/test_memory.py -v` / Expected: FAIL
- [ ] **Step 3: `memory.py` を実装する**（`bench.py` の 92〜127 行目と同じ規則）
- [ ] **Step 4: 成功を確認** — 同じコマンドで PASS
- [ ] **Step 5: コミット** — `git commit -m "feat(worker): SAM2 のメモリバンクとポインタの組み立てを追加"`

---

### Task 4: `Sam2VideoTracker`（ONNX の 5 グラフでフレームを順に処理）と移植の検証

**Files:**
- Create: `worker/cutout/tracker.py`, `worker/cutout/models.py`, `worker/tests/test_tracker.py`, `worker/tests/test_tracker_parity.py`

**Interfaces:**
- Consumes: `cutout.memory` の全関数、`cutout.contract.NormalizedPoint`
- Produces:
  - `models.fetch_models(model_dir: Path, revision: str) -> Path`（`constants.json` と `onnx/*.onnx` を取得。既にあれば何もしない）、`MODEL_REVISION: str`（Task 1 で使ったコミットハッシュ）
  - `models.load_sessions(model_dir: Path, device: Literal["cpu","cuda"]) -> Sam2Sessions`（cuda で CUDA EP が使えなければ例外）
  - `tracker.FrameResult(high_res_logits: np.ndarray[512,512] float32, object_present: bool)`
  - `tracker.Sam2VideoTracker(sessions: Sam2Sessions, constants: dict, total_frames: int)`、`.process(frame_rgb: np.ndarray[H,W,3] uint8, points: list[NormalizedPoint] | None) -> FrameResult`（1 枚目だけ points を渡す。2 枚目以降は None）
- 前処理・1 枚目・2 枚目以降の手順は仕様 §6.4。クリック点は `x*512, y*512`、ラベルは include=1 / exclude=0。
- `Sam2Sessions` は 5 つの `run(**feeds) -> dict` を持つ。テストでは偽物を差し込める形にする（`Protocol`）。
- 履歴は最新 6 フレームのメモリと 15 フレームのポインタだけ保持する（それより古いものは捨てる）。

- [ ] **Step 1: 偽のセッションで呼び出し順を確かめるテストを書く**

```python
def test_first_frame_uses_points_and_binarize_1():
    s = RecordingSessions(object_score=1.0)
    Sam2VideoTracker(s, CONSTANTS, total_frames=3).process(rgb(), [NormalizedPoint(0.5, 0.25, "include")])
    assert s.calls["mask_decoder"][0]["input_points"].tolist() == [[[[256.0, 128.0]]]]
    assert s.calls["mask_decoder"][0]["input_labels"].tolist() == [[[1]]]
    assert int(np.asarray(s.calls["memory_encoder"][0]["binarize"]).reshape(-1)[0]) == 1
    assert "memory_attention" not in s.calls

def test_second_frame_uses_memory_attention_and_pad_point():
    s = RecordingSessions(object_score=1.0)
    tr = Sam2VideoTracker(s, CONSTANTS, total_frames=3)
    tr.process(rgb(), [NormalizedPoint(0.5, 0.5, "include")]); tr.process(rgb(), None)
    assert s.calls["memory_attention"][0]["memory"].shape == (7 * 1024 + 64, 1, 64)
    assert s.calls["mask_decoder"][1]["input_labels"].tolist() == [[[-1]]]
    assert int(np.asarray(s.calls["memory_encoder"][1]["binarize"]).reshape(-1)[0]) == 0

def test_lost_when_object_score_not_positive():
    tr = Sam2VideoTracker(RecordingSessions(object_score=0.0), CONSTANTS, total_frames=1)
    assert tr.process(rgb(), [NormalizedPoint(0.5, 0.5, "include")]).object_present is False

def test_history_is_bounded():
    tr = Sam2VideoTracker(RecordingSessions(object_score=1.0), CONSTANTS, total_frames=40)
    tr.process(rgb(), [NormalizedPoint(0.5, 0.5, "include")])
    for _ in range(39): tr.process(rgb(), None)
    assert len(tr._recent) <= 6 and len(tr._recent_ptrs) <= 15
```

- [ ] **Step 2: 失敗を確認** — Run: `cd worker && uv run pytest tests/test_tracker.py -v` / Expected: FAIL
- [ ] **Step 3: `models.py` と `tracker.py` を実装する**
- [ ] **Step 4: 成功を確認** — 同じコマンドで PASS
- [ ] **Step 5: 移植の正しさのテスト（遅いテスト）を書く**

`test_tracker_parity.py` に `@pytest.mark.slow` を付ける。`goodbye-background-tmp` の `bench.py` を `BENCH_PY` 環境変数で指定し（無ければ skip）、同じ短い動画（`data/input.mp4` の先頭 10 フレーム、点 (0.51, 0.44)）を両方で CPU 実行して、各フレームの `low_res_mask > 0` 同士の IoU が ≥ 0.99 であることを確かめる（比較のため `FrameResult` とは別に、テスト用に low-res のロジットも取り出せるようにしてよい）。
`pyproject.toml` に `[tool.pytest.ini_options] markers = ["slow"]` と `addopts = "-m 'not slow'"` を足す。

- [ ] **Step 6: 実行** — Run: `cd worker && BENCH_PY=/path/to/bench.py uv run pytest -m slow tests/test_tracker_parity.py -v` / Expected: PASS（10 フレームすべて IoU ≥ 0.99）
- [ ] **Step 7: コミット** — `git commit -m "feat(worker): SAM2 の動画トラッカーを追加し bench.py との一致を検証"`

---

### Task 5: 合成とロゴのバッジ

**Files:**
- Create: `worker/cutout/compositor.py`, `worker/scripts/make_logo.py`, `worker/cutout/assets/logo.png`, `worker/tests/test_compositor.py`, `frontend/video-favicon.png`

**Interfaces:**
- Produces（`cutout.compositor`）:
  - `alpha_from_logits(logits_512: np.ndarray[512,512] float32, width: int, height: int) -> np.ndarray[H,W] float32` — バイリニア拡大 → sigmoid(16.0×ロジット)（2026-11-07 に変更。係数は 4 → 16。当初は erode あり。仕様 §6.3 参照）。
  - `composite(frame_rgb: np.ndarray[H,W,3] uint8, alpha: np.ndarray[H,W] | None, background_rgb: tuple[int,int,int]) -> np.ndarray[H,W,3] uint8` — `alpha=None` は見失ったフレーム（全面背景色）。
  - `LogoBadge.load(path: Path)`、`.apply(frame_rgb) -> np.ndarray` — 短辺の 8%（四捨五入、最小 16px）の正方形を、右下から短辺の 2% 内側に**不透明**で置く。
- `make_logo.py`: `assets/brand/logo-original.webp` を正方形にトリミングし、角丸（半径 = 一辺の 18%）の外側を透明にした 256×256 の PNG を `worker/cutout/assets/logo.png` に、32×32 を `frontend/video-favicon.png` に書く。角丸の外側だけ透明で、内側（`#171717` の背景を含む）は完全に不透明。

- [ ] **Step 1: 失敗するテストを書く**

```python
def test_alpha_is_probability_and_shrinks_edge():
    logits = np.full((512, 512), -20, np.float32); logits[:, 256:] = 20
    a = alpha_from_logits(logits, 100, 50)
    assert a.shape == (50, 100) and a.min() >= 0 and a.max() <= 1
    assert a[25, 99] > 0.99 and a[25, 0] < 0.01
    assert a[25, 50] < alpha_without_erode(logits, 100, 50)[25, 50]   # 縁が内側に削れている

def test_composite_lost_frame_is_full_background():
    out = composite(np.full((4, 4, 3), 200, np.uint8), None, (0, 255, 0))
    assert np.all(out == [0, 255, 0])

def test_composite_blends():
    out = composite(np.full((1, 1, 3), 100, np.uint8), np.array([[0.5]], np.float32), (0, 0, 0))
    assert out[0, 0, 0] == 50

def test_logo_badge_position_and_opacity():
    badge = LogoBadge.load(LOGO)
    out = badge.apply(np.zeros((1080, 1920, 3), np.uint8) + np.array([0, 255, 0], np.uint8))
    size, margin = round(1080 * 0.08), round(1080 * 0.02)
    cx, cy = 1920 - margin - size // 2, 1080 - margin - size // 2
    assert tuple(out[cy, cx]) != (0, 255, 0)                 # バッジの中心は背景色ではない
    assert tuple(out[1080 - margin - size - 2, cx]) == (0, 255, 0)   # バッジの外は元のまま
```
（`alpha_without_erode` はテスト内で erode を省いた同じ計算。）

- [ ] **Step 2: 失敗を確認** — Run: `cd worker && uv run pytest tests/test_compositor.py -v` / Expected: FAIL
- [ ] **Step 3: `make_logo.py` を書いて実行し、`compositor.py` を実装する** — Run: `cd worker && uv run python scripts/make_logo.py`（Pillow は dev 依存に追加）
- [ ] **Step 4: 成功を確認** — 同じテストで PASS
- [ ] **Step 5: コミット** — `git add worker frontend/video-favicon.png && git commit -m "feat(worker): マスクの合成とロゴのバッジを追加"`

---

### Task 6: ffmpeg による動画の読み書き

**Files:**
- Create: `worker/cutout/video_io.py`, `worker/tests/test_video_io.py`, `worker/tests/media.py`（`testsrc` でテスト動画を作るヘルパー）

**Interfaces:**
- Produces（`cutout.video_io`）:
  - `VideoInfo(duration_ms: int, fps: float, width: int, height: int, has_audio: bool, is_hdr: bool)`、`probe(path: Path) -> VideoInfo`（映像が無い・読めないなら `UnsupportedVideo` 例外）
  - `output_geometry(info: VideoInfo) -> tuple[int, int, float]` — 回転後の幅・高さを長辺 1080 以下・偶数に、fps を min(元, 30) に。
  - `FrameReader(path, start_ms, end_ms, width, height, fps, is_hdr)`：イテレータで `np.ndarray[H,W,3] uint8`（RGB）を返す。`-ss/-to`、`fps=`、HDR なら `zscale=t=linear,tonemap=hable,zscale=t=bt709:m=bt709:p=bt709`、`scale=W:H`。301 枚で止める。
  - `Mp4Writer(out_path, width, height, fps, audio_source: Path | None, start_ms, end_ms)`：`.write(frame_rgb)`、`.close()`。libx264 veryfast / CRF 18 / yuv420p / `+faststart`、音声は `-map 1:a:0?` で AAC。
- ffmpeg と ffprobe はサブプロセスで呼ぶ。エラー出力はログに残し、例外に含める。

- [ ] **Step 1: 失敗するテストを書く**（`media.py` が `ffmpeg -f lavfi -i testsrc=...` と `sine` で動画を作る）

```python
def test_60fps_is_capped_to_30_and_range_is_respected(tmp_path):
    src = make_video(tmp_path, fps=60, seconds=3, size=(1280, 720), audio=True)
    info = probe(src); w, h, fps = output_geometry(info)
    assert (w, h, fps) == (1280, 720, 30)
    frames = list(FrameReader(src, 500, 2500, w, h, fps, info.is_hdr))
    assert len(frames) == 60 and frames[0].shape == (720, 1280, 3)

def test_writer_keeps_audio_and_length(tmp_path):
    src = make_video(tmp_path, fps=30, seconds=3, size=(640, 360), audio=True)
    out = tmp_path / "out.mp4"
    wr = Mp4Writer(out, 640, 360, 30, src, 0, 2000)
    for f in FrameReader(src, 0, 2000, 640, 360, 30, False): wr.write(f)
    wr.close()
    o = probe(out)
    assert o.has_audio and abs(o.duration_ms - 2000) <= 100 and o.fps == 30

def test_no_audio_source_gives_video_only(tmp_path):            # Review Focus 2
    src = make_video(tmp_path, fps=30, seconds=1, size=(320, 240), audio=False)
    out = write_through(src, tmp_path)
    assert probe(out).has_audio is False

def test_odd_and_large_sizes_become_even_and_capped(tmp_path):  # Review Focus 3
    assert output_geometry(info_of(width=721, height=405, fps=30))[:2] == (720, 404)
    assert output_geometry(info_of(width=3840, height=2160, fps=30))[:2] == (1080 * 16 // 9 // 2 * 2, 1080)
    write_through(make_video(tmp_path, fps=30, seconds=1, size=(721, 405), audio=False), tmp_path)  # 例外なし

def test_rotation_metadata_gives_portrait(tmp_path):
    src = make_video(tmp_path, fps=30, seconds=1, size=(640, 360), audio=False, rotate=90)
    assert output_geometry(probe(src))[:2] == (360, 640)

def test_hlg_10bit_is_detected_and_tonemapped(tmp_path):
    src = make_video(tmp_path, fps=30, seconds=1, size=(640, 360), audio=False, hlg=True)
    info = probe(src); assert info.is_hdr
    f = next(iter(FrameReader(src, 0, 1000, 640, 360, 30, True)))
    assert f.dtype == np.uint8 and 20 < f.mean() < 235

def test_unreadable_file_raises_unsupported(tmp_path):
    bad = tmp_path / "bad.mp4"; bad.write_bytes(b"not a video")
    with pytest.raises(UnsupportedVideo): probe(bad)
```
（`write_through`, `info_of` はテスト内ヘルパー。）

- [ ] **Step 2: 失敗を確認** — Run: `cd worker && uv run pytest tests/test_video_io.py -v` / Expected: FAIL（ffmpeg に zscale が無い環境では HDR のテストを `pytest.skip`）
- [ ] **Step 3: `video_io.py` を実装する**
- [ ] **Step 4: 成功を確認** — 同じコマンドで PASS
- [ ] **Step 5: コミット** — `git commit -m "feat(worker): ffmpeg による動画の読み込み・書き出しを追加"`

---

### Task 7: 通知（HMAC 署名・間引き・再送）

**Files:**
- Create: `worker/cutout/notifier.py`, `worker/tests/test_notifier.py`

**Interfaces:**
- Produces（`cutout.notifier`）:
  - `sign(secret: bytes, timestamp: int, body: bytes) -> str`（hex）
  - `Notifier(callback_url: str, secret: bytes, post: Callable[[str, bytes, dict], int], clock: Callable[[], float], sleep: Callable[[float], None])`
  - `.progress(event: dict)` — 前回送信から 0.5 秒未満なら送らない。失敗しても例外を出さない。
  - `.final(event: dict) -> bool` — 1・2・4 秒の間隔で最大 3 回再送（初回を含めて 4 回）。2xx が返れば True。
- ヘッダーは Global Constraints の HMAC のとおり。本文は `json.dumps(event, separators=(",", ":")).encode()`。

- [ ] **Step 1: 失敗するテストを書く**

```python
def test_sign_matches_known_vector():
    assert sign(b"secret", 1700000000, b'{"a":1}') == hmac.new(b"secret", b'1700000000.{"a":1}', "sha256").hexdigest()

def test_progress_is_throttled_to_half_second():
    sent, t = [], [0.0]
    n = Notifier("u", b"s", lambda u, b, h: sent.append(b) or 200, lambda: t[0], lambda s: None)
    n.progress({"p": 1}); t[0] = 0.3; n.progress({"p": 2}); t[0] = 0.6; n.progress({"p": 3})
    assert len(sent) == 2

def test_progress_failure_is_swallowed():
    n = Notifier("u", b"s", lambda *a: (_ for _ in ()).throw(OSError()), lambda: 0.0, lambda s: None)
    n.progress({"p": 1})

def test_final_retries_with_backoff():
    codes, waits = iter([500, 500, 500, 204]), []
    n = Notifier("u", b"s", lambda *a: next(codes), lambda: 0.0, waits.append)
    assert n.final({"phase": "done"}) is True and waits == [1, 2, 4]

def test_headers_contain_timestamp_and_signature():
    got = {}
    n = Notifier("u", b"s", lambda u, b, h: got.update(h=h, b=b) or 200, lambda: 1700000000.4, lambda s: None)
    n.final({"phase": "done"})
    assert got["h"]["X-Signature-Timestamp"] == "1700000000"
    assert got["h"]["X-Signature"] == sign(b"s", 1700000000, got["b"])
```

- [ ] **Step 2: 失敗を確認** — Run: `cd worker && uv run pytest tests/test_notifier.py -v` / Expected: FAIL
- [ ] **Step 3: `notifier.py` を実装する**（`post` の本番実装は `httpx.post(url, content=body, headers=h, timeout=10).status_code`）
- [ ] **Step 4: 成功を確認** — 同じコマンドで PASS
- [ ] **Step 5: コミット** — `git commit -m "feat(worker): HMAC 署名付きの進捗通知を追加"`

---

### Task 8: `run_job`（ダウンロード → 処理 → アップロード → 通知）

**Files:**
- Create: `worker/cutout/pipeline.py`, `worker/tests/test_pipeline.py`

**Interfaces:**
- Consumes: Task 2〜7 のすべて
- Produces（`cutout.pipeline`）:
  - `Transfer` Protocol: `head_size(url) -> int | None`、`download(url, dest: Path)`、`upload(url, src: Path, content_type: str)`
  - `run_job(req: WorkerJobRequest, transfer: Transfer, notifier: Notifier, make_tracker: Callable[[int], Sam2VideoTracker], logo: LogoBadge, workdir: Path) -> None` — 例外を外に出さない。最後に必ず `notifier.final(done または failed)` を呼び、`workdir` を消す。
- 失敗コードの対応: HEAD のサイズ > `max_input_bytes` → `too_large`、`UnsupportedVideo` → `unsupported_video`、`start_ms ≥ 長さ` → `range_out_of_bounds`、1 枚目で `object_present=False` → `subject_not_found`、それ以外の例外 → `internal_error`。
- `end_ms > 長さ` のときは `end_ms = 長さ` に切り詰めて続行する（Review Focus 1）。
- 進捗: ダウンロード開始で `downloading`、フレームごとに `tracking(i/total)`、エンコーダを閉じる前に `finishing`、アップロード後に `done(frames, lost_frames, duration_ms)`。`total` は `min(301, round((end-start)/1000*fps))`。

- [ ] **Step 1: 失敗するテストを書く**（`FakeTransfer` はローカルファイルを返し、アップロード先をテスト用ディレクトリに置く。`FakeTracker` は決まったロジットを返す。`notifier` は送った event を記録する偽物）

```python
def test_happy_path_emits_phases_and_uploads(tmp_path):
    ev, tr = run(tmp_path, video=make_video(tmp_path, fps=30, seconds=2, size=(320, 240), audio=True), range_ms=(0, 1000))
    assert [e["phase"] for e in ev][0] == "downloading" and ev[-1] == {"job_id": JOB, "phase": "done", "frames": 30, "lost_frames": 0, "duration_ms": 1000}
    assert "finishing" in [e["phase"] for e in ev] and probe(tr.uploaded).has_audio

def test_too_large(tmp_path):
    assert final(run(tmp_path, head_size=300 * 1024 * 1024)) == ("failed", "too_large")

def test_unsupported(tmp_path):
    assert final(run(tmp_path, video=bytes_file(tmp_path, b"xx"))) == ("failed", "unsupported_video")

def test_start_beyond_duration(tmp_path):
    assert final(run(tmp_path, range_ms=(5000, 6000), seconds=2)) == ("failed", "range_out_of_bounds")

def test_end_slightly_beyond_duration_is_clamped(tmp_path):      # Review Focus 1
    ev, _ = run(tmp_path, range_ms=(1000, 2040), seconds=2)
    assert ev[-1]["phase"] == "done" and abs(ev[-1]["duration_ms"] - 1000) <= 40

def test_subject_not_found_on_first_frame(tmp_path):
    assert final(run(tmp_path, first_present=False)) == ("failed", "subject_not_found")

def test_unexpected_exception_is_internal_error_and_workdir_removed(tmp_path):
    ev, _ = run(tmp_path, tracker_raises=True)
    assert final((ev, None)) == ("failed", "internal_error") and not (tmp_path / "work").exists()

def test_lost_frames_are_counted(tmp_path):
    ev, _ = run(tmp_path, present_pattern=[True, False, True], range_ms=(0, 100), seconds=1)
    assert ev[-1]["lost_frames"] == 1
```

- [ ] **Step 2: 失敗を確認** — Run: `cd worker && uv run pytest tests/test_pipeline.py -v` / Expected: FAIL
- [ ] **Step 3: `pipeline.py` を実装する**（デコード → 推論 → 合成 → エンコードは 1 フレームずつ流す。全フレームを溜めない）
- [ ] **Step 4: 成功を確認** — Run: `cd worker && uv run pytest -v` / Expected: 全 PASS
- [ ] **Step 5: コミット** — `git commit -m "feat(worker): ジョブ全体の処理（run_job）と失敗コードの対応を追加"`

---

### Task 9: Modal のアダプタとデプロイ

オーナーの作業が必要: Modal の Secret `goodbye-background-video`（キー `WORKER_CALLBACK_SECRET`）を作る。プロキシ認証用のトークン（`MODAL_KEY` / `MODAL_SECRET`）を発行する。

**Files:**
- Create: `worker/modal_app.py`, `worker/tests/test_modal_app.py`, `worker/README.md`

**Interfaces:**
- Consumes: `cutout.pipeline.run_job`, `cutout.contract.WorkerJobRequest`, `cutout.models`
- Produces: Web エンドポイント `POST /`（プロキシ認証必須）。本文は `WorkerJobRequest` の JSON。正しければ `process.spawn(body)` して **202** `{"accepted": true}`、契約違反なら 400。Rust の `MODAL_ENDPOINT_URL` はこの URL。
- 関数 `process`: GPU=`L4`、`timeout=300`、`retries=0`、`scaledown_window=60`、`@modal.enter` で `load_sessions(..., "cuda")` と `LogoBadge.load`。
- アダプタの中で行う依存の組み立て: `HttpTransfer`（httpx）、`Notifier(post=httpx の実装, clock=time.time, sleep=time.sleep)`、`workdir=/tmp/{job_id}`。
- 受け付け処理は `accept(body: dict, spawn: Callable[[dict], None]) -> tuple[int, dict]` という純粋な関数に切り出し、テストする（Modal を import せずにテストできるように、`modal_app.py` の中で Modal に依存しない部分を `cutout/accept.py` に置いてもよい）。

**引き継ぎ事項（実装前に必ず反映）:**
- (a) `Transfer.head_size` で、GET 用に署名された入力 URL へ HEAD を送らない（SigV4 はメソッドも署名するので 403 になる）。`GET` に `Range: bytes=0-0` を付けて `Content-Range` の全体サイズを読むか、403 なら `None` を返す。
- (b) `download` はストリームで読み、`max_input_bytes` を超えた時点で止めて `too_large` にする（全部読んでから確かめない）。
- (c) 署名付き URL をログに出さない。httpx の例外は URL を含むので、ログにはステータスコードと例外の型だけを出す。
- (d) `Notifier` の時計は `time.time`（UNIX 秒。署名のタイムスタンプに使うため。`time.monotonic` ではない）。
- (e) 本番の前に、一致の遅いテスト（IoU ≥ 0.99）を実行し、`MODEL_REVISION="main"` をコミットのハッシュに固定する（開発のセッションでは huggingface.co に接続できず未実施）。

- [ ] **Step 1: 失敗するテストを書く**

```python
def test_accept_valid_request_spawns_and_returns_202():
    spawned = []
    assert accept(example_request(), spawned.append) == (202, {"accepted": True}) and len(spawned) == 1

def test_accept_invalid_request_returns_400_without_spawn():
    spawned = []
    status, _ = accept({**example_request(), "points": []}, spawned.append)
    assert status == 400 and spawned == []
```

- [ ] **Step 2: 失敗を確認 → 実装 → 成功を確認** — Run: `cd worker && uv run pytest tests/test_modal_app.py -v`
- [ ] **Step 3: デプロイして疎通を確認する**

Run: `cd worker && modal deploy modal_app.py`
Run: 認証ヘッダー無しで `curl -X POST <URL>` → Expected: 401/403。正しいヘッダーと契約違反の本文 → 400。
- [ ] **Step 4: `worker/README.md` に、デプロイの方法・Secret の名前・計測の結果へのリンクを書いてコミット** — `git commit -m "feat(worker): Modal のアダプタを追加"`

---

### Task 10: Rust のドメインの値・時計・エラー分類

**Files:**
- Create: `src/video_cutout/mod.rs`, `clock.rs`, `clock_impl.rs`, `job/{mod,id,clip_range,point,background,phase,video_job,error}.rs`
- Modify: `src/main.rs`（`mod video_cutout;`）、`src/common/traits.rs`（`ErrorType` に `Unauthorized`, `TooManyRequests`, `Unavailable` を追加）、`src/presentation/errors/app_error.rs`（401 / 429 / 503 の対応）、`Cargo.toml`（`async-trait`, `rusty-s3`, `hmac`, `sha2`, `hex` を追加）

**Interfaces:**
- Produces:
  - `VideoClock: Send + Sync { fn now_ms(&self) -> u64 }`（UNIX ミリ秒）、`RealVideoClock`、`FakeVideoClock::new(ms)` + `.advance(ms: u64)`（`AtomicU64`）
  - `VideoJobId(Uuid)`: `new_random()`, `from_str(&str) -> Result<_, VideoJobError>`, `value()`
  - `ClipRange::new(start_ms: u32, end_ms: u32) -> Result<Self, VideoJobError>`、`MAX_CLIP_MS: u32 = 10_000`
  - `NormalizedPoint::new(x: f32, y: f32, label: PointLabel) -> Result<..>`、`PointLabel::{Include, Exclude}`、`Points::new(Vec<NormalizedPoint>) -> Result<..>`（1〜8）
  - `Background::{Green, Blue, Magenta}`、`.rgb() -> [u8; 3]`、`Background::from_str`
  - `FailureCode::{DispatchFailed, Timeout, InternalError, SubjectNotFound, UnsupportedVideo, RangeOutOfBounds, TooLarge}`、`.as_str()`（仕様 §5.1 の小文字の名前）、`.refunds_quota() -> bool`、`FailureCode::from_worker(&str) -> Option<Self>`
  - `JobPhase::{Created, Queued, Downloading, Tracking(f32), Finishing, Done, Failed(FailureCode)}`、`.is_terminal()`、`.advance(next: JobPhase) -> Option<JobPhase>`（前に進むときだけ `Some`。`Tracking` 同士は進捗が増えるときだけ。最終状態からは常に `None`）
  - `VideoJob`（仕様 §4.3 のフィールド。`owner_client_id: Uuid`, `owner_ip: IpAddr`, `size_bytes: u64`, `content_type: String`, `range: ClipRange`, `background: Option<Background>`, `points: Option<Points>`, `phase: JobPhase`, `created_at_ms`, `last_event_at_ms`, `quota_consumed: bool`, `quota_refunded: bool`, `result: Option<JobResult { frames, lost_frames, duration_ms }>`）、`input_key()`, `output_key()`、`deadline_ms() -> u64`（仕様 §5.2）
  - `VideoJobError`（`leaf_error!`）: `InvalidId => ("VIDEO_JOB_INVALID_ID", InvalidInput)`, `InvalidRange => ("INVALID_RANGE", InvalidInput)`, `InvalidPoints => ("INVALID_POINTS", InvalidInput)`, `InvalidBackground => ("INVALID_BACKGROUND", InvalidInput)`, `NotFound => ("VIDEO_JOB_NOT_FOUND", NotFound)`, `InvalidState => ("VIDEO_JOB_INVALID_STATE", Conflict)`

- [ ] **Step 1: 失敗するテストを書く**（各ファイル末尾の `#[cfg(test)] mod xxx_test`）

```rust
#[test] fn clip_range_accepts_exactly_10s() { assert!(ClipRange::new(0, 10_000).is_ok()); }
#[test] fn clip_range_rejects_over_10s_and_reversed() {
    assert_eq!(ClipRange::new(0, 10_001), Err(VideoJobError::InvalidRange));
    assert_eq!(ClipRange::new(500, 500), Err(VideoJobError::InvalidRange));
}
#[test] fn points_bounds() {
    assert!(NormalizedPoint::new(1.01, 0.5, PointLabel::Include).is_err());
    assert!(Points::new(vec![]).is_err());
    assert!(Points::new(vec![p(); 9]).is_err());
}
#[test] fn phase_only_moves_forward() {
    assert_eq!(JobPhase::Tracking(0.5).advance(JobPhase::Tracking(0.4)), None);
    assert_eq!(JobPhase::Tracking(0.5).advance(JobPhase::Tracking(0.6)), Some(JobPhase::Tracking(0.6)));
    assert_eq!(JobPhase::Finishing.advance(JobPhase::Downloading), None);
    assert_eq!(JobPhase::Done.advance(JobPhase::Failed(FailureCode::Timeout)), None);
    assert_eq!(JobPhase::Queued.advance(JobPhase::Failed(FailureCode::InternalError)), Some(JobPhase::Failed(FailureCode::InternalError)));
}
#[test] fn refund_table_matches_spec() {
    use FailureCode::*;
    for c in [DispatchFailed, Timeout, InternalError, SubjectNotFound, UnsupportedVideo] { assert!(c.refunds_quota()); }
    for c in [RangeOutOfBounds, TooLarge] { assert!(!c.refunds_quota()); }
}
#[test] fn deadlines() {
    let j = job_created_at(1_000);
    assert_eq!(j.deadline_ms(), 1_000 + 30 * 60_000);
    assert_eq!(with_phase(j.clone(), JobPhase::Queued, 2_000).deadline_ms(), 2_000 + 5 * 60_000);
    assert_eq!(with_phase(j.clone(), JobPhase::Tracking(0.1), 3_000).deadline_ms(), 3_000 + 3 * 60_000);
    assert_eq!(with_phase(j, JobPhase::Done, 4_000).deadline_ms(), 4_000 + 30 * 60_000);
}
#[test] fn fake_clock_advances() { let c = FakeVideoClock::new(5); c.advance(10); assert_eq!(c.now_ms(), 15); }
```
（`with_phase(job, phase, at_ms)` は `phase` と `last_event_at_ms` を設定するテスト用ヘルパー。）

- [ ] **Step 2: 失敗を確認** — Run: `cargo test video_cutout::job` / Expected: コンパイルエラーで FAIL
- [ ] **Step 3: 実装する**（エラーは `leaf_error!`。`ErrorType` の追加に合わせて `app_error.rs` の `match` を 401/429/503 に対応）
- [ ] **Step 4: 成功を確認** — Run: `cargo test` / Expected: 既存も含め全 PASS
- [ ] **Step 5: コミット** — `git commit -m "feat(video): ジョブのドメインの値と時計を追加"`

---

### Task 11: ジョブのリポジトリとロック

**Files:**
- Create: `src/video_cutout/job/repository.rs`, `repository_impl.rs`, `locks.rs`

**Interfaces:**
- Produces:
  - `VideoJobRepository: Send + Sync { fn get(&self, id: &VideoJobId) -> Option<Arc<VideoJob>>; fn save(&self, job: VideoJob); fn remove(&self, id: &VideoJobId); fn list_expired(&self, now_ms: u64) -> Vec<Arc<VideoJob>>; }`
  - `VideoJobRepositoryInMemory::new()` — `DashMap<VideoJobId, Arc<VideoJob>>`。期限は `VideoJob::deadline_ms()` で判断し、`TtlStore` の最終アクセス基準は使わない（仕様 §4.4）。
  - `VideoJobLocks::new()`、`.lock_for(&VideoJobId) -> Arc<tokio::sync::Mutex<()>>`、`.remove(&VideoJobId)`

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[test] fn save_get_remove() { let r = VideoJobRepositoryInMemory::new(); let j = job(); r.save(j.clone()); assert_eq!(*r.get(&j.id).unwrap(), j); r.remove(&j.id); assert!(r.get(&j.id).is_none()); }
#[test] fn list_expired_uses_deadline() {
    let r = VideoJobRepositoryInMemory::new(); r.save(job_created_at(0)); r.save(job_created_at(10 * 60_000));
    assert_eq!(r.list_expired(30 * 60_000 + 1).len(), 1);
}
#[tokio::test] async fn same_id_shares_lock() {
    let l = VideoJobLocks::new(); let id = VideoJobId::new_random();
    let _g = l.lock_for(&id).lock_owned().await;
    assert!(l.lock_for(&id).try_lock().is_err());
}
```

- [ ] **Step 2: 失敗を確認 → 実装 → 成功を確認** — Run: `cargo test video_cutout::job`
- [ ] **Step 3: コミット** — `git commit -m "feat(video): ジョブのリポジトリとジョブ単位のロックを追加"`

---

### Task 12: 1 日の回数制限（DB）

**Files:**
- Create: `migrations/sqlite/20261106000000_video_quota_usage.sql`, `migrations/postgres/20261106000000_video_quota_usage.sql`
- Create: `src/video_cutout/quota/{mod,daily_quota,daily_quota_sqlite,daily_quota_postgres,jst_day,error}.rs`

**Interfaces:**
- Produces:
  - `jst_day(now_ms: u64) -> i64` = `(now_ms / 1000 + 9 * 3600) / 86400`
  - `QuotaKeys { client_id: Uuid, ip: IpAddr }`、`DAILY_LIMIT: i64 = 3`
  - `#[async_trait] DailyQuota: Send + Sync { async fn remaining(&self, keys: &QuotaKeys, day: i64) -> Result<u8, QuotaError>; async fn try_consume(&self, keys: &QuotaKeys, day: i64) -> Result<u8, QuotaError>; async fn refund(&self, keys: &QuotaKeys, day: i64) -> Result<(), QuotaError>; }` — 戻り値の `u8` は消費後の残り回数。
  - `SqliteDailyQuota::new(SqlitePool)`、`PostgresDailyQuota::new(PgPool)`
  - `QuotaError`（`leaf_detail_error!`）: `Exceeded => ("QUOTA_EXCEEDED", TooManyRequests)`, `Database => ("QUOTA_DB_ERROR", Internal)`
- テーブル: `video_quota_usage (day INTEGER NOT NULL, key_kind TEXT NOT NULL, key TEXT NOT NULL, count INTEGER NOT NULL, PRIMARY KEY (day, key_kind, key))`。`key_kind` は `client` / `ip`。
- `try_consume`: 1 トランザクションで 2 行の count を読み、どちらかが 3 以上なら `Exceeded`。そうでなければ両方 UPSERT で +1、ついでに `day < 当日 - 7` の行を削除。`refund`: 両方を `MAX(count - 1, 0)`。

- [ ] **Step 1: 失敗するテストを書く**（SQLite のインメモリ DB にマイグレーションを当てて使う。`recorder.rs` のテストと同じ作り）

```rust
#[test] fn jst_day_rolls_at_15utc() {
    let d = jst_day(1_700_000_000_000);
    let midnight_jst_ms = ((d * 86400 - 9 * 3600) * 1000) as u64;
    assert_eq!(jst_day(midnight_jst_ms - 1), d - 1); assert_eq!(jst_day(midnight_jst_ms), d);
}
#[tokio::test] async fn three_per_day_then_exceeded() {
    let q = quota().await; let k = keys("10.0.0.1");
    assert_eq!(q.try_consume(&k, 100).await, Ok(2)); q.try_consume(&k, 100).await.unwrap(); q.try_consume(&k, 100).await.unwrap();
    assert_eq!(q.try_consume(&k, 100).await, Err(QuotaError::Exceeded));
    assert_eq!(q.try_consume(&k, 101).await, Ok(2));
}
#[tokio::test] async fn either_key_blocks() {
    let q = quota().await;
    for i in 0..3 { q.try_consume(&keys_with(client(i), "10.0.0.1"), 100).await.unwrap(); }
    assert_eq!(q.try_consume(&keys_with(client(9), "10.0.0.1"), 100).await, Err(QuotaError::Exceeded));
    assert_eq!(q.remaining(&keys_with(client(9), "10.0.0.2"), 100).await, Ok(3));
}
#[tokio::test] async fn refund_restores_and_never_negative() {
    let q = quota().await; let k = keys("10.0.0.1");
    q.try_consume(&k, 100).await.unwrap(); q.refund(&k, 100).await.unwrap(); q.refund(&k, 100).await.unwrap();
    assert_eq!(q.remaining(&k, 100).await, Ok(3));
}
```

- [ ] **Step 2: 失敗を確認 → 実装 → 成功を確認** — Run: `cargo test video_cutout::quota`
- [ ] **Step 3: PostgreSQL 版は SQL の方言（`$1`、`GREATEST`）だけを変えた同じ実装にし、`cargo check` が通ることを確認する**（Postgres に繋ぐテストは書かない。既存の `recorder.rs` と同じ扱い）
- [ ] **Step 4: コミット** — `git add migrations src/video_cutout/quota && git commit -m "feat(video): 1日の回数制限を DB に保存する DailyQuota を追加"`

---

### Task 13: 外部へのポート（R2・ワーカー）、HMAC、作成と開始のユースケース

**Files:**
- Create: `src/video_cutout/storage/{mod,video_object_storage,r2_video_object_storage,error}.rs`
- Create: `src/video_cutout/worker/{mod,cutout_worker,modal_cutout_worker,request,error}.rs`
- Create: `src/video_cutout/signature.rs`, `src/video_cutout/testing.rs`
- Create: `src/video_cutout/{create_job,start_job}/{mod,input,output,error,usecase}.rs`
- Modify: `src/application/error.rs`（`CreateVideoJobUseCase`, `StartVideoJobUseCase` の variant）

**Interfaces:**
- Consumes: Task 10〜12
- Produces:
  - `#[async_trait] VideoObjectStorage: Send + Sync { fn presign_put(&self, key: &str, content_type: &str, ttl: Duration) -> String; fn presign_get(&self, key: &str, ttl: Duration, download_filename: Option<&str>) -> String; async fn head(&self, key: &str) -> Result<Option<u64>, StorageError>; async fn delete(&self, key: &str) -> Result<(), StorageError>; }`
  - `R2VideoObjectStorage::new(account_id, bucket, access_key_id, secret_access_key)` — エンドポイント `https://{account_id}.r2.cloudflarestorage.com`、リージョン `auto`、パス形式。`presign_get` のファイル名は `response-content-disposition=attachment; filename="..."`。
  - `#[async_trait] CutoutWorker: Send + Sync { async fn dispatch(&self, req: &WorkerJobRequest) -> Result<(), WorkerError>; }`、`ModalCutoutWorker::new(endpoint_url, modal_key, modal_secret)`（`Modal-Key` / `Modal-Secret` ヘッダー、タイムアウト 30 秒、2xx 以外は `WorkerError::Dispatch`）
  - `WorkerJobRequest`（`serde::Serialize`。フィールドは契約の JSON と同じ名前）
  - `signature::sign(secret: &[u8], timestamp: u64, body: &[u8]) -> String`、`signature::verify(secret, timestamp_header: &str, signature_header: &str, body: &[u8], now_s: u64) -> Result<(), SignatureError>`（`SignatureError::Invalid => ("INVALID_SIGNATURE", Unauthorized)`）
  - `CreateVideoJobInput { client_id: Uuid, ip: IpAddr, size_bytes: u64, content_type: String, start_ms: u32, end_ms: u32 }` → `CreateVideoJobOutput { job_id: VideoJobId, upload_url: String, remaining_today: u8 }`
    - エラー: 末端 `CreateError`（`leaf_error!`）`TooLarge => ("TOO_LARGE", InvalidInput)`, `UnsupportedContentType => ("UNSUPPORTED_CONTENT_TYPE", InvalidInput)`。
      親 `CreateVideoJobError`（`parent_error!`）= `Create(CreateError)`, `Job(VideoJobError)`, `Quota(QuotaError)`
  - `StartVideoJobInput { client_id, ip, job_id: VideoJobId, points: Points, background: Background }` → `StartVideoJobOutput { remaining_today: u8 }`
    - エラー: 末端 `StartError`（`leaf_error!`）`UploadNotFound => ("UPLOAD_NOT_FOUND", Conflict)`。
      親 `StartVideoJobError` = `Start(StartError)`, `Job(VideoJobError)`, `Quota(QuotaError)`, `Storage(StorageError)`。
      `StorageError`（`leaf_detail_error!`）= `Request => ("STORAGE_FAILED", Internal)`。`WorkerError`（`leaf_detail_error!`）= `Dispatch => ("WORKER_DISPATCH_FAILED", Internal)`（`StartVideoJob` 内で `Failed(DispatchFailed)` に変換するので HTTP には出ない）
    - 処理順は仕様 §4.5。HEAD のサイズが 200MB 超なら回数を消費せず `Failed(TooLarge)` にして `Ok` を返す。投入失敗は `refund` して `Failed(DispatchFailed)`、`quota_refunded=true` にして `Ok` を返す。
    - `callback_url` = `{PUBLIC_BASE_URL}/internal/video-jobs/{id}/events`。ワーカー用 URL は 30 分。
  - `testing.rs`: `FakeStorage`（`head` の戻り値を設定でき、`deleted: Mutex<Vec<String>>` を記録）、`FakeWorker`（`dispatched: Mutex<Vec<WorkerJobRequest>>`、`fail: bool`、`delay: Option<Duration>`）、`InMemoryQuota`（`DailyQuota` の HashMap 実装）

- [ ] **Step 1: HMAC と契約のテストを書く**

```rust
#[test] fn sign_known_vector() { assert_eq!(sign(b"secret", 1_700_000_000, br#"{"a":1}"#), hex::encode(hmac_sha256(b"secret", br#"1700000000.{"a":1}"#))); }
#[test] fn verify_rejects_tamper_and_skew() {
    let s = sign(b"k", 1000, b"body");
    assert!(verify(b"k", "1000", &s, b"body", 1000).is_ok());
    assert!(verify(b"k", "1000", &s, b"bodx", 1000).is_err());
    assert!(verify(b"k", "1000", &s, b"body", 1301).is_err());
    assert!(verify(b"k", "1000", &s, b"body", 699).is_err());
}
#[test] fn worker_request_matches_contract_keys() {
    let example: serde_json::Value = serde_json::from_str(include_str!("../../../contracts/video_cutout/worker_job_request.json")).unwrap();
    let ours = serde_json::to_value(sample_request()).unwrap();
    assert_eq!(keys_recursive(&ours), keys_recursive(&example));
}
#[test] fn r2_presigned_put_targets_bucket_and_key() {
    let url = r2().presign_put("video-jobs/x/input", "video/mp4", Duration::from_secs(900));
    assert!(url.starts_with("https://acc.r2.cloudflarestorage.com/bucket/video-jobs/x/input?") && url.contains("X-Amz-Expires=900"));
}
```
（`serde_json` が無ければ dev 依存に追加。）

- [ ] **Step 2: ユースケースのテストを書く**

```rust
#[tokio::test] async fn create_checks_quota_without_consuming() {
    let h = harness(); let out = h.create.execute(create_input()).await.unwrap();
    assert_eq!(out.remaining_today, 3); assert!(out.upload_url.contains("X-Amz-Expires=900"));
}
#[tokio::test] async fn create_rejects_size_and_type() {
    let h = harness();
    assert!(matches!(h.create.execute(CreateVideoJobInput { size_bytes: 209_715_201, ..create_input() }).await, Err(CreateVideoJobError::Create(CreateError::TooLarge))));
    assert!(matches!(h.create.execute(CreateVideoJobInput { content_type: "video/webm".into(), ..create_input() }).await, Err(CreateVideoJobError::Create(CreateError::UnsupportedContentType))));
}
#[tokio::test] async fn start_dispatches_once_under_concurrency() {
    let h = harness_with(FakeWorker::delayed(Duration::from_millis(50))); let id = h.created_job().await;
    let (a, b) = tokio::join!(h.start.execute(start_input(id)), h.start.execute(start_input(id)));
    assert_eq!([a.is_ok(), b.is_ok()].iter().filter(|x| **x).count(), 1);
    assert_eq!(h.worker.dispatched.lock().unwrap().len(), 1);
}
#[tokio::test] async fn start_before_upload_is_conflict_and_keeps_quota() {   // Review Focus 4
    let h = harness_with_head(None); let id = h.created_job().await;
    assert!(matches!(h.start.execute(start_input(id)).await, Err(StartVideoJobError::Start(StartError::UploadNotFound))));
    assert_eq!(h.remaining().await, 3); assert_eq!(h.phase(id), JobPhase::Created);
}
#[tokio::test] async fn second_job_exceeds_quota_and_stays_created() {      // Review Focus 5
    let h = harness_with_used(2); let a = h.created_job().await; let b = h.created_job().await;
    h.start.execute(start_input(a)).await.unwrap();
    assert!(matches!(h.start.execute(start_input(b)).await, Err(StartVideoJobError::Quota(QuotaError::Exceeded))));
    assert_eq!(h.phase(b), JobPhase::Created);
}
#[tokio::test] async fn dispatch_failure_refunds() {
    let h = harness_with(FakeWorker::failing()); let id = h.created_job().await;
    h.start.execute(start_input(id)).await.unwrap();
    assert_eq!(h.phase(id), JobPhase::Failed(FailureCode::DispatchFailed)); assert_eq!(h.remaining().await, 3);
}
#[tokio::test] async fn other_client_gets_not_found() {
    let h = harness(); let id = h.created_job().await;
    assert!(matches!(h.start.execute(StartVideoJobInput { client_id: Uuid::new_v4(), ..start_input(id) }).await, Err(StartVideoJobError::Job(VideoJobError::NotFound))));
}
#[tokio::test] async fn oversized_upload_fails_without_consuming() {
    let h = harness_with_head(Some(209_715_201)); let id = h.created_job().await;
    h.start.execute(start_input(id)).await.unwrap();
    assert_eq!(h.phase(id), JobPhase::Failed(FailureCode::TooLarge)); assert_eq!(h.remaining().await, 3);
}
```

- [ ] **Step 3: 失敗を確認** — Run: `cargo test video_cutout` / Expected: FAIL
- [ ] **Step 4: 実装する**
- [ ] **Step 5: 成功を確認** — Run: `cargo test` / Expected: 全 PASS
- [ ] **Step 6: コミット** — `git commit -m "feat(video): R2・ワーカーのポートと、ジョブの作成・開始のユースケースを追加"`

---

### Task 14: 状態の取得・通知の反映・期限切れの削除

**Files:**
- Create: `src/video_cutout/{get_job,report_event,delete_expired_jobs}/{mod,input,output,error,usecase}.rs`
- Modify: `src/application/error.rs`（3 つの variant）

**Interfaces:**
- Consumes: Task 10〜13
- Produces:
  - `GetVideoJobInput { client_id, ip, job_id }` → `GetVideoJobOutput { phase: JobPhase, failure: Option<FailureCode>, quota_refunded: bool, download_url: Option<String>, remaining_today: u8 }`。ロック内で期限切れなら `Failed(Timeout)` にして返却。`Done` なら 10 分の GET URL（ファイル名 `cutout-{job_id の先頭 8 文字}.mp4`）。
  - `WorkerEvent`（`serde::Deserialize`、`#[serde(tag = "phase", rename_all = "lowercase")]`: `Downloading`, `Tracking { progress: f32 }`, `Finishing`, `Done { frames: u32, lost_frames: u32, duration_ms: u32 }`, `Failed { code: String }`）＋ `job_id: String`
  - `ReportVideoJobEventInput { job_id: VideoJobId, event: WorkerEvent }` → `()`。`advance` が `None` なら何もしない。最終状態になったら入力ファイルを**await して**削除（失敗は `eprintln!` だけ）。`Failed` で `refunds_quota()` なら `refund` して `quota_refunded=true`。未知の `code` は `InternalError` として扱う。
  - `DeleteExpiredVideoJobsUseCase::execute(now_ms) -> usize`（処理した件数）。期限切れのうち、進行中（`Queued`〜`Finishing`）は `Failed(Timeout)` + 返却にして保存し直す（30 分後にもう一度期限が来る）。`Created`・`Done`・`Failed` は入出力を削除してストアとロックから消す。
  - `VideoCleanupThrottle::should_run(now_ms) -> bool`（最後に `true` を返してから 60,000ms 経っていれば `true`。`AtomicU64`）

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[tokio::test] async fn tracking_progress_and_done_gives_download_url() {
    let h = started_job().await;
    h.report(json!({"job_id": h.id_str(), "phase": "tracking", "progress": 0.5})).await;
    assert_eq!(h.get().await.phase, JobPhase::Tracking(0.5));
    h.report(json!({"job_id": h.id_str(), "phase": "done", "frames": 30, "lost_frames": 0, "duration_ms": 1000})).await;
    let out = h.get().await;
    assert_eq!(out.phase, JobPhase::Done); assert!(out.download_url.unwrap().contains("output.mp4"));
    assert_eq!(h.storage.deleted(), vec![h.input_key()]);
}
#[tokio::test] async fn out_of_order_and_late_events_are_ignored() {
    let h = started_job().await;
    h.report(tracking(0.6)).await; h.report(tracking(0.4)).await; assert_eq!(h.get().await.phase, JobPhase::Tracking(0.6));
    h.report(failed("internal_error")).await; h.report(done()).await;
    assert_eq!(h.get().await.phase, JobPhase::Failed(FailureCode::InternalError));
}
#[tokio::test] async fn failure_refund_follows_table() {
    for (code, refunded) in [("unsupported_video", true), ("subject_not_found", true), ("range_out_of_bounds", false)] {
        let h = started_job().await; h.report(failed(code)).await;
        assert_eq!((h.get().await.quota_refunded, h.remaining().await == 3), (refunded, refunded));
    }
}
#[tokio::test] async fn get_times_out_queued_after_5min() {
    let h = started_job().await; h.clock.advance(5 * 60_000 + 1);
    let out = h.get().await; assert_eq!(out.phase, JobPhase::Failed(FailureCode::Timeout)); assert!(out.quota_refunded);
}
#[tokio::test] async fn cleanup_deletes_files_of_finished_jobs_after_30min() {
    let h = started_job().await; h.report(done()).await; h.clock.advance(30 * 60_000 + 1);
    assert_eq!(h.cleanup().await, 1);
    assert!(h.storage.deleted().contains(&h.output_key())); assert!(h.repo.get(&h.id).is_none());
}
#[test] fn throttle_runs_once_per_minute() {
    let t = VideoCleanupThrottle::new(); assert!(t.should_run(0)); assert!(!t.should_run(59_999)); assert!(t.should_run(60_000));
}
```

- [ ] **Step 2: 失敗を確認 → 実装 → 成功を確認** — Run: `cargo test video_cutout`
- [ ] **Step 3: コミット** — `git commit -m "feat(video): ジョブ状態の取得・ワーカー通知の反映・期限切れの削除を追加"`

---

### Task 15: 配線・設定・ハンドラ・ルート・IP・利用ログ

**Files:**
- Create: `src/video_cutout/config.rs`, `src/presentation/client_ip.rs`
- Create: `src/api/handler/video_cutout/{mod.rs, create_job/*, start_job/*, get_job/*, report_event/*}`
- Modify: `src/composition/wiring.rs`, `src/presentation/router.rs`, `src/main.rs`, `src/analytics/usage_tracking/operation.rs`, `src/analytics/usage_tracking/recorder.rs`（2 箇所）, `.env.example`, `CLAUDE.md`, `docs/ARCHITECTURE.md`

**Interfaces:**
- Consumes: Task 10〜14
- Produces:
  - `VideoCutoutConfig::from_env() -> Option<VideoCutoutConfig>`（Global Constraints の環境変数。`TRUSTED_PROXY_HOPS` 以外が 1 つでも欠ければ `None`）
  - `client_ip(headers: &HeaderMap, peer: SocketAddr, trusted_hops: usize) -> IpAddr` — hops=0 なら peer。N ≥ 1 なら `X-Forwarded-For` をカンマで分けて右から N 番目（足りなければ peer）。
  - `App::new(usage_recorder, video_quota: Arc<dyn DailyQuota>, video_config: Option<VideoCutoutConfig>)`。`App` に `video: Option<VideoCutout>` を持たせ、`async fn create_video_job / start_video_job / get_video_job / report_video_job_event` を追加。`video` が `None` なら `VideoCutoutError::Disabled => ("VIDEO_CUTOUT_DISABLED", Unavailable)`。各メソッドの先頭で `VideoCleanupThrottle` が許せば `DeleteExpiredVideoJobsUseCase` を await する。
  - HTTP（`client-id` ヘッダーが無い・UUID でなければ 400 `MISSING_CLIENT_ID`）:
    - `POST /video-jobs` 本文 `{size_bytes, content_type, start_ms, end_ms}` → 201 `{job_id, upload_url, remaining_today}`
    - `POST /video-jobs/{id}/start` 本文 `{points: [{x, y, label: "include"|"exclude"}], background: "green"|"blue"|"magenta"}` → 202 `{phase: "queued" | "failed", failure_code: string|null, remaining_today}`
    - `GET /video-jobs/{id}` → 200 `{phase: "created"|"queued"|"downloading"|"tracking"|"finishing"|"done"|"failed", progress: number|null, failure_code: string|null, quota_refunded: bool, download_url: string|null, remaining_today}`
    - `POST /internal/video-jobs/{id}/events` → 204。署名の検証（Task 13 の `verify`）→ パスと本文の `job_id` が違えば 400 `JOB_ID_MISMATCH` → `report_video_job_event`。
  - ルーター: `/video-jobs` と `/video-jobs/{id}/start` は `track_usage` の `.layer` より**前**、`GET /video-jobs/{id}` と `/internal/...` は**後**に追加する（利用ログに記録しない）。
  - `UsageOperation` に `CreateVideoJob`（パス `video-jobs`）と `StartVideoJob`（`video-jobs/` で始まり `/start` で終わる）を追加し、`recorder.rs` の 2 箇所で `"create-video-job"` / `"start-video-job"` に変換する。
  - `main.rs`: DB の接続ごとに `SqliteDailyQuota` / `PostgresDailyQuota` を作って `App::new` に渡す。`axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>())`。既存の GC タスクからも `app.cleanup_video_jobs().await` を呼ぶ（補助）。

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[test] fn client_ip_uses_peer_when_hops_zero() { assert_eq!(client_ip(&xff("1.1.1.1"), peer("9.9.9.9"), 0), ip("9.9.9.9")); }
#[test] fn client_ip_takes_nth_from_right() {
    assert_eq!(client_ip(&xff("6.6.6.6, 2.2.2.2"), peer("9.9.9.9"), 1), ip("2.2.2.2"));
    assert_eq!(client_ip(&xff("6.6.6.6, 2.2.2.2, 3.3.3.3"), peer("9.9.9.9"), 2), ip("2.2.2.2"));
    assert_eq!(client_ip(&xff("2.2.2.2"), peer("9.9.9.9"), 2), ip("9.9.9.9"));
}
#[test] fn usage_operation_maps_video_paths() {
    assert!(matches!(UsageOperation::new("video-jobs"), UsageOperation::CreateVideoJob));
    assert!(matches!(UsageOperation::new("video-jobs/6f1c2b9e-0d4a-4c1e-9a57-3f2e8b1d7c60/start"), UsageOperation::StartVideoJob));
}
#[test] fn config_is_none_when_any_var_missing() { assert!(VideoCutoutConfig::from_vars(vars_without("R2_BUCKET")).is_none()); }
#[tokio::test] async fn events_endpoint_rejects_bad_signature_and_mismatched_id() { /* Router を App のテスト用コンストラクタ（Fake を注入）で組み、tower::ServiceExt::oneshot で叩く: 署名なし → 401、正しい署名で別 id → 400、正しい → 204 */ }
#[tokio::test] async fn video_routes_return_503_when_disabled() { /* video_config = None の App で POST /video-jobs → 503 {"code":"VIDEO_CUTOUT_DISABLED"} */ }
```
（`VideoCutoutConfig::from_vars(impl Fn(&str) -> Option<String>)` をテストのために用意し、`from_env` はそれを `std::env::var` で呼ぶだけにする。Router のテストのために `App::for_test(...)` を `#[cfg(test)]` で用意する。SAM2 のモデルを読まずに作れる必要があるため、画像側の依存を受け取らない最小の形にする。）

- [ ] **Step 2: 失敗を確認 → 実装 → 成功を確認** — Run: `cargo test`
- [ ] **Step 3: ローカルで起動して疎通を確認する** — Run: `DATABASE_URL_SQLITE=sqlite://database/usage.db?mode=rwc cargo run`（動画の環境変数なし）→ `curl -X POST localhost:3000/video-jobs -H 'client-id: <uuid>' -H 'content-type: application/json' -d '{}'` / Expected: 503 `VIDEO_CUTOUT_DISABLED`。画像機能（`/upload`）は従来どおり動く。
- [ ] **Step 4: `.env.example` に環境変数を追記。`CLAUDE.md` に「動画の非同期ポートは spawn_blocking の例外」「動画ルートの追加箇所」「Cloud Run の最大 1 台を変えない」を追記し、`docs/ARCHITECTURE.md` に動画の流れを追記してコミット** — `git commit -m "feat(video): 動画ジョブの API・配線・IP の取得・利用ログを追加"`

---

### Task 16: フロントエンド（動画ページ）

**Files:**
- Create: `frontend/video.html`, `frontend/js/video/{page,range,preview,upload,job,messages}.js`
- Modify: `frontend/js/api.js`（`createVideoJob`, `startVideoJob`, `getVideoJob` を追加）、`frontend/index.html`（動画ページへのリンク）、`frontend/README.md`（動画の流れと API）

**Interfaces:**
- Consumes: Task 15 の HTTP API、既存の `api.js` の `upload` / `segment` / `undo` / `redo` / `getPreview`
- Produces:
  - `range.js`: `clampRange(startMs, endMs, durationMs, maxMs = 10000) -> {startMs, endMs}`、`captureFrame(video: HTMLVideoElement, ms) -> Promise<Blob>`（`seeked` を待って canvas → PNG）
  - `preview.js`: `toNormalized(previewX, previewY, previewW, previewH) -> {x, y}`（0〜1 に丸め込み）
  - `upload.js`: `putWithProgress(url, file, contentType, onProgress) -> Promise<void>`（`XMLHttpRequest`）
  - `job.js`: `pollJob(jobId, onUpdate, intervalMs = 1000) -> {stop()}`。`document.visibilitychange` で表示に戻ったら即座に 1 回取りに行く。GET が 404 なら「サーバーが再起動したため、やり直してください」を出して止める。
  - `messages.js`: 失敗コード → 日本語の文言（仕様 §5.1。`dispatch_failed` は「混み合っています。時間をおいて試してください」）、`queued` → 「準備中（GPU を起動しています）」、`tracking` → 「処理中 NN%」
- 画面の流れは仕様 §7。送信ボタンは「アップロード完了」かつ「点が 1 つ以上」のときだけ有効（Review Focus 4）。残り回数を常に表示し、0 なら選択画面で止める。
- `video.html` の favicon は `video-favicon.png`。

- [ ] **Step 1: 純粋な関数を確かめるテストページを作る**

フロントにはテストの仕組みが無いので、`frontend/js/video/selftest.html`（`<script type="module">` で `clampRange` と `toNormalized` を呼び、結果を `console.assert` と画面に出す）を作る。確認する値:
`clampRange(0, 12000, 30000)` → `{startMs: 0, endMs: 10000}`、`clampRange(25000, 31000, 30000)` → `{startMs: 25000, endMs: 30000}`、`toNormalized(300, 150, 600, 300)` → `{x: 0.5, y: 0.5}`、`toNormalized(-5, 400, 600, 300)` → `{x: 0, y: 1}`。
- [ ] **Step 2: Chromium で selftest を開いて確認する** — Run: Playwright で `selftest.html` を開き、`console` に assert の失敗が無いこと / Expected: 失敗 0 件
- [ ] **Step 3: `video.html` と残りのモジュールを実装する**
- [ ] **Step 4: ローカルで画面を確認する** — 動画の環境変数なしで起動し、`/video.html` を開く。動画を選ぶ → 範囲 → 開始フレームのプレビュー（既存の `/upload`・`/segment` が動く）まで進み、送信で 503 の文言が出ることを、iPhone 13 相当の画面サイズのスクリーンショットで確認する。
- [ ] **Step 5: コミット** — `git commit -m "feat(frontend): 動画切り抜きのページを追加"`

---

### Task 17: 本番の準備と通しの確認

オーナー向けの手順書: [`docs/video-cutout-deploy.md`](../../video-cutout-deploy.md)（R2・Secret Manager・環境変数・デプロイ・`TRUSTED_PROXY_HOPS` の確認・移植のテスト・iPhone の確認）。

オーナーの作業が必要: R2 のバケット・CORS（`https://chickren.goodbye-background.com` からの PUT/GET）・ライフサイクルルール（`video-jobs/` を 1 日）・API トークン。Cloud Run の Secret Manager と環境変数。Modal の予算上限 $30。

**Files:**
- Modify: `docs/video-cutout-measurements.md`（通しの結果を追記）、必要なら `.env.example`

- [ ] **Step 1: `TRUSTED_PROXY_HOPS` を確定する**

本番に、一時的に `X-Forwarded-For` と接続元をログに出す状態でデプロイし、`curl -H 'X-Forwarded-For: 6.6.6.6' https://chickren.goodbye-background.com/video-jobs ...` でアクセスする。
Expected: ログの `X-Forwarded-For` の右端が、自分の本当のグローバル IP になっている → `TRUSTED_PROXY_HOPS=1`。違う形なら、本当の IP が右から何番目かで N を決める。確認後、ログ出力は戻す。

**引き継ぎ事項（このタスクで必ず行う）:**
- 本物の PostgreSQL に対して `DailyQuota`（Postgres 実装）の煙テストをする（消費・上限・返却。開発のセッションでは SQLite でしか確かめていない）。
- R2 の CORS で、本番のオリジンからの PUT に `content-type` と `content-length` のヘッダーを許可する（PUT URL が両方を署名しているため）。
- `frontend/index.html` の動画（`video.html`）へのリンク 2 か所を戻す（判断 R9。本番の準備が終わるまで隠している）。
- 本物の iPhone でフレームの取り込み（プレビューの静止画）が空にならないことを確認する。

- [ ] **Step 2: 通しで動かす（iPhone 13 / iOS 18 の Safari）**

仕様 §8.3 の 3 項目と、Task 1 で用意した 4 種類の動画（HDR、60fps、被写体が途中から入る、緑っぽい服）で確認する:
1. 選択 → 範囲 → プレビュー → 送信 → 途中で他のアプリに切り替えて戻っても進捗が続く
2. ダウンロードして写真アプリに保存できる
3. CapCut のクロマキーで背景だけが抜け、ロゴが残る（緑・青・マゼンタ）
加えて、4 回目の送信で 429 の文言が出ること、送信から受け取りまでの時間（コールド / ウォーム）を記録する。

- [ ] **Step 3: 成功の基準（仕様 §1.2）と照らして結果を `docs/video-cutout-measurements.md` に追記し、コミット**

```bash
git add docs/video-cutout-measurements.md && git commit -m "docs: 動画切り抜きの本番での通しの確認結果を追加"
```
