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
- `cargo test` は ONNX Runtime のリンクが必要。テストは models/ 無しでも動く（Sam2Segmenter::new を呼ばない）
- 現状 `cargo check` で 15 件の既存警告（未使用関数・ライフタイム省略など）が出るのが正常

## ディレクトリ構成（src/）

```
main.rs              起動・DB 接続・GC タスク・axum serve
config.rs            定数（MAX_HISTORY=40, CLEANUP_INTERVAL=60s, TTL_SEC=10min）
composition/wiring.rs  ★ DI。全オブジェクトの生成と配線。App = ハンドラから見たファサード
presentation/        router（ルート定義）, AppError→HTTP 変換, 利用ログミドルウェア
api/handler/<ep>/    エンドポイントごとの handler.rs / request.rs / response.rs
image_io/            ユースケース: upload / get_preview / get_completed
editing/             ユースケース: prepare_segment / segment / undo / redo / save
  segmenter/         ★ SAM2 推論（segmenter_sam2.rs）と前後処理（mask_resizer/smoother/applier）
  session/           編集セッション（SAM2 特徴量 + ポイント履歴 + マスク履歴）
image/               画像ドメイン: original / segmenter_input / preview / completed
point/               クリック座標・ラベル・undo/redo 履歴
session/             セッション（session_id → 元画像 image_id の対応）
maintenance/         TtlStore（全ストアの実体）と期限切れ削除ユースケース
analytics/           利用ログの記録（mpsc チャネル → バックグラウンドで INSERT）
common/              エラー定義マクロ（leaf_error! / leaf_detail_error! / parent_error!）
```

## 設計の約束事（コードを変更するときに守ること）

- **レイヤー**: handler → `App`（wiring.rs）→ UseCase → Service → Repository/Storage/Generator。
  ハンドラはユースケースを直接触らず `App` のメソッドだけを呼ぶ
- **1 ユースケース = 1 ディレクトリ**: `input.rs` / `output.rs` / `error.rs` / `usecase.rs` / `mod.rs`
- **trait とその実装はファイルを分ける**: 通常は `xxx.rs` が trait、`xxx_impl.rs` が実装。
  例外: `image/completed/generator/` だけ逆（`generator.rs` が実装、`generator_impl.rs` が trait）
- **依存は `Arc<dyn Trait>`** で注入する。ストアは `Arc` で共有されている（wiring.rs で確認）
- **エラー**: 必ず `common/macros.rs` のマクロで定義する。HTTP ステータスと返すコード文字列は
  末端（leaf）で決まり、親エラーは委譲するだけ。新しいユースケースを足したら
  `application/error.rs` の `ApplicationError` にも variant を追加する
- **重い処理は `spawn_blocking`**: segment/undo/redo/save/upload ハンドラはそうしている。新規 API も同様に
- **新しい API ルートを追加したら**: `presentation/router.rs`、`analytics/usage_tracking/operation.rs`、
  `analytics/usage_tracking/recorder.rs`（2箇所の文字列変換）、`frontend/js/api.js` を更新
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
7. **SAM2 の ONNX セッションは Mutex 1本** → 全ユーザーの推論が直列化される

## 既知の問題（修正タスクの候補。詳細と根拠は docs/ARCHITECTURE.md）

- 🔴 `SAM2EditingSessionRepository` が GC 対象に登録されておらず、1セッション約16MBがリークし続ける
- 🔴 mask decoder の出力はロジットなのに、mask_applier / mask_smoother は 0〜1 の確率として扱っている
- 🟠 同一セッションへの並行リクエストで lost update が起きる（サーバー側にロックが無い）
- 🟠 Postgres モードではマイグレーションが実行されない。SQLite 用マイグレーションが Postgres 専用型
  （BIGSERIAL / TIMESTAMPTZ）を使っており、SQLite では `id` が自動採番されない
- 🟠 65535px 超の画像で `as u16` が回り込み、サイズ検証をすり抜ける
- 🟡 rule.txt（3000px / 30分 / 保存後10分で削除）と実装（8000px / 10分 / 保存後の特別扱い無し）が不一致
- 🟡 `frontend/README.md` は `/get-preview` が `image/png` を返すと書いているが、実際は `image/webp`
- 🟡 `image_pe.bin` のパスだけ `model_dir` を無視してハードコード
- 🟡 ログが全て `println!`（ポイント座標などリクエストごとに大量に出る）
