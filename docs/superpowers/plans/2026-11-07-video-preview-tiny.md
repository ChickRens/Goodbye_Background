# 動画のプレビューを本番と同じ tiny にする（追加計画）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** 動画ページのプレビューを、画像機能の SAM2.1 small（1024px）の流用から、本番の GPU ワーカーと同じ
SAM2.1 tiny（512px、`diffusionstudio/sam2.1-tiny-video-onnx-fp16@66673b5db39371b7dd7847f4d3bc0d4f4179b79e`）の 1 枚目の推論に置き換え、
「プレビューで見えた範囲 = 出力の 1 フレーム目」にする。

**背景:** 本番で、ベストを 1 点タップしたとき small のプレビューは人物全体、tiny の本番はベストだけを選んだ（2026-11-07）。

**Architecture:** Rust（Cloud Run の CPU）が tiny の `vision_encoder` と `mask_decoder` を ort で動かす。
フレームの登録時に 1 回だけエンコードして特徴量をジョブごとにメモリに置き、点が変わるたびにデコーダだけを回してマスクの PNG を返す。

**Spec:** `docs/superpowers/specs/2026-11-05-video-cutout-design.md`（D8 を本計画で置き換える）

## Global Constraints

- 本番のワーカー（`worker/cutout/tracker.py` の 1 枚目、`worker/cutout/compositor.py`）と同じ計算にする:
  - 前処理: フレームを 512×512 にバイリニアで引き伸ばし（余白なし）、RGB を 0〜1 → `constants.json` の `image_mean` / `image_std` で正規化、CHW `[1,3,512,512]`
  - デコーダ入力: `feats0`, `feats1`, `feats2_cond = feats2_no_mem`, `input_points = [[[[x*512, y*512], ...]]]`（f32）, `input_labels = [[[1 or 0, ...]]]`（i32。include=1 / exclude=0）。詰め点なし
  - 出力 `high_res_mask`（512×512 のロジット）→ プレビュー画像の大きさにバイリニア拡大 → ±50 にクリップ → ×16 → ±50 にクリップ → sigmoid（`EDGE_SHARPNESS = 16`）
  - グラフの入力 dtype はモデルの宣言に合わせる（fp16 なら変換）。出力の fp16 は f32 にする（`worker/cutout/models.py` の `OnnxGraph` と同じ）
- モデルの置き場所: `models/sam2.1-tiny-video/constants.json` と `models/sam2.1-tiny-video/onnx/{vision_encoder,mask_decoder}.onnx`（リビジョン固定）。無ければ動画機能ごと無効（起動ログに理由）
- プレビューは回数を消費しない。ジョブの持ち主の `client-id` だけ、`Created` のときだけ使える。重い処理は `spawn_blocking`
- 特徴量はジョブと同じ寿命（ジョブの削除・期限切れで消す）。1 ジョブ約 4MB
- 既存の約束事（CLAUDE.md）を守る。エラーは `common/macros.rs`、trait と実装はファイルを分ける、日本語のコメント

## Review Focus

1. フレームを登録せずにプレビューを呼ぶ → 409 `PREVIEW_FRAME_NOT_FOUND`
2. 送信（start）した後にプレビューを呼ぶ → 409 `VIDEO_JOB_INVALID_STATE`
3. 他人の client-id → 404
4. 点が 0 個 → 400 `INVALID_POINTS`（9 個以上も 400）
5. 大きすぎる画像（長辺 > 2000 や 10MB 超）→ 400

---

### Task P1: Rust のプレビュー推論とユースケースと API

**Files:**
- Create: `src/video_cutout/preview/{mod,segmenter,segmenter_tiny,feature_store,error}.rs`
- Create: `src/video_cutout/{set_preview_frame,preview_mask}/{mod,input,output,error,usecase}.rs`
- Create: `src/api/handler/video_cutout/{set_preview_frame,preview_mask}/{mod,handler,request,response}.rs`
- Modify: `src/composition/video_wiring.rs`, `src/presentation/router.rs`（利用ログ対象外の Router に入れる）, `src/application/error.rs`, `src/video_cutout/config.rs`（モデルの有無）, `src/main.rs`, `src/video_cutout/delete_expired_jobs/usecase.rs`（特徴量も消す）, `CLAUDE.md`, `docs/ARCHITECTURE.md` §9

**Interfaces:**
- `PreviewSegmenter: Send + Sync { fn encode(&self, frame: &RgbImage) -> Result<PreviewFeatures, PreviewError>; fn decode(&self, features: &PreviewFeatures, points: &Points) -> Result<Array2<f32> /*512x512 logits*/, PreviewError>; }`
- `Sam2TinyPreviewSegmenter::new(model_dir: &Path) -> Result<Self, PreviewError>`
- `PreviewFeatureStore`: `save(job_id, Arc<PreviewFeatures>)`, `get(job_id) -> Option<Arc<..>>`, `remove(job_id)`
- `POST /video-jobs/{id}/preview-frame`（multipart、フィールド `file`、PNG/JPEG、10MB 以下、長辺 ≤ 2000）→ 204
- `POST /video-jobs/{id}/preview` 本文 `{points:[{x,y,label}]}` → 200 `image/png`（グレースケール 8bit の alpha マスク。大きさは登録したフレームと同じ）
- エラーコード: `PREVIEW_FRAME_NOT_FOUND`（409）, `INVALID_PREVIEW_IMAGE`（400）, `PREVIEW_FAILED`（500）

- [ ] 前処理・点の変換・alpha 変換の純粋関数をテストから書く（固定値の入力 → 期待値。Python の式と同じ数値になることを手計算の小さな例で確認）
- [ ] 偽の `PreviewSegmenter` でユースケースと Router のテスト（Review Focus 1〜5、成功時は PNG を返す、回数を消費しない）
- [ ] 実装（ort の読み込みは `segmenter_sam2.rs` の書き方に合わせる）
- [ ] `cargo test` と `cargo check`（警告 15 件のまま）
- [ ] コミット

### Task P2: フロントエンドの切り替え・モデル取得スクリプト・手順書

**Files:**
- Modify: `frontend/js/video/preview.js`, `frontend/js/video/page.js`, `frontend/js/api.js`, `frontend/README.md`
- Create: `worker/scripts/fetch_preview_models.py`（`cutout.models.fetch_models` を使い、`vision_encoder` と `mask_decoder` と `constants.json` だけを `models/sam2.1-tiny-video/` に取得）
- Modify: `docs/video-cutout-deploy.md`（モデルの取得手順とデプロイ）, 仕様書 D8 の更新

- [ ] プレビューを新しい 2 つの API に切り替える（点の一覧は手元で管理し、取り消し・やり直しは一覧を送り直す。マスクをフレームの上に重ねて表示）
- [ ] selftest と Playwright（API は偽の応答）で確認
- [ ] モデル取得スクリプトと手順書
- [ ] コミット
