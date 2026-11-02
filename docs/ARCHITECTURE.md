# Goodbye Background アーキテクチャ詳細

`CLAUDE.md` の補足。コードを読む前の地図として使う。各ファイル先頭の `//!` コメントにも同じ粒度の説明がある。

---

## 1. 全体像

```
ブラウザ (frontend/*.html, js/*.js)
   │  POST /upload, /segment, ...（JSON or multipart, header: client-id）
   ▼
axum Router (presentation/router.rs)
   ├─ middleware: track_usage ──────────────► UsageRecorder ─(mpsc)─► SQLite / Postgres
   ├─ api/handler/*  (リクエスト → Input 変換, spawn_blocking)
   │     ▼
   │  App (composition/wiring.rs)  … 全ユースケースのファサード
   │     ▼
   │  UseCase (image_io/*, editing/*)
   │     ▼
   │  Service (PreviewService, SegmentService, PrepareSegmentService, CompletedService)
   │     ▼
   │  Repository / Storage (全て TtlStore = DashMap + 最終アクセス時刻)
   │  Generator (WebP エンコード, リサイズ) / Sam2Segmenter (ONNX Runtime)
   └─ fallback: ServeDir("frontend")
```

バックグラウンドタスク:
- GC: 60 秒ごとに `App::cleanup` → `DeleteExpiredEntriesUseCase` → 各ストアの `delete_expired`
- 利用ログ: `UsageRecorder` のチャネルを受信して INSERT

---

## 2. インメモリストア一覧

全て `maintenance::ttl_store::TtlStore`。キーへのアクセス（`get`/`insert`）で寿命が 10 分延長される。

| ストア | キー | 値 | 1件のサイズ目安 | GC 対象 |
|---|---|---|---|---|
| `SessionRepositoryInMemory` | SessionId | Session（→元画像 ImageId） | 数十B | ✅ |
| `OriginalImageRepositoryInMemory` | ImageId | 元画像 生RGBA | 幅×高×4 B（4000x3000 で 48MB） | ✅ |
| `SegmenterInputStorageInMemory` | ImageId（元画像と同じ） | 1024x1024 生RGBA | 4MB | ✅ |
| `SAM2EditingSessionRepository` | SessionId | 特徴量 + ポイント履歴 + マスク履歴 | 約16MB + 256KB×履歴数 | ❌ **登録漏れ** |
| `PreviewStorageInMemory`（2系統） | ImageId | 長辺600px WebP | 数十KB | ✅ |
| `CompletedRepositoryInMemory` | ImageId | 元サイズ WebP q95 | 数百KB〜数MB | ✅ |

---

## 3. 座標系（最重要）

| 座標系 | どこで使われるか | 変換 |
|---|---|---|
| プレビュー座標 | フロントがクリック位置として送る `x, y`（u16） | — |
| 元画像座標 | `PointHistory` に保存される値 | プレビュー ÷ `point_scale` |
| SAM2 座標 (1024x1024, f32) | prompt encoder への入力 | 元画像 × (1024 / 元の幅or高さ)（縦横別） |
| マスク座標 (256x256) | mask decoder の出力 | `mask_resizer` で元画像サイズへバイリニア補間 |

- `point_scale` = `600 / max(元画像の幅, 高さ)`。`WebPPreviewImageGenerator::generate` が返し、
  `UploadOutput` → `PrepareSegmentInput` → `CommonEditingSession.point_scale` と受け渡される
- 変数名 `preview_to_original_scale` は意味が逆（実際は original → preview の倍率）なので注意
- `Coordinate` の乗除算は `as u16` で切り捨て。範囲外の座標のバリデーションは無い
- 入力画像もポイントも「アスペクト比を無視して 1024x1024 に引き伸ばす」方式で揃えている（SAM2 公式と同じ）

---

## 4. 処理シーケンス

### 4.1 `/upload`
```
handler(upload) ── multipart "file" を読む ── spawn_blocking ──► App::upload
  UploadUseCase
    1. FileImageLoader::load        バイト列 → 形式推測 → デコード → EXIF回転 → RGBA8 → ImageSize検証(2..=8000)
    2. SessionId::new, Session 保存
    3. PreviewService::generate_and_save_original  → (プレビューWebP保存, scale)
    4. OriginalImage 保存
  PrepareSegmentUseCase → PrepareSegmentServiceImpl::prepare
    5. 元画像 → 1024x1024 にリサイズ（SAM2InputGenerator）
    6. image encoder 実行 → SAM2StaticContext（重い）
    7. CommonEditingSession(空のポイント履歴, 特徴量, マスク履歴[None], scale) を保存
    8. 1024x1024 画像を保存
← { session_id, image_id(元画像プレビュー) }
```

### 4.2 `/segment`
```
handler ── {session_id, x, y, is_foreground} → Point(プレビュー座標) ── spawn_blocking
  SegmentUseCase
    SegmentServiceImpl::segment
      1. Session → 元画像 ImageId
      2. 元画像 / 入力画像 / 編集セッション を取得、編集セッションを clone
      3. points_with(point)   既存点 + 新点(÷scale で元画像座標に)
      4. Sam2Segmenter::segment_fast
           _inference: 座標を1024系に → prompt encoder(点, 前回マスク) → mask decoder → Mask(256², ロジット)
           _generate_image_fast: resize → apply(アルファ書き込み)
      5. apply_edit(point, 新マスク) で両履歴に追加 → 編集セッション保存
      6. 新しい ImageId を振った RGBA Image を返す
    PreviewService::generate_and_save_segmented   600px WebP にしてプレビューストアへ
← { image_id(新しいプレビュー) }
```

### 4.3 `/undo` `/redo`
- 編集セッションの両履歴を1つ戻す/進める → 保存 → その時点のポイント列 + その時点のマスクで再推論（fast）
- 新しいマスクは履歴に **保存しない**
- undo でポイントが 0 になったら推論せず元画像（アルファ不変）を返す
- できないときは `UNDO_FAILED` / `REDO_FAILED`（400）

### 4.4 `/save` → `/get-completed`
- `resegment`: 履歴を変えずに現在のポイントで推論し、`segment_high_quality`（ガイデッドフィルタで平滑化）
- `CompletedService` が元サイズのまま WebP(q95) にして保存。返した image_id で `/get-completed` から取得
- 0 点で保存した場合は元画像をそのまま WebP 化（返す image_id は元画像と同じ）

### 4.5 `/get-preview`
- image_id でプレビューストア（元画像用 / 切り抜き用の両方）を引いて WebP を返す

---

## 5. エラー処理

```
leaf_error! / leaf_detail_error!         … 例: SessionRepositoryError::SessionNotFound => ("SESSION_NOT_FOUND", NotFound)
   ▼ parent_error!（From 自動生成）
SegmentServiceError / PrepareServiceError
   ▼
SegmentUseCaseError / UploadUseCaseError / ...
   ▼
ApplicationError                          … application/error.rs
   ▼ + PresentationError（ID パース失敗, multipart, spawn_blocking の JoinError）
AppError::into_response                   … ステータス = ErrorType, ボディ = {"code": "..."}
```

| ErrorType | HTTP |
|---|---|
| InvalidInput | 400 |
| NotFound | 404 |
| Conflict | 409 |
| Internal | 500 |

- クライアントには `code` だけ返し、詳細（`cause`）は `eprintln!` でサーバーログへ
- `spawn_blocking` 内の panic は `BLOCKING_TASK_FAILED`（500）になる
- axum 自身のエラー（JSON パース失敗 422、ボディ 10MB 超過 413）は `AppError` を通らない
  → 利用ログ上は「成功」として記録される

主なエラーコード: `INVALID_SESSION_ID`, `INVALID_IMAGE_ID`, `SESSION_NOT_FOUND`, `IMAGE_NOT_FOUND`,
`EDITING_SESSION_NOT_FOUND`, `UNSUPPORTED_FORMAT`, `CORRUPTED_IMAGE`, `LONG_HEIGHT`, `LONG_WIDTH`,
`SHORT_HEIGHT`, `SHORT_WIDTH`, `UNDO_FAILED`, `REDO_FAILED`, `HISTORY_CORRUPTED`, `INFERENCE_ERROR`,
`PREPROCESS_ERROR`, `AMBIGUOUS_ID`, `MULTIPART_ERROR`, `UNKNOWN_TAG_NAME`, `NO_TAG_NAME_ERROR`,
`BLOCKING_TASK_FAILED`

---

## 6. undo/redo 履歴の仕組み

`PointHistory`（ポイント）と `InferenceContextHistory`（マスク）を常にセットで操作する。

```
初期      Point: items=[]          cursor=0   Ctx: items=[None]        cursor=0
+p0       Point: items=[p0]        cursor=1   Ctx: items=[None,m0]     cursor=1
+p1       Point: items=[p0,p1]     cursor=2   Ctx: items=[None,m0,m1]  cursor=2
undo      Point: current=[p0]      cursor=1   Ctx: current=m0          cursor=1
+p2       Point: items=[p0,p2]     cursor=2   Ctx: items=[None,m0,m2]  cursor=2   ← redo 履歴(p1,m1)は破棄
```

- `cursor` の値は両者で一致する。`can_undo/can_redo` が食い違ったら `HISTORY_CORRUPTED`
- undo 可能回数が `MAX_HISTORY`(40) を超えると `base_len` を進めて古い点を「確定」する
  （配列からは消えず SAM2 には渡され続ける。undo で戻れなくなるだけ）
- 配列自体は縮まないので、点を打ち続けると履歴（マスク 256KB/個）は増え続ける

---

## 7. SAM2 / ONNX

| モデル | 入力 | 出力 |
|---|---|---|
| image encoder | image [1,3,1024,1024]（0〜1 RGB。正規化はモデル内） | image_embeddings [1,256,64,64], high_res_feature_s0 [1,32,256,256], high_res_feature_s1 [1,64,128,128] |
| prompt encoder | point_coords [1,N,2], point_labels [1,N], mask [1,1,256,256], has_mask [1] | sparse_embeddings [1,N+1,256], dense_embeddings [1,256,64,64] |
| mask decoder | 上記 + image_pe [1,256,64,64] | masks [1,1,256,256]（ロジット）, iou_pred 等（未使用） |

- エクスポート: `scripts/image_encoder_export.py`, `scripts/prompt_encoder_export.py`, `scripts/export.py`（mask decoder）
- `scripts/image_pe_generate.py`: image_pe を f32 生バイナリ `image_pe.bin` に変換
- `scripts/verify_iou.py`: 公式 SAM2ImagePredictor と ONNX パイプラインの出力を IoU で比較
- 各 ONNX セッションは `Arc<Mutex<Session>>` で1つだけ → 推論は全体で直列化

---

## 8. 既知の問題と根拠

コメント追加時点（2026-11）でコードを読んで判明したもの。修正はしていない。

### 🔴 重大
1. **編集セッションのメモリリーク**
   `composition/wiring.rs` で `DeleteExpiredEntriesUseCase::new` に `editing_session_repo` を渡していない。
   `SAM2EditingSessionRepository` は `DeleteExpiredRepository` を実装しているのに呼ばれないので、
   アップロードのたびに約16MB（特徴量）+ マスク履歴が永久に残る。
   → `ExpiringEditingSessionRepository` を追加して GC 対象に含める。

2. **マスクのロジットを確率として扱っている**
   `scripts/export.py` の `MaskDecoderWrapper` は sigmoid を掛けずに `mask_decoder` の出力をそのまま返す
   （SAM2 の仕様上ロジット。閾値は 0）。一方 `mask_applier.rs` は `alpha = clamp(mask*255)`、
   `mask_smoother.rs` は `(p - 0.5).abs()` など 0〜1 前提の式を使っている。
   結果としてアルファはほぼ「ロジット > 0 で二値化」になり、平滑化のブレンド係数は発散している。
   → resize 後（または applier の直前）に sigmoid を適用し、テストも確率前提で書き直す。
   ※ ただし prompt encoder に渡す「前回マスク」はロジットのままである必要がある。

### 🟠 中
3. **同一セッションの並行更新**: `SegmentServiceImpl` は「取得 → clone → 変更 → 保存」をロック無しで行う。
   同じ session_id へ同時に segment が来ると後勝ちで片方の点が消える。フロントの `busy` ロック頼み。
4. **DB マイグレーション**: `main.rs` は SQLite のときだけ `sqlx::migrate!` を実行し、Postgres では実行しない。
   さらにマイグレーション SQL が `BIGSERIAL` / `TIMESTAMPTZ` を使っており、SQLite では
   `id` が rowid の別名にならず NULL のまま入る。`.env.example` の URL（`sql://...`）もそのままでは使えない。
5. **巨大画像の u16 回り込み**: `loader_impl.rs` で `width as u16` を検証前に行うので 65535px 超が素通りする。
6. **推論の直列化**: ONNX セッションが Mutex 1本なので、同時利用者が増えると待ち時間が線形に伸びる。

### 🟡 小
7. 仕様（`rule.txt`）と実装の差: 最大 3000px ↔ 8000px、寿命 30 分 ↔ 10 分、保存後 10 分で削除 ↔ 特別扱い無し。
8. `frontend/README.md` の API 表で `/get-preview` が `image/png` となっているが実際は `image/webp`。
9. `Sam2Segmenter::new(model_dir)` なのに `image_pe.bin` だけ `"models/image_pe.bin"` 固定。
10. `image/completed/generator/` の trait/実装ファイル名が他と逆。
11. `RedoUseCaseError::SegmenterInputStorage` は生成されない死んだ variant。
12. `App::new` は `Result` を返すが、失敗は全て `expect` で panic する。
13. ログが `println!` / `eprintln!`（リクエストごとに座標や処理時間が出る）。`tracing` への移行を推奨。
14. `SQLiteUsageRecorder` と `PostgresUsageRecorder` がプレースホルダ以外同一の重複コード。
15. `desktop.ini`（OneDrive の設定ファイル）がコミットされている。
