# CLAUDE.md — Goodbye Background

SAM2（Segment Anything 2.1, hiera_small）で画像の背景を消す Web アプリ。
ユーザーが画像上をクリック（前景/背景のポイント）→ サーバーが SAM2 で推論 → 切り抜き結果を返す。

- バックエンド: Rust (edition 2024) / axum 0.8 / tokio / ort (ONNX Runtime) / ndarray / rayon
- フロントエンド: ビルド不要の Vanilla JS（`frontend/`）。axum が同一オリジンで静的配信する
- 画像データは **全てインメモリ**（DB には入らない）。DB は利用ログ（usage_records）専用

詳細設計（座標系・データの寿命・処理シーケンス・既知の問題）は **`docs/ARCHITECTURE.md`**、
フロントエンドの設計は **`frontend/README.md`** を参照。

## コマンド

```bash
# 起動（リポジトリ直下で実行すること。models/ と frontend/ を相対パスで読む）
DATABASE_URL_SQLITE=sqlite://database/usage.db?mode=rwc cargo run            # SQLite（デフォルト）
DATABASE_URL_POSTGRES=postgres://... cargo run -- --postgres                  # PostgreSQL

# テスト / 型チェック
cargo test
cargo check --all-targets

# ネットワーク制限下（ort-sys が ONNX Runtime バイナリを DL できない）で型チェックだけしたい場合
ORT_SKIP_DOWNLOAD=1 cargo check --all-targets

# Docker
docker compose up --build
```

- `models/` は git 管理外。起動には以下が必要（無いと `App::new` で panic）:
  `sam2.1_hiera_small_{image_encoder,prompt_encoder,mask_decoder}.onnx`, `image_pe.bin`
  → `scripts/` の Python（SAM2 公式リポジトリを `SAM2/` に置いて実行）で生成する
- 動画切り抜きのプレビューには `models/sam2.1-small-video/`（`constants.json`, `onnx/{vision_encoder,mask_decoder}.onnx`）も必要。
  無いと動画機能ごと無効（503。起動ログに足りないファイル名が出る）。画像機能は動く
- `cargo test` は ONNX Runtime のリンクが必要。テストは models/ 無しでも動く（Sam2Segmenter::new を呼ばない）。
  ort-sys のダウンロードが通らない環境では、PyPI の `onnxruntime==1.24.2` の wheel から
  `libonnxruntime.so.1.24.2` を取り出し、`ORT_LIB_LOCATION=<dir> ORT_PREFER_DYNAMIC_LINK=1
  LD_LIBRARY_PATH=<dir> cargo test` で動的リンクできる
- 現状 `cargo check` で 15 件の既存警告（未使用関数・ライフタイム省略など）が出るのが正常

## ディレクトリ構成（src/）

```
main.rs              起動・DB 接続・GC タスク・axum serve
config.rs            定数（MAX_HISTORY=40, CLEANUP_INTERVAL=60s, TTL_SEC=10min）
composition/wiring.rs  ★ DI。全オブジェクトの生成と配線。App = ハンドラから見たファサード
composition/video_wiring.rs  動画切り抜きの DI。VideoCutoutApp = 動画のハンドラから見たファサード
presentation/        router（ルート定義）, AppError→HTTP 変換, 利用ログミドルウェア, client_ip（回数制限用の IP）
api/handler/image_cutout/<ep>/  画像切り抜きのエンドポイントごとの handler.rs / request.rs / response.rs
api/handler/video_cutout/<ep>/  動画切り抜きのエンドポイント（create_job / start_job / get_job / report_event / set_preview_frame / preview_mask）
video_cutout/        動画切り抜き機能（ジョブ・回数制限・R2/Modal のポート・プレビュー（preview/。SAM2.1 small）・ユースケース・config.rs）
image_cutout/        画像切り抜き機能（既存の画像機能は全てここ）
  image_io/          ユースケース: upload / get_preview / get_completed
  editing/           ユースケース: prepare_segment / segment / undo / redo / save
    segmenter/       ★ SAM2 推論（segmenter_sam2.rs）と前後処理（mask_resizer/smoother/applier）
    session/         編集セッション（SAM2 特徴量 + ポイント履歴 + マスク履歴）とセッション単位のロック
  image/             画像ドメイン: original / segmenter_input / preview / completed
  point/             クリック座標・ラベル・undo/redo 履歴
  session/           セッション（session_id → 元画像 image_id の対応）
maintenance/         TtlStore（全ストアの実体）と期限切れ削除ユースケース
migrations/{sqlite,postgres}/  利用ログテーブルのマイグレーション（起動時に自動適用。src の外）
analytics/           利用ログの記録（mpsc チャネル → バックグラウンドで INSERT）
common/              エラー定義マクロ（leaf_error! / leaf_detail_error! / parent_error!）
```

## 設計の約束事（コードを変更するときに守ること）

- **レイヤー**: handler → `App`（wiring.rs）→ UseCase → Service → Repository/Storage/Generator。
  ハンドラはユースケースを直接触らず `App` のメソッドだけを呼ぶ
- **1 ユースケース = 1 ディレクトリ**: `input.rs` / `output.rs` / `error.rs` / `usecase.rs` / `mod.rs`
- **trait とその実装はファイルを分ける**: 通常は `xxx.rs` が trait、`xxx_impl.rs` が実装。
  例外: `image_cutout/image/completed/generator/` だけ逆（`generator.rs` が実装、`generator_impl.rs` が trait）
- **依存は `Arc<dyn Trait>`** で注入する。ストアは `Arc` で共有されている（wiring.rs で確認）
- **エラー**: 必ず `common/macros.rs` のマクロで定義する。HTTP ステータスと返すコード文字列は
  末端（leaf）で決まり、親エラーは委譲するだけ。新しいユースケースを足したら
  `application/error.rs` の `ApplicationError` にも variant を追加する
- **重い処理は `spawn_blocking`**: segment/undo/redo/save/upload ハンドラはそうしている。新規 API も同様に。
  **例外は動画**: 動画のポート（R2・Modal・回数の DB）はネットワーク待ちだけの非同期 trait なので、
  動画のハンドラは `spawn_blocking` を使わずそのまま await する（ブロッキング用スレッドを待ちで占有しないため）。
  ただし動画のプレビューの推論（CPU）はユースケースの中で `spawn_blocking` する
- **動画のファサードは `VideoCutoutApp`**（`composition/video_wiring.rs`）。画像の `App` には動画のメソッドを足さない。
  `App::new` は SAM2 のモデルを読むので、動画をモデル無しで組み立て・テストできるよう分けている（設計判断 R4）。
  ルーターのテストは `VideoCutoutApp::for_test`（偽の R2/ワーカー/回数/時計）で組む
- **新しい API ルートを追加したら**: `presentation/router.rs`、`analytics/usage_tracking/operation.rs`、
  `analytics/usage_tracking/recorder.rs`（2箇所の文字列変換）、`frontend/js/api.js` を更新
- **動画のルートを追加したら**: `presentation/router.rs` の `video_routes` に足す。利用ログに記録するものは
  1 つ目の Router（`track_usage` の前に merge される）に入れて `operation.rs` / `recorder.rs` も更新、
  記録しないもの（ポーリング・内部の通知）は 2 つ目の Router に入れる。ハンドラは `api/handler/video_cutout/<ep>/`、
  メソッドは `VideoCutoutApp` に足し、先頭で `maybe_cleanup()` を呼ぶ
- **ストアの値は不変**: `get` で `Arc<V>` を取り、clone して変更し、`save/insert` で丸ごと置き換える
- テストは各ファイル末尾の `#[cfg(test)] mod xxx_test`。時刻が絡むものは `FakeClock` を使う
- 既存コメント・ドキュメントは日本語。追記も日本語で揃える

## 必ず押さえておくべき前提（間違えやすい所）

1. **座標系が3つある**: フロントが送る `x,y` は「プレビュー画像（長辺600px）」座標 →
   `CommonEditingSession` で `÷ point_scale` して「元画像」座標で保存 →
   `Sam2Segmenter::_scale_prompt` で「1024x1024」座標にして SAM2 へ
2. **`preview_to_original_scale` という名前は逆**: 実際は `preview = original × scale`（= 600 / 長辺）
3. **image_id はバージョン**: segment/undo/redo のたびに新しい ImageId が振られ、その ID で
   プレビューが保存される。session_id は編集セッションのハンドルで不変
4. **`/save` の image_id は `/get-completed` 専用**（プレビューストアには入らない）
5. **`ImageSize::new(height, width)`** — 高さが先
6. **TTL は「最後にアクセスしてから 10 分」**（`get` するたびに延長）。サーバー再起動で全データ消失
7. **SAM2 の ONNX セッションは Mutex 1本** → 全ユーザーの推論が直列化される。
   これは意図的（オーナー判断: 利用者が少ないうちはメモリを増やさない）。プール化はしないこと
8. **マスクはロジット**: mask decoder の出力・履歴に保存する前回マスクはロジットのまま。
   画像に適用する直前（リサイズ後）にだけ `SAM2MaskActivation::sigmoid` で確率にする
9. **ストアを追加したら GC に登録**: `wiring.rs` の `DeleteExpiredEntriesUseCase::new` に渡さないとリークする
10. **編集セッションを変更する処理は `EditingSessionLocks` で囲む**: 取得→変更→保存の間にロックを持つ。
    ロック順は「セッションロック → ONNX の Mutex」のみ
11. **segment/undo/redo の切り抜きはプレビュー解像度**: `PreviewImageGenerator::resize_to_preview` で
    縮小した元画像を `segment_fast` の `output_image` に渡す。フル解像度で処理するのは save（resegment）だけ。
    座標変換には `original_image` のサイズを使うので、2つを取り違えないこと
12. **保存用 WebP は quality 95 / method 2**（オーナー判断: 速度とファイルサイズの中間）。
    変えるときは `image_cutout/image/completed/generator/generator.rs` の計測表を参照
13. **マイグレーションは DB ごと**: `migrations/sqlite/` と `migrations/postgres/` の両方に追加する。
    適用済みのファイルは絶対に書き換えない（sqlx のチェックサム不一致で起動しなくなる）。
    本番 DB は Google ログインのブランチ（feat/user-google-login）の平らな `migrations/` で作られたので、
    `postgres/20260919032531`・`20261004122625`・`20261101090000` は本番が適用したバイト列そのもの
    （改行が混在: CRLF / CRLF / LF。オーナーが Windows の autocrlf でビルドしたため）。絶対に編集しない（テストで固定）。
    `.gitattributes`（`migrations/** -text`）で migrations の改行変換を止めているので、エディタで改行を変えないこと。
    `migrations/sqlite/` は開発用で LF のまま
14. **Cloud Run の最大インスタンス数は 1 のまま変えない**: 動画ジョブはインメモリなので、2 台以上にすると
    ポーリングやワーカーの通知が別の台に届いて 404 になる（docs/ARCHITECTURE.md §9）
15. **`TRUSTED_PROXY_HOPS`**: 回数制限の IP を `X-Forwarded-For` の右から何番目で取るか（`presentation/client_ip.rs`）。
    0 = 接続元。Cloud Run で 0 のままだと全員が同じ IP になり、全員で 1 日 3 回になる。**本番の値は 1**（2026-11-08 に確認。
    Google の入口が右端に接続元を書き足し、偽の値は左に残るだけ。前段の中継は無い）。
    回数の IP のキーは、IPv4 はアドレス、**IPv6 は /64**（`quota/daily_quota.rs` の `ip_quota_key`。スマホは /64 の中でアドレスを使い捨てるため）。
    確かめる間だけ `LOG_CLIENT_IP_DEBUG=1` を付けると `POST /video-jobs` ごとに `[client-ip-debug]` の行
    （生の `X-Forwarded-For`・接続元・hops・選ばれた IP）が出る。**確認が終わったら外す**（環境変数を消す。手順は `docs/video-cutout-deploy.md`）
16. **動画のモデルは SAM2.1 small（動画用 ONNX、入力 1024）。GPU ワーカーと Rust（CPU）のプレビューで同じものを使う**
    （2026-11-08 に tiny → small。オーナー判断。docs/video-cutout-measurements.md の案 C、docs/ARCHITECTURE.md §9.4）。
    モデルのリビジョンは `worker/cutout/models.py` の `MODEL_REVISION` 1 か所。プレビューとワーカーでモデルが違うと
    選ぶ範囲が食い違う（以前、画像の small と本番の tiny で起きた）。`video_cutout/preview/` の前処理・alpha は
    `worker/cutout/tracker.py`・`compositor.py` と同じ数値になるよう書いてある（EDGE_SHARPNESS などを変えるときは両方直す）。
    入力の大きさは両方とも `constants.json` の `image_size` から決める（512 の決め打ちはしない）。
    ワーカーの記憶の選び方は **DAM4SAM**（`worker/cutout/tracker_variants.py` の `PRODUCTION_POLICY`）。
    2 枚目以降の話なので、プレビュー（1 枚目）は SAM2 そのままの計算でワーカーと一致する（マスクの IoU 0.9999 を確認）。
    特徴量はジョブごとに約 8MB（f16）をメモリに置き、開始・削除・期限切れで消す。最大 16 件（LRU。約 128MB）、
    フレームの登録は同時 2 件まで（超えたら 503 `PREVIEW_BUSY`）

## 既知の問題（修正タスクの候補。詳細と根拠は docs/ARCHITECTURE.md）

- 🟡 rule.txt（3000px / 30分 / 保存後10分で削除）と実装（8000px / 10分 / 保存後の特別扱い無し）が不一致
- 🟡 `frontend/README.md` は `/get-preview` が `image/png` を返すと書いているが、実際は `image/webp`
- 🟡 `image_pe.bin` のパスだけ `model_dir` を無視してハードコード
- 🟡 ログが全て `println!`（ポイント座標などリクエストごとに大量に出る）
