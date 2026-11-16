# 動画: 前回のマスクで 1 点ずつ積み上げる Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 点を打ったフレームで、点を打った順に 1 点ずつ「前回のマスク」を渡して積み上げ、画像の切り抜きと同じように複数の物（人物＋マイク）を選べるようにする。

**Architecture:** マスクデコーダだけを `mask_input` / `has_mask` 付きで書き出し直した ONNX（Modal Volume に置き、SHA-256 で固定）に差し替える。
Rust のプレビューと Python のワーカーは、点を打ったフレームで同じ積み上げ（点の列の先頭 i 個 + 前回の low_res_mask）を行う。
API・フロント・サーバーの状態は変えない。追跡中のフレームは `has_mask=0` で今と同じ結果。

**Tech Stack:** Rust（ort / ndarray / sha2）、Python（onnxruntime / numpy）、Modal Volume、書き出しのみ torch 2.15.0 + transformers 5.17.0

**Spec:** `docs/superpowers/specs/2026-11-16-video-prompt-mask-input-design.md`

## Global Constraints

- 新しいデコーダのファイル名: `mask_decoder_with_mask_v1.onnx`
- SHA-256: `ba8862fdba8cb0027f7314bfb649fe18c16852a792ad6ca0dc76818aacac6ba6`（Python の `cutout/models.py` と Rust の `segmenter_sam2.rs` の 2 か所に書き、値をそろえる）
- Modal Volume: 名前 `goodbye-background-models`、Volume 内のパス `sam2/mask_decoder_with_mask_v1.onnx`、ワーカーのマウント先 `/vol`
- プレビューの置き場所: `models/sam2.1-small-video/onnx/mask_decoder_with_mask_v1.onnx`（vision_encoder と constants.json は今までどおり Hugging Face）
- 前回のマスクの丸め: `clip(low_res_mask, -32, 32)`。1 回目は `mask_input = 0（[1,1,256,256]）`, `has_mask = [0.0]`、2 回目以降は `has_mask = [1.0]`
- 追跡中のフレーム: ダミー点（座標 0,0・label -1）、`mask_input = 0`, `has_mask = [0.0]`
- DAM4SAM の候補は名前の付いた出力 `candidate_masks [1,4,256,256]` / `candidate_ious [1,4]` から読む（グラフの内部の名前 `/Gather_8_output_0` は使わない）
- SHA-256 が合わなければ、ワーカーは起動時に例外、プレビューはモデル無しと同じ扱い（動画機能 503・起動ログに理由）
- コメント・ドキュメントは日本語

## Review Focus

- **途中で object score が 0 以下になる点の列**（例: 1 点目が背景）: 前回のマスクは -1024 → -32 に丸めて次に渡す（公式と同じ）。最後の結果だけで判定し、途中で止めない
- **1 点目が「消す」**: 積み上げはそのまま行い、例外にしない
- **点 8 個（API の上限）でのプレビューの時間**: デコーダを 8 回回す。CPU で 1 リクエストの時間を計測して記録する
- **Volume のファイルが無い／壊れている**: 黙って diffusionstudio の旧デコーダに戻らない。ワーカーは起動に失敗、プレビューは 503
- **プレビューとワーカーの一致**: 同じ点の列で、プレビューのマスクとワーカーの 1 枚目のマスクが一致する（IoU ≥ 0.999）

---

### Task 1: デコーダの書き出しスクリプトと Volume への置き方

**Files:**
- Create: `worker/scripts/export_mask_decoder_with_mask.py`
- Modify: `docs/video-cutout-deploy.md`（「プレビューのモデルを取得する」の前に「新しいマスクデコーダを Modal Volume に置く（初回のみ）」の節）

**Interfaces:**
- Produces: `mask_decoder_with_mask_v1.onnx`（spec §2.1 の入出力）

- [ ] **Step 1:** 検証に使った書き出しのコード（spec §2.1 の選び方。`DecoderWithMask` が `transformers.Sam2VideoModel` のプロンプトエンコーダ・マスクデコーダ・`object_pointer_proj` を使う）をスクリプトにする。引数は `<チェックポイントのディレクトリ> <出力先>`。チェックポイントは `huggingface.co/facebook/sam2.1-hiera-small/resolve/main/` の `config.json` と `model.safetensors` を curl で落としたもの（xet の配布元に繋がらない環境があるため `from_pretrained` に直接リポジトリ名を渡さない）。冒頭の docstring に torch 2.15.0・transformers 5.17.0・opset 17・fp32 と、spec §2.1 の検証の数値を書く
- [ ] **Step 2:** 使い捨ての venv で書き出し、`sha256sum` が Global Constraints の値と一致することを確かめる（一致しなければ、この計画を止めてオーナーに報告。ファイルはセッションの scratchpad の `models/mask_decoder_with_mask_fp32.onnx` を正とする）
- [ ] **Step 3:** deploy の手順に次を書く（PowerShell。オーナーの PC で 1 回だけ）:
  `modal volume create goodbye-background-models` → `modal volume put goodbye-background-models <ファイル> sam2/mask_decoder_with_mask_v1.onnx` → `modal volume ls goodbye-background-models sam2` で確認
- [ ] **Step 4:** Commit `feat(video): 前回のマスクを受け取るマスクデコーダの書き出しスクリプトを足す`

### Task 2: ワーカー — モデルの読み込みと SHA-256 の照合

**Files:**
- Modify: `worker/cutout/models.py`
- Modify: `worker/cutout/tracker_variants.py:45`（`CANDIDATE_OUTPUTS`）
- Test: `worker/tests/test_models.py`（新規）

**Interfaces:**
- Produces（`cutout/models.py`）:
  - `MASK_DECODER_FILE = "mask_decoder_with_mask_v1.onnx"`、`MASK_DECODER_SHA256 = "ba8862fd…6ba6"`（全桁）、`MODELS_VOLUME = "goodbye-background-models"`、`MASK_DECODER_VOLUME_PATH = "sam2/mask_decoder_with_mask_v1.onnx"`
  - `GRAPH_NAMES` から `"mask_decoder"` を外し、Hugging Face から取るのは 4 グラフだけにする（`fetch_models` の `graphs` 省略時もこの 4 つ）
  - `def verify_sha256(path: Path, expected: str) -> None`（合わなければ `ValueError`。メッセージにファイル名と実際の値）
  - `def load_sessions(model_dir: Path, device: Literal["cpu","cuda"], mask_decoder_path: Path) -> Sam2Sessions`（`mask_decoder_path` を `verify_sha256` してから読む。`decoder_extra_outputs` 引数と `with_extra_outputs` は消す）
- Produces（`tracker_variants.py`）: `CANDIDATE_OUTPUTS = ["candidate_masks", "candidate_ious"]`

- [ ] **Step 1: 失敗するテストを書く**（`test_models.py`）
  - `test_verify_sha256_accepts_matching_file`: `tmp_path` に `b"abc"` を書き、`hashlib.sha256(b"abc").hexdigest()` で例外が出ない
  - `test_verify_sha256_rejects_other_file`: 違う値で `pytest.raises(ValueError, match="mask")`（ファイル名 `mask.onnx`）
  - `test_graph_names_exclude_mask_decoder`: `"mask_decoder" not in GRAPH_NAMES` かつ `len(GRAPH_NAMES) == 4`
- [ ] **Step 2:** `cd worker && uv run pytest tests/test_models.py -v` → FAIL
- [ ] **Step 3:** 実装する。`load_sessions` を呼んでいる所（`modal_app.py`・`measure/measure_app.py`・`scripts/diagnose_logits.py`・`tests/test_tracker_variants.py`・`tests/test_tracker_parity.py`）の引数を新しい形にする（`mask_decoder_path` は呼び出し側で `model_dir / "onnx" / MASK_DECODER_FILE` か Volume のパス）
- [ ] **Step 4:** `uv run pytest -q` → PASS（`slow` は飛ぶ）
- [ ] **Step 5:** Commit `feat(worker): マスクデコーダを SHA-256 で照合して読み、候補は名前の付いた出力から取る`

### Task 3: ワーカー — 点を打ったフレームで 1 点ずつ積み上げる

**Files:**
- Create: `worker/cutout/prompt_decode.py`
- Modify: `worker/cutout/tracker.py:96-120`、`worker/cutout/tracker_variants.py:388-418`
- Test: `worker/tests/test_prompt_decode.py`（新規）、`worker/tests/test_tracker.py`、`worker/tests/test_tracker_variants.py`（偽のデコーダが `mask_input` / `has_mask` を受け、`low_res_mask` を返すようにする）

**Interfaces:**
- Consumes: Task 2 の `CANDIDATE_OUTPUTS`
- Produces（`cutout/prompt_decode.py`）:
  - `def decode_points(decoder: Graph, feats0, feats1, feats2_cond, points: list[NormalizedPoint], size: int) -> dict`（spec §2.2 の積み上げ。最後のデコーダの出力をそのまま返す）
  - `def decode_tracking(decoder: Graph, feats0, feats1, feats2_cond) -> dict`（ダミー点・`has_mask=0`）
  - `MASK_INPUT_CLIP = 32.0`

- [ ] **Step 1: 失敗するテストを書く**（`test_prompt_decode.py`。呼ばれた引数を記録する偽のデコーダ。`low_res_mask` は呼ばれた回数 k に対して `np.full((1,1,256,256), 100.0 * k)` を返す）
  - `test_one_point_calls_decoder_once_without_mask`: 呼び出し 1 回、`has_mask == [0.0]`、`mask_input` が全部 0
  - `test_three_points_chain_prefixes_in_order`: 呼び出し 3 回、i 回目の `input_points` が先頭 i 点（size=1024 で `[[[[x*1024, y*1024], …]]]`）、`input_labels` が先頭 i 個
  - `test_chain_passes_previous_low_res_clipped`: 2 回目・3 回目の `has_mask == [1.0]`、`mask_input` が全部 32.0（100 を 32 に丸めたもの）
  - `test_returns_last_decoder_output`: 戻り値の `low_res_mask` が 300.0
  - `test_exclude_first_point_still_chains`: 1 点目が exclude（label 0）でも 2 回呼ばれ、1 回目の `input_labels == [[[0]]]`
  - `test_tracking_uses_pad_point_without_mask`: `decode_tracking` の `input_labels == [[[-1]]]`、`has_mask == [0.0]`
- [ ] **Step 2:** `uv run pytest tests/test_prompt_decode.py -v` → FAIL
- [ ] **Step 3:** `prompt_decode.py` を実装し、`tracker.py` と `tracker_variants.py` の点のあるフレーム／追跡のフレームのデコードをこの 2 関数に置き換える。`tracker_variants` の `_is_single_output` は、最後の出力の `high_res_mask` と `candidate_masks` を比べる今の方法のまま使う
- [ ] **Step 4:** 既存の `test_tracker.py` / `test_tracker_variants.py` の期待値を直す（例: `test_exclude_label_is_zero` は 2 回目の呼び出しが `[[[1, 0]]]`）。`uv run pytest -q` → PASS
- [ ] **Step 5:** Commit `feat(worker): 点を打ったフレームで前回のマスクを渡して 1 点ずつ積み上げる`

### Task 4: Modal・プレビュー用の取得スクリプト

**Files:**
- Modify: `worker/modal_app.py`（`Worker` に `volumes={"/vol": modal.Volume.from_name(MODELS_VOLUME)}`、`load` で `load_sessions(MODEL_DIR, "cuda", Path("/vol") / MASK_DECODER_VOLUME_PATH)`）
- Modify: `worker/measure/measure_app.py`（同じ Volume をマウント）
- Modify: `worker/scripts/fetch_preview_models.py`
- Test: `worker/tests/test_fetch_preview_models.py`

**Interfaces:**
- Consumes: Task 2 の定数・`verify_sha256`
- Produces: `PREVIEW_FILES = ["constants.json", "onnx/vision_encoder.onnx", "onnx/mask_decoder_with_mask_v1.onnx"]`、`def fetch_mask_decoder(model_dir: Path, run=subprocess.run) -> Path`（`modal volume get --force goodbye-background-models sam2/mask_decoder_with_mask_v1.onnx <model_dir>/onnx/mask_decoder_with_mask_v1.onnx` を実行し、`verify_sha256`。既にあって SHA-256 が合えば実行しない）

- [ ] **Step 1: 失敗するテストを書く**
  - `test_fetch_mask_decoder_runs_modal_volume_get`: 偽の `run` が受けた引数が上のコマンドと一致し、書かれたファイルの SHA-256 が合えば成功（テストでは `MASK_DECODER_SHA256` を monkeypatch して小さいバイト列にする）
  - `test_fetch_mask_decoder_skips_when_hash_matches`: 既にあって合えば `run` を呼ばない
  - `test_fetch_mask_decoder_rejects_wrong_hash`: 取得後に合わなければ `ValueError`
  - `test_preview_graphs_fetch_only_vision_encoder_from_hf`: `fetch_models` に渡す `graphs == ["vision_encoder"]`
- [ ] **Step 2:** `uv run pytest tests/test_fetch_preview_models.py -v` → FAIL
- [ ] **Step 3:** 実装する。古い `onnx/mask_decoder.onnx` が残っていても使わない（消さない）
- [ ] **Step 4:** `uv run pytest -q` → PASS
- [ ] **Step 5:** Commit `feat(worker): 新しいマスクデコーダを Modal Volume から読む・取得する`

### Task 5: Rust のプレビュー — 新しいデコーダで 1 点ずつ積み上げる

**Files:**
- Modify: `src/video_cutout/preview/segmenter_sam2.rs`
- Test: 同じファイルの `segmenter_sam2_test`

**Interfaces:**
- Consumes: Global Constraints のファイル名・SHA-256
- Produces:
  - `pub const PREVIEW_MODEL_FILES: [&str; 3] = ["constants.json", "onnx/vision_encoder.onnx", "onnx/mask_decoder_with_mask_v1.onnx"]`（`config.rs` がそのまま使う）
  - `pub const MASK_DECODER_SHA256: &str`
  - `pub fn verify_sha256(path: &Path, expected: &str) -> Result<(), PreviewError>`（合わなければ `PreviewError::ModelLoad`。ファイル名と実際の値を入れる）
  - `pub fn point_prefixes(points: &Points) -> Vec<Points>`（先頭 1 個, 2 個, …, n 個）
  - `pub fn clip_mask_input(low_res: &Array4<f32>) -> Array4<f32>`（-32〜32）
  - `PreviewSegmenter::decode` の signature は変えない（積み上げは実装の中。`preview_mask` のユースケースと偽物は変更なし）

- [ ] **Step 1: 失敗するテストを書く**
  - `point_prefixes_grow_in_tap_order`: 3 点（include, include, exclude）→ 長さ 1・2・3、最後の要素のラベルが exclude
  - `clip_mask_input_clamps_to_32`: `[-1024, -5, 5, 100]` → `[-32, -5, 5, 32]`
  - `verify_sha256_accepts_and_rejects`: `tempfile` 無しで `std::env::temp_dir()` に `b"abc"` を書き、`ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad` で Ok、別の値で `Err(PreviewError::ModelLoad(_))`
  - 既存の `#[ignore] preview_model_encodes_and_decodes` に、3 点で `decode` が成功し `logits` が有限であることを足す
- [ ] **Step 2:** `cargo test segmenter_sam2` → FAIL（ネットワーク制限下は CLAUDE.md の `ORT_LIB_LOCATION` の手順）
- [ ] **Step 3:** 実装する。`Sam2PreviewSegmenter::new` で `verify_sha256` してから読む。`decode` は `point_prefixes` の順にデコーダを回し、入力に `mask_input`（`Feed::F32`、1 回目は 0）・`has_mask`（`[1]`、0.0 / 1.0）を足し、出力に `low_res_mask` を加えて次へ渡す。結果は最後の `high_res_mask` と `object_score_logits`
- [ ] **Step 4:** `cargo test` → PASS、`cargo check --all-targets` の警告が 15 件のまま
- [ ] **Step 5:** Commit `feat(video): プレビューで前回のマスクを渡して 1 点ずつ積み上げる`

### Task 6: 実モデルでの確認とドキュメント

**Files:**
- Modify: `CLAUDE.md`（前提 16 に「マスクデコーダだけ自前の書き出し（Modal Volume・SHA-256）、点を打ったフレームは 1 点ずつ積み上げ」）
- Modify: `docs/ARCHITECTURE.md` §9.4、`docs/video-cutout-deploy.md`（プレビューのモデルの取得・確認の項目）、`docs/video-cutout-measurements.md`（今回の再現と計測）

- [ ] **Step 1:** 実モデルで Python の `slow` テストを通す: `SAM2_MODEL_DIR=<4 グラフ> SAM2_MASK_DECODER=<新しい onnx> uv run pytest -m slow -q` → PASS（`test_tracker_parity` は新しいデコーダで基準を取り直す必要があれば、その理由をテストのコメントに書く）
- [ ] **Step 2:** Rust の `#[ignore]` テストを実モデルで通す: `SAM2_PREVIEW_MODEL_DIR=models/sam2.1-small-video cargo test -- --ignored preview_model` → PASS
- [ ] **Step 3:** 今回の動画のフレーム 0 で、スタンド・袈裟・ケーブルの 3 点（正規化座標 `(0.955,0.782)` `(0.751,0.837)` `(0.585,0.947)`）を Rust のプレビューとワーカーの 1 枚目に通し、両方の alpha（`keep_largest_region` の後）が 600,000 画素以上で、互いの IoU ≥ 0.999 であることを確かめる
- [ ] **Step 4:** 点 1・4・8 個でのプレビュー 1 リクエストのデコード時間（CPU）を計測し、measurements に表で書く
- [ ] **Step 5:** ドキュメントを更新し、Commit `docs(video): 前回のマスクで積み上げる方式と Modal Volume の手順を書く`
