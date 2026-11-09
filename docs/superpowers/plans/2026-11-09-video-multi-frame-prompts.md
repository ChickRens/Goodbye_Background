# 動画切り抜き: 複数フレームへの点の指定 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 開始位置以外のフレーム（最大 5 枚）にも点を打てるようにし、ワーカーがそのフレームで被写体を記憶なしで捉え直す。

**Architecture:** 点を `prompts = [{time_ms, points}]` で受け渡す。Rust はフレームごとにプレビューの特徴量を持ち、送信時に検査してワーカーへ
`prompts`（と移行用の `points`）を送る。ワーカーの `VariantTracker` は指定フレームを 1 枚目と同じ計算でデコードし条件メモリに入れる（方式 A）。

**Tech Stack:** Rust（axum 0.8 / ort / ndarray）、Python 3.12（numpy / onnxruntime / cv2、Modal）、Vanilla JS

**Spec:** `docs/superpowers/specs/2026-11-09-video-multi-frame-prompts-design.md`

## Global Constraints

- prompt: `{ "time_ms": <整数>, "points": [...] }`。1〜5 個（`MAX_PROMPTS = 5`）、1 つ目の `time_ms` = `range_ms.start`、昇順・重複なし、`start ≤ time_ms < end`
- points は今と同じ（1〜8 個、0.0〜1.0、`include` / `exclude`）
- ワーカーへの依頼は `prompts` と `points`（= 1 つ目の prompt の点）の両方を送る。ワーカーは `prompts` 優先、無ければ `points` を開始位置の 1 つとして扱う
- フレーム番号 = `round((time_ms − range_ms.start) × fps ÷ 1000)`（fps = 出力の fps）。同じ番号は後の prompt を使う。`>= total` は捨てる
- 特徴量の全体の上限 `MAX_PREVIEW_FEATURES = 16`（枚単位の LRU）、1 ジョブ最大 5 枚
- 新しいエラーコード: `TOO_MANY_PREVIEW_FRAMES`（409）、`INVALID_PREVIEW_TIME`（400）、`INVALID_PROMPTS`（400）。`common/macros.rs` のマクロで定義
- CLAUDE.md の約束事を守る（特に 16: 指定フレームの出力 = プレビュー。13: マイグレーションは触らない）。コメント・文書は日本語

## Review Focus

- 範囲の終わり近くの `time_ms`（フレーム番号が `total` 以上）→ 捨てて残りは正常に処理（Task 3 のテスト）
- 近すぎて同じフレーム番号になる 2 つの prompt（例 4300 と 4310 @30fps）→ 後のものを使い、落ちない（Task 3 のテスト）
- 指定フレームで被写体が見つからない（2 枚目以降）→ 見失いとして数え、ジョブは成功（Task 3 のテスト）
- 範囲（手順 2）を選び直したあと → 追加したフレームと点は全部消える（Task 8 のテスト）
- 他の人の登録で特徴量が LRU で消えた 2 枚目以降のフレーム → そのフレームだけ登録し直してプレビューが出る（Task 8 のテスト。409 の再登録を時刻つきで）

---

### Task 1: 契約（ワーカーへの依頼）に prompts を足す

**Files:**
- Modify: `contracts/video_cutout/worker_job_request.json`（`"prompts": [{ "time_ms": 1200, "points": [...] }, { "time_ms": 4300, "points": [...] }]` を足す。`points` は 1 つ目と同じ値で残す）
- Modify: `worker/cutout/contract.py`、`src/video_cutout/worker/request.rs`
- Test: `worker/tests/test_contract.py`、`src/video_cutout/worker/request.rs` の既存のテスト

**Interfaces:**
- Produces（Python）: `@dataclass(frozen=True) class Prompt: time_ms: int; points: list[NormalizedPoint]`、`WorkerJobRequest.prompts: list[Prompt]`（`points` フィールドは削除。`from_json` が `prompts` 優先、無ければ `[Prompt(range_ms.start, points)]`）、`MAX_PROMPTS = 5`
- Produces（Rust）: `pub struct WorkerPrompt { pub time_ms: u32, pub points: Vec<WorkerPoint> }`、`WorkerJobRequest.prompts: Vec<WorkerPrompt>`（`points` も残す）

- [ ] **Step 1: 失敗するテストを書く**（Python）
  - `test_prompts_parsed_from_contract_example`: 例の JSON から `[p.time_ms for p in req.prompts] == [1200, 4300]`
  - `test_legacy_points_become_single_prompt_at_start`: `prompts` を消した JSON で `req.prompts == [Prompt(1200, [NormalizedPoint(0.51, 0.44, "include")])]`
  - `test_prompts_rules_rejected`（parametrize）: 6 個／1 つ目が start でない／重複／降順／`time_ms >= end`／`time_ms` が整数でない → `ContractError`
- [ ] **Step 2: 実行して失敗を確かめる** — `cd worker && uv run pytest tests/test_contract.py -q` → FAIL
- [ ] **Step 3: `contract.py` に `Prompt`・`_prompts(v, range_ms)` を実装し、JSON の例を更新。Rust の `WorkerJobRequest` に `prompts` を足し、既存の「契約の例と同じ形」のテストが通るようにする**
- [ ] **Step 4: 実行して通る** — `uv run pytest -q`（全件）、`cargo test worker::request` → PASS（Rust の他の箇所が `prompts` を渡さずコンパイルできない所は、この時点では `vec![]` ではなく `points` から作った 1 つの prompt を入れる仮の値で通し、Task 7 で本物にする）
- [ ] **Step 5: コミット** `契約: ワーカーへの依頼に prompts（複数フレームの点）を足す`

### Task 2: VariantTracker が 2 枚目以降の点を受け付ける

**Files:**
- Modify: `worker/cutout/tracker_variants.py`
- Test: `worker/tests/test_tracker_variants.py`

**Interfaces:**
- Consumes: なし（今の `VariantTracker`）
- Produces: `VariantTracker.process(frame_rgb, points: list[NormalizedPoint] | None) -> FrameResult` — 1 枚目は points 必須（今どおり）、2 枚目以降も points を受け付ける。`VariantTracker.prompted: list[int]`（点で条件メモリにしたフレーム。0 を含む）

- [ ] **Step 1: 失敗するテストを書く**（偽のセッション。`tests/test_tracker.py` の `RecordingSessions` と同じ作りに候補マスクの出力を足したもの）
  - `test_points_on_later_frame_decode_without_memory`: 3 枚目に points を渡すと、その回のデコードの `feats2_cond` が `vision_encoder` の `feats2_no_mem`、`memory_attention` が呼ばれない、`memory_encoder` の `binarize == 1`、入力点の座標が `x * S`
  - `test_prompted_frame_becomes_conditioning_memory`（policy="dam4sam"）: 3 枚目を指定した後の 4 枚目で、`_blocks_dam4sam` の選ばれた条件メモリに 3 が入る（時間埋め込みの行が `COND_ROW`）
  - `test_prompted_frame_not_added_to_drm_twice`: `stats["drm_added"]` に指定フレームが入らない
- [ ] **Step 2: 失敗を確かめる** — `uv run pytest tests/test_tracker_variants.py -q` → FAIL（今は「2 枚目以降は points を渡さない」で ValueError）
- [ ] **Step 3: 実装**。points があるフレームは 1 枚目と同じ分岐（`feats2_no_mem`・点・`binarize=1`・`_is_single_output` で候補を決める）。条件メモリの一覧は `[0] + self._prompted_later + self._drm`（DAM4SAM の `select_closest_cond` に渡す）。DAM4SAM の DRM の判定（`_dam_update`）は指定フレームでは呼ばない。`HISTORY_FRAMES` の掃除で指定フレームを消さない
- [ ] **Step 4: 通る** — `uv run pytest -q` と `SAM2_MODEL_DIR=models/sam2.1-tiny-video-full uv run pytest -q -m slow tests/test_tracker_variants.py`（policy="sam2" が本番と一致するテストが壊れていない）
- [ ] **Step 5: コミット** `ワーカー: 追跡の途中のフレームにも点を受け付ける（記憶なしで捉え直し、条件メモリにする）`

### Task 3: パイプラインが prompts をフレームに割り当てる

**Files:**
- Modify: `worker/cutout/pipeline.py`、`worker/cutout/postprocess.py`
- Test: `worker/tests/test_pipeline.py`、`worker/tests/test_postprocess.py`

**Interfaces:**
- Consumes: `WorkerJobRequest.prompts`（Task 1）、`VariantTracker.process`（Task 2）
- Produces: `pipeline.prompt_frames(prompts: list[Prompt], start_ms: int, fps: float, total: int) -> dict[int, list[NormalizedPoint]]`、`DetachedBlobFilter.reset(logits: np.ndarray) -> None`

- [ ] **Step 1: 失敗するテストを書く**
  - `test_prompt_frames_rounding_and_collision`: start=1000, fps=30, total=300 で `[1000, 4300, 4310, 11000]` → `{0: …, 99: <4310 の点>}`（4300 と 4310 は同じ 99、後のもの。11000 は 300 以上で捨てる）
  - `test_tracker_receives_points_on_prompted_frames`: 偽のトラッカーが受け取った points が、フレーム 0 と指定フレームだけ `not None`
  - `test_lost_on_later_prompted_frame_is_not_failure`: 指定フレームで `object_present=False` → `done`、`lost_frames == 1`
  - `test_reset_replaces_previous_mask`（postprocess）: `reset(x)` の後、x の範囲の近くの塊が残る
  - `test_blob_filter_skips_prompted_frames`（`REMOVE_DETACHED_BLOBS=True`）: 指定フレームの離れた塊は消えない（緑にならない）
- [ ] **Step 2: 失敗を確かめる** — `uv run pytest tests/test_pipeline.py tests/test_postprocess.py -q` → FAIL
- [ ] **Step 3: 実装**。`_process` で `prompt_frames(req.prompts, start_ms, fps, total)` を作り、`tracker.process(frame, prompts.get(frames))`。指定フレーム（0 以外）では後処理をかけず `blob_filter.reset(logits)`。`make_tracker` の型は `Callable[[int], Tracker]`（`process` を持つ Protocol）にする
- [ ] **Step 4: 通る** — `uv run pytest -q` → 全件 PASS
- [ ] **Step 5: コミット** `ワーカー: prompts をフレーム番号に割り当て、指定フレームで後処理を置き直す`

### Task 4: Rust のドメイン `Prompts`

**Files:**
- Create: `src/video_cutout/job/prompt.rs`（`job/mod.rs` に登録）
- Modify: `src/video_cutout/job/error.rs`（`InvalidPrompts => ("INVALID_PROMPTS", ErrorType::InvalidInput)`）、`src/video_cutout/job/video_job.rs`（`points: Option<Points>` → `prompts: Option<Prompts>`）

**Interfaces:**
- Produces: `pub const MAX_PROMPTS: usize = 5;`、`pub struct Prompt { time_ms: u32, points: Points }`（`Prompt::new(time_ms, points)`、`time_ms()`、`points()`）、`pub struct Prompts(Vec<Prompt>)` と `Prompts::new(prompts: Vec<Prompt>, range: &ClipRange) -> Result<Prompts, VideoJobError>`、`as_slice()`、`first() -> &Prompt`

- [ ] **Step 1: 失敗するテストを書く**（`prompt.rs` 末尾の `mod prompt_test`）: 正しい 1 個・5 個が通る／0 個・6 個／1 つ目が start でない／重複／降順／`time_ms == end` → `InvalidPrompts`
- [ ] **Step 2: 失敗を確かめる** — `cargo test prompt_test` → FAIL（コンパイルエラー）
- [ ] **Step 3: 実装**し、`VideoJob.prompts` に置き換える（使っている所は Task 7 まで `Prompts` を 1 つの prompt で作って通す）
- [ ] **Step 4: 通る** — `cargo test`、`cargo check --all-targets` の警告が 15 件のまま
- [ ] **Step 5: コミット** `動画: 複数フレームの点（Prompts）のドメインを足す`

### Task 5: 特徴量をジョブと時刻ごとに持つ

**Files:**
- Modify: `src/video_cutout/preview/feature_store.rs`

**Interfaces:**
- Produces: `save(&self, job_id: VideoJobId, time_ms: u32, features: Arc<PreviewFeatures>) -> Result<(), TooManyFrames>`（そのジョブにその時刻が無く、すでに `MAX_PROMPTS` 枚なら `Err`）、`get(&self, job_id: &VideoJobId, time_ms: u32) -> Option<Arc<PreviewFeatures>>`、`remove(&self, job_id: &VideoJobId)`（全時刻）、`retain_ids(...)`（今どおり）

- [ ] **Step 1: 失敗するテストを書く**: 同じジョブの別の時刻を別々に持つ／同じ時刻は置き換え（枚数は増えない）／6 枚目は `Err`／全体 16 枚を超えると最も長く使われていない 1 枚だけ消える／`remove` で全時刻が消える
- [ ] **Step 2: 失敗を確かめる** — `cargo test preview_feature_store_test` → FAIL
- [ ] **Step 3: 実装**（キーを `(VideoJobId, u32)` に）。使っている所（set_preview_frame・preview_mask・start_job・delete_expired_jobs）は、この時点では開始位置の時刻（`job.range.start_ms()`）で呼んで通す
- [ ] **Step 4: 通る** — `cargo test`
- [ ] **Step 5: コミット** `動画: プレビューの特徴量をジョブと時刻ごとに持つ（1 ジョブ最大 5 枚）`

### Task 6: preview-frame と preview に time_ms を足す

**Files:**
- Modify: `src/api/handler/video_cutout/set_preview_frame/{handler,request}.rs`、`src/api/handler/video_cutout/preview_mask/{handler,request}.rs`、`src/video_cutout/set_preview_frame/{input,error,usecase}.rs`、`src/video_cutout/preview_mask/{input,usecase}.rs`、`src/video_cutout/preview/error.rs`（`TooManyFrames => ("TOO_MANY_PREVIEW_FRAMES", ErrorType::Conflict)`、`InvalidTime => ("INVALID_PREVIEW_TIME", ErrorType::InvalidInput)`）、`frontend/js/api.js`（`setPreviewFrame(jobId, blob, timeMs)`・`previewMask(jobId, timeMs, points)`）
- Test: 各ユースケースのテスト、`src/presentation/router.rs` のルーターのテスト

**Interfaces:**
- Consumes: `PreviewFeatureStore::save/get`（Task 5）
- Produces: `SetPreviewFrameInput.time_ms: u32`、`PreviewMaskInput.time_ms: u32`。HTTP: `POST /video-jobs/{id}/preview-frame?time_ms=4300`（無い・整数でない → 400 `INVALID_PREVIEW_TIME`）、`POST /video-jobs/{id}/preview` の本文 `{ "time_ms": 4300, "points": [...] }`

- [ ] **Step 1: 失敗するテストを書く**: 範囲外の `time_ms` → `InvalidTime`／2 つの時刻で登録すると両方 `get` できる／6 枚目 → `TooManyFrames`（409）／`preview` はその時刻の特徴量でデコード（偽のセグメンタに渡った特徴量の `frame_width` で区別）／登録していない時刻 → 409 `PREVIEW_FRAME_NOT_FOUND`／ルーター: `time_ms` の無い `preview-frame` → 400
- [ ] **Step 2: 失敗を確かめる** — `cargo test set_preview_frame preview_mask router` → FAIL
- [ ] **Step 3: 実装**（クエリは `axum::extract::Query<PreviewFrameQuery { time_ms: u32 }>`。読めないときの拒否を `INVALID_PREVIEW_TIME` に揃える）。`api.js` の 2 関数に引数を足す（呼び出し側は Task 8 で直すまで開始位置の時刻を渡す）
- [ ] **Step 4: 通る** — `cargo test`
- [ ] **Step 5: コミット** `動画: プレビューのフレームの登録と表示に時刻を足す`

### Task 7: 送信（start）が prompts を受け取り、ワーカーへ送る

**Files:**
- Modify: `src/api/handler/video_cutout/start_job/request.rs`（`prompts: Vec<PromptRequest { time_ms: u32, points: Vec<PointRequest> }>`）、`src/video_cutout/start_job/{input,usecase}.rs`、`frontend/js/api.js`（`startVideoJob(jobId, { prompts, background })`）
- Test: `start_job/usecase.rs` のテスト、ルーターのテスト

**Interfaces:**
- Consumes: `Prompts::new`（Task 4）、`WorkerJobRequest.prompts`（Task 1）
- Produces: `StartVideoJobInput.prompts: Vec<Prompt>`（検査前）。ユースケースが `Prompts::new(input.prompts, &job.range)` で検査し、違反は 400 `INVALID_PROMPTS`。ワーカーへは `prompts` 全部と `points = prompts.first().points`

- [ ] **Step 1: 失敗するテストを書く**: 2 つの prompt で開始 → `job.prompts` に 2 つ、ワーカーの依頼の `prompts.len() == 2`・`points` が 1 つ目の点／1 つ目の時刻が開始でない → 400 `INVALID_PROMPTS` で回数を消費しない／ルーター: 旧形式 `{points, background}` → 400
- [ ] **Step 2: 失敗を確かめる** — `cargo test start_job router` → FAIL
- [ ] **Step 3: 実装**（Task 1・4・5 で入れた仮の値をすべて本物に置き換える）
- [ ] **Step 4: 通る** — `cargo test`、`cargo check --all-targets`（警告 15 件のまま）
- [ ] **Step 5: コミット** `動画: 送信で複数フレームの点を受け取り、ワーカーへ送る`

### Task 8: スマホの画面（フレームのタブと追加）

**Files:**
- Modify: `frontend/video.html`（手順 4 にタブの列・「このフレームにも点を打つ」ボタン・範囲の中で動かせる動画と「このフレームに決める」・案内の文）、`frontend/css/video.css`、`frontend/js/video/preview.js`（1 フレーム分のコントローラを `createFramePreview({ jobId, timeMs, frameBlob })` にし、`page.js` が複数持つ）、`frontend/js/video/page.js`、`frontend/js/video/messages.js`
- Test: `frontend/js/video/selftest.html`

**Interfaces:**
- Consumes: `setPreviewFrame(jobId, blob, timeMs)`・`previewMask(jobId, timeMs, points)`（Task 6）、`startVideoJob(jobId, { prompts, background })`（Task 7）
- Produces: `canSend(frames) -> boolean`（全フレームで点が 1 つ以上・`objectPresent === true`）、`buildPrompts(frames) -> [{ time_ms, points }]`（時刻の昇順。`time_ms` は `Math.round(currentTime * 1000)`）

- [ ] **Step 1: selftest に失敗するテストを足す**: `canSend` が 1 枚でも未確定・被写体なしなら false／`buildPrompts` が昇順で 1 つ目が開始位置／同じ時刻のフレームは追加できない／5 枚で追加ボタンが無効／範囲を選び直すとフレームが開始位置の 1 枚だけに戻る／409 `PREVIEW_FRAME_NOT_FOUND` でそのフレームの時刻だけ登録し直す
- [ ] **Step 2: 失敗を確かめる** — `selftest.html` をブラウザ（Playwright の chromium）で開き、足したテストが FAIL
- [ ] **Step 3: 実装**。案内の文は仕様 §6 のとおり。タブの表示は `m:ss.s`
- [ ] **Step 4: 通る** — selftest が全件 PASS。390px 幅のスクリーンショットでタブと案内が崩れていない
- [ ] **Step 5: コミット** `動画: 手順 4 で複数のフレームに点を打てるようにする`

### Task 9: 実モデルでの確認と文書

**Files:**
- Modify: `worker/scripts/diagnose_logits.py`（`--prompt "TIME_MS:x,y;x,y,exclude"` を複数回。`--point` は開始位置の prompt の省略形として残す）、`CLAUDE.md`（約束事 16 に「指定フレームもプレビューと一致」）、`docs/ARCHITECTURE.md`（§9 の流れと §9.4）、`docs/video-cutout-deploy.md`（§12: Modal と Cloud Run のデプロイ。順番は自由・iPhone での確認）、`docs/video-cutout-measurements.md`
- Test: `worker/tests/test_diagnose_logits.py`（`--prompt` の読み取り）

- [ ] **Step 1: 失敗するテストを書く**: `parse_prompt("4300:0.5,0.4;0.2,0.5,exclude") == (4300, [NormalizedPoint(0.5,0.4,"include"), NormalizedPoint(0.2,0.5,"exclude")])`
- [ ] **Step 2: 失敗を確かめる** → FAIL
- [ ] **Step 3: 実装**し、カットのある元のダーツの動画（8.4 秒）を `--prompt 0:0.60,0.86 --prompt <カットの約 0.4 秒後>:<人物の胸>` で処理する（small ＋ dam4sam）
- [ ] **Step 4: 確かめる** — CSV の `present == 0` が 15 フレーム → 2 フレーム以下。指定フレームの出力とプレビュー（Rust の 1 枚目と同じ計算）のマスクの IoU ≥ 0.999。結果を計測の記録に書き、文書を更新
- [ ] **Step 5: コミット** `動画: 複数フレームの点を実モデルで確認し、文書を更新`
