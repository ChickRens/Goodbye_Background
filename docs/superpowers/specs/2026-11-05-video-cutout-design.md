# 動画切り抜き（MVP）設計書

- 日付: 2026-11-05
- 状態: レビュー待ち
- 対象リポジトリ: `ChickRens/Goodbye_Background`（Rust 本体 + 新規 `worker/`）
- 参照（使い捨てのスパイク。本番コードには流用しない）:
  - `ChickRens/goodbye-background-tmp` … `bench.py`（トラッキングループの Python 実装）, `convert.py`, `sam2_gpu_bench.ipynb`, `data/out/report.txt`
  - `ChickRens/movie-cutout` … `index.html`（ブラウザ内推論の JS 版ループ。不採用）

---

## 1. 目的と範囲

### 1.1 目的

ミーム動画の制作者が、スマホのブラウザから数秒〜10秒のクリップを送り、最初のフレームを1〜3回タップするだけで、
**そのまま素材として使えるグリーンバック MP4** を受け取れるようにする。競合（CapCut）に対する勝ち筋は「手間の少なさ」。

### 1.2 成功の基準

| 項目 | 基準 |
|---|---|
| 体験 | iPhone 13 / iOS 18 の Safari で、送信から受け取りまで 1 分以内（コールドスタート込み） |
| 原価 | 10 秒・30fps の動画 1 本あたり 1 円未満（GPU 代） |
| 月の請求 | Modal の無料クレジット（月 $30）以内。予算上限で最悪でも $30 を超えない |
| 品質 | 本番のトラッキングループが `bench.py` と IoU ≥ 0.99 で一致する |
| 素材として使えるか | CapCut のクロマキーで背景だけが抜け、ロゴは残る（緑・青・マゼンタの 3 色で） |

### 1.3 範囲

**含める（この仕様書）**

- 動画の選択 → 開始・終了位置の選択 → 開始フレームでのタップとプレビュー → ジョブ投入 → 進捗表示 → グリーンバック MP4 のダウンロード
- 背景色は緑・青・マゼンタから選ぶ
- 無料枠の制限（10 秒、1 日 3 回、ロゴ入り）
- 既存の画像切り抜きのコードを `src/image_cutout/` に移す（挙動は変えない）

**含めない（別の仕様書で扱う）**

- B: 透過動画（WebM VP9 アルファ、GIF、MOV）
- C: 課金・認証・有料版（ロゴなし・長尺・優先処理）
- 利用規約（著作権のある素材の扱い）
- PWA 化
- 端末側での合成（WebCodecs）
- 前後両方向のトラッキング（任意のフレームから前にも追う）
- ジョブ状態の DB 永続化

---

## 2. 決定事項の一覧

引き継ぎ時点で決まっていたこと（ターゲット、スマホ優先の Web、出力形式の方針、モデル
`diffusionstudio/sam2.1-tiny-video-onnx-fp16` を fp16 のまま使う、サーバーレス GPU、Rust は小さな CPU サーバーに常駐、
Python ワーカーはポートの向こう側、既存の画像用 trait に動画用メソッドを足さない、長時間処理はジョブ、ファイルは一時保存のみ）
に加えて、このブレインストーミングで以下を決めた。

| # | 論点 | 決定 | 主な理由 |
|---|---|---|---|
| D1 | 範囲 | MVP はグリーンバック MP4 だけ。B / C は別仕様書 | 合成場所と API が決まらないと B も C も設計できない |
| D2 | 合成の場所 | GPU ワーカーで推論・合成・エンコードまで行う | 端末エンコード（iOS 18 Safari）の未確認リスクを MVP から外す。差額は 1 本 +$0.001 程度 |
| D3 | ファイルの通り道 | Cloudflare R2 + 署名付き URL。Rust は動画の中身に触れない | 小さな CPU サーバーの帯域・メモリが動画の大きさに引っ張られない |
| D4 | fps | 全フレーム推論。上限 30fps（60fps は 30fps に落とす） | 間引き＋補間は IoU 0.73 まで崩れた（`report.txt`） |
| D5 | GPU 事業者 | Modal（L4）。中核は事業者に依存しない Python パッケージ、Modal は薄いアダプタ | 月 $30 の無料クレジットで月数千本まで実質無料 |
| D6 | 進捗の伝え方 | ワーカー → Rust に HMAC 署名付きで通知。スマホは Rust を 1 秒ごとにポーリング | iOS Safari でタブを裏に回しても復帰できる。Rust は Modal の API を知らない |
| D7 | ジョブ状態の保存 | ジョブはインメモリ（`TtlStore`）。再起動・0 台への縮小で消えるのを許容。**1 日の回数だけは既存の DB（本番は PostgreSQL）に保存** | ジョブ処理中はポーリングでインスタンスが生きている。回数は 0 台への縮小のたびにリセットされると制限にならない |
| D8 | プレビュー | 既存の画像切り抜き（CPU、SAM2.1 hiera_small）の `/upload` と `/segment` を使い回す | 追加原価ゼロで「対象の取り違え」を防げる。細部の不一致は許容し「目安」と表示 |
| D9 | タップするフレームと範囲 | 開始・終了位置を選ぶ画面を作り、開始位置のフレームでタップ。ワーカーは範囲だけ処理 | 10 秒上限への対応と開始フレームの選択を 1 画面で済ませる |
| D10 | 無料枠 | 上限 10 秒、1 日 3 回（IP と `client-id` の両方で数える）、ロゴ入り、Modal の月の予算上限 $30 | 1 人 1 日最大 30 秒 |
| D11 | コードの配置 | 同じリポジトリに `worker/` と `contracts/` を追加。Rust は `src/video_cutout/`、既存は `src/image_cutout/` に移す | 契約の変更を 1 コミットで両側直せる |
| D12 | 出力 | H.264 / yuv420p / MP4、長辺 1080px 以下、音声は範囲に合わせて残す（AAC） | どの編集アプリ・SNS でもそのまま読める |
| D13 | ロゴ | `assets/brand/logo-original.webp` を、黒背景ごと角丸の**不透明**バッジとして右下に入れる | 半透明や背景なしだと、クロマキーでロゴまで抜ける |
| D14 | 本番環境 | Cloud Run（`https://chickren.goodbye-background.com`、最小 0 台・最大 1 台・リクエストベースの CPU）を前提にする。最大 1 台は変えない | インメモリの状態を 1 台で共有するため（§4.9） |

---

## 3. 全体の流れ

```
スマホ                       Rust（CPU・常駐）                 R2                Modal ワーカー（GPU）
 │ 動画を選ぶ → 開始/終了を選ぶ
 │── POST /video-jobs ──────────►│ 回数の残りを確認（消費はしない）
 │   {size, type, start, end}    │ 状態=Created、15 分有効の PUT URL を発行
 │◄── {job_id, upload_url} ──────│
 │── PUT upload_url（XMLHttpRequest で進捗表示）──────────►│
 │ （並行して）開始位置のフレームを canvas で切り出す → 既存の /upload, /segment でプレビュー
 │── POST /video-jobs/{id}/start ►│ ロック内: 持ち主確認 → Created か → R2 に HEAD
 │   {points(0〜1), background}   │ → 回数を 1 消費 → Queued で保存 → ワーカーに投入
 │                                │── POST（入出力の署名付き URL・範囲・点・色・ロゴ有無・通知先）──►│ spawn して 202
 │◄── 202 ───────────────────────│                                                  │
 │                                │◄── 通知 downloading / tracking 0.45 / finishing / done（HMAC 署名付き）
 │── GET /video-jobs/{id}（1 秒ごと）►│
 │◄── {phase, progress, download_url?} │ Done のときだけ、その場で 10 分有効の GET URL を発行
 │── GET download_url ───────────────────────────────────►│
```

- 回数を消費するのは `/start` のとき。アップロードやプレビューだけでは減らない。
- 画面の表示: `Queued` は「準備中（GPU を起動しています）」、`Tracking` は「処理中 NN%」。コールドスタートは `Queued` の長さとして見える。
- `job_id` は推測できない UUID v4。作成時の `client-id` に紐付け、他の `client-id` からの `/start` と `GET` は 404。
- アップロードの上限 200MB は 2 段で守る: Rust が申告サイズで弾き、ワーカーがダウンロード前に実サイズを確認する
  （署名付き PUT だけでは大きさを強制しきれないため）。
- タップ点は**正規化座標（0〜1）**で送る。フロントでプレビュー座標をプレビュー画像の幅・高さで割る。
  ワーカーは 512×512 に引き伸ばしたフレームで x・y それぞれに 512 を掛ける（縦横比を無視した引き伸ばしなので縦横別に掛けるだけでよい）。
  既存の 3 つの座標系（プレビュー / 元画像 / 1024）とは混ざらない。
- 開始フレームは「指定時刻以降の最初のフレーム」と定義し、ブラウザも ffmpeg も同じ時刻（ミリ秒）を使う。±1 フレームのずれはトラッキングが吸収する。

---

## 4. Rust 側の設計

### 4.1 既存コードの移動（Task 0、挙動を変えない）

| `src/image_cutout/` に移す | 最上位に残す（共有） |
|---|---|
| `image_io/`, `editing/`, `image/`, `point/`, `session/` | `maintenance/`, `analytics/`, `common/`, `presentation/`, `composition/`, `application/`, `config.rs`, `api/` |

- `api/handler/` は `api/handler/image_cutout/` と `api/handler/video_cutout/` に分ける。
- `git mv` で履歴を残し、パスだけ直す。`cargo test` の結果が移動前と同じであることを確認する。
- `CLAUDE.md` と `docs/ARCHITECTURE.md` の構成図を更新する。
- 動画の作業とは別のコミット（できれば別の PR）にする。

### 4.2 構成

```
src/video_cutout/
  job/        VideoJob, VideoJobId, JobPhase, ClipRange, NormalizedPoint, Background
              video_job_repository.rs (trait) / video_job_repository_impl.rs (TtlStore)
              video_job_locks.rs
  quota/      daily_quota.rs (trait) / daily_quota_impl.rs（DashMap、日本時間の日付ごと。Clock を注入）
  storage/    video_object_storage.rs (trait) / r2_video_object_storage.rs（rusty-s3 で署名、reqwest で HEAD/DELETE）
  worker/     cutout_worker.rs (trait) / modal_cutout_worker.rs（reqwest で POST）
  create_job/ start_job/ get_job/ report_event/ delete_expired_jobs/
              ← 1 ユースケース = 1 ディレクトリ（input / output / error / usecase / mod）
```

- 既存の約束事（handler → `App` → UseCase → Service → Repository、`Arc<dyn Trait>` で注入、trait と実装はファイルを分ける、
  エラーは `common/macros.rs`、新ユースケースは `ApplicationError` に variant 追加、ストアは GC に登録）をすべて守る。
- Python の存在が現れるのは `modal_cutout_worker.rs` の 1 ファイルだけ。

### 4.3 ドメインの値

- `ClipRange { start_ms: u32, end_ms: u32 }`: `start_ms < end_ms` かつ `end_ms − start_ms ≤ 10_000`。
- `NormalizedPoint { x: f32, y: f32, label: Include | Exclude }`: x・y は 0.0〜1.0。1 ジョブ 1〜8 点。
- `Background`: `Green (0,255,0)` / `Blue (0,0,255)` / `Magenta (255,0,255)`。
- `JobPhase`: `Created → Queued → Downloading → Tracking(progress: 0.0〜1.0) → Finishing → Done`。どこからでも `Failed(FailureCode)`。
  - 逆向きの遷移と、進捗が減る通知は無視する（通知の順序入れ替わり対策）。
  - `Done` と `Failed` は最終状態。以後の通知は無視する。
- `VideoJob`: id、持ち主の `client-id`、作成時の IP、申告サイズ、Content-Type、`ClipRange`、`Background`、点、`JobPhase`、
  入力キー `video-jobs/{id}/input`、出力キー `video-jobs/{id}/output.mp4`、作成時刻、最後の通知時刻、回数を消費したかどうか。

### 4.4 ポート

| trait | メソッド | 実装 |
|---|---|---|
| `VideoObjectStorage` | `presign_put(key, content_type, ttl)`, `presign_get(key, ttl, download_filename)`, `head(key) -> Option<size>`, `delete(key)` | `R2VideoObjectStorage` |
| `CutoutWorker` | `dispatch(WorkerJobRequest)` | `ModalCutoutWorker` |
| `VideoJobRepository` | `get`, `save`, `remove`, `list_expired(now)` | `TtlStore` ベース |
| `DailyQuota` | `remaining(client_id, ip, today)`, `try_consume(...)`, `refund(...)` | sqlx（SQLite 版 / PostgreSQL 版。`recorder.rs` と同じ分け方）。非同期の trait |

- `VideoObjectStorage` と `CutoutWorker` はネットワーク待ちだけなので、`async-trait` で非同期の trait にする（`Arc<dyn Trait>` のまま使える）。
  `spawn_blocking` には載せない（ブロッキング用スレッドを待ちで占有しないため）。これは CLAUDE.md の
  「重い処理は spawn_blocking」の例外として、理由と一緒に CLAUDE.md に追記する。
- `TtlStore` には読み・変更・書き戻しをまとめる操作が無いので、`EditingSessionLocks` と同じ形の `VideoJobLocks` を作り、
  `StartVideoJob` / `ReportVideoJobEvent` / 期限処理は取得 → 変更 → 保存の間ロックを持つ。
  非同期処理（R2 の HEAD、ワーカー投入）をまたぐので、ロックは `tokio::sync::Mutex` を使う。
- GC への登録: ジョブのストアは `TtlStore::delete_expired`（最終アクセス基準）ではなく、状態ごとの期限（5.2）で
  `DeleteExpiredVideoJobs` が消す。`VideoJobLocks` はジョブを消すときに一緒に消す。`DailyQuota` の古い行は §4.8 のとおり消費時に消す。
  リクエストベースの CPU では裏の GC タスクがほとんど進まないため、`DeleteExpiredVideoJobs` は**動画 API へのリクエストの処理中に**
  （最後の実行から 60 秒以上経っていれば）呼ぶ。`main.rs` の GC タスクからも呼ぶが、そちらは補助とする（§4.9）。
- 追加するクレート: `rusty-s3`（署名のみ）、`hmac`、`sha2`、`async-trait`、`hex`。`reqwest` は既存。

### 4.5 ユースケース

| ユースケース | 処理 |
|---|---|
| `CreateVideoJob` | 残り回数 > 0（消費しない）、申告サイズ ≤ 200MB、Content-Type が `video/mp4` か `video/quicktime`、`ClipRange` が正しい → `Created` で保存 → 15 分有効の PUT URL を返す |
| `StartVideoJob` | ロック内: 持ち主確認 → `Created` か → R2 に HEAD（無ければ `upload_not_found`、200MB 超なら `Failed(too_large)`）→ `try_consume` → `Queued` で保存 → `dispatch`。投入失敗なら `refund` して `Failed(dispatch_failed)` |
| `GetVideoJob` | 持ち主確認 → 期限（5.2）を過ぎていれば `Failed(timeout)` にして `refund` → `Done` ならその場で 10 分有効の GET URL を発行（ファイル名 `cutout-{id 先頭8文字}.mp4`） |
| `ReportVideoJobEvent` | ロック内で状態を進める。`Done` / `Failed` になったら入力ファイルを削除（失敗してもログだけ）。`Failed` は 5.1 の表に従って `refund` |
| `DeleteExpiredVideoJobs` | 動画 API のリクエスト処理中に、60 秒に 1 回まで呼ぶ（§4.9）。期限切れのジョブを集め、R2 の削除 → ストアから削除。進行中で期限を過ぎたものは `Failed(timeout)` にして `refund` |

### 4.6 エンドポイント

| ルート | 認証 | 利用ログ |
|---|---|---|
| `POST /video-jobs` | `client-id` ヘッダー + IP | 記録する |
| `POST /video-jobs/{id}/start` | 持ち主の `client-id` | 記録する |
| `GET /video-jobs/{id}` | 持ち主の `client-id` | 記録しない（1 秒ごとのポーリングのため） |
| `POST /internal/video-jobs/{id}/events` | HMAC-SHA256 | 記録しない |

- CLAUDE.md の手順どおり `presentation/router.rs`、`analytics/usage_tracking/operation.rs`、`recorder.rs`（2 箇所）、`frontend/js/api.js` を更新する。
- HMAC: ヘッダー `X-Signature-Timestamp`（UNIX 秒）と `X-Signature`（`hex(HMAC-SHA256(secret, timestamp + "." + body))`）。
  時刻のずれが 5 分を超えたら 401。パスの `id` と本文の `job_id` が違えば 400。比較は定数時間で行う。
- IP の取り方: `TRUSTED_PROXY_HOPS=N`（N ≥ 1）のとき、`X-Forwarded-For` を**右から数えて N 番目**の値を使う。0 のときは `ConnectInfo` の接続元。
  - 先頭（左端）は利用者が自由に偽装できる（Google のフロントエンドは受け取った値の後ろに本当の IP を書き足すだけ）ので使わない。
  - Cloud Run に直接つないでいる本番では N = 1（右端）の見込み。**実装時に、偽の `X-Forwarded-For` を付けて本番にアクセスし、
    ログで実際の値を確かめてから N を確定する**（実装計画にタスクとして入れる）。
  - `ConnectInfo` を使うため、`axum::serve` を `into_make_service_with_connect_info` に変える（現状のコードは IP を一切取っていない）。
  - Cloud Run で N = 0 にすると全利用者が Google の内部アドレス 1 つになり、全員で 1 日 3 回になってしまうので注意。

### 4.7 設定

| 環境変数 | 内容 |
|---|---|
| `R2_ACCOUNT_ID`, `R2_BUCKET`, `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY` | R2 |
| `MODAL_ENDPOINT_URL`, `MODAL_KEY`, `MODAL_SECRET` | Modal の Web エンドポイントとプロキシ認証 |
| `WORKER_CALLBACK_SECRET` | 通知の HMAC 鍵（Modal 側にも Secret として同じ値を置く） |
| `PUBLIC_BASE_URL` | ワーカーが通知を送る先（例: `https://chickren.goodbye-background.com`） |
| `TRUSTED_PROXY_HOPS` | 上記。既定は `0`。本番（Cloud Run）は確認後に `1` の見込み |

1 つでも欠けていたら動画のルートは 503（`video_cutout_disabled`）を返し、画像機能は通常どおり動く。
`.env.example` に追記する。

### 4.8 回数制限の詳細

- キーは `(client-id, 日本時間の日付)` と `(IP, 日本時間の日付)` の 2 系統。どちらかが 3 に達したら止める。消費・返却は両方同時。
- 日付は注入した Clock から日本時間（UTC+9 固定）で計算する。
- 保存先は既存の利用ログ用 DB。テーブル `video_quota_usage (day DATE, key_kind TEXT, key TEXT, count INTEGER, PRIMARY KEY (day, key_kind, key))`。
  - マイグレーションは `migrations/sqlite/` と `migrations/postgres/` の**両方に新しいファイルとして**追加する（適用済みのファイルは書き換えない）。
  - 消費は 1 トランザクションで、2 系統とも `count < 3` を確認してから両方を +1 する。返却は両方を −1（0 未満にしない）。
  - 消費のついでに、8 日以上前の行を削除する。
- DB に置くので、再起動や 0 台への縮小ではリセットされない。
- モバイル回線では IP を多人数で共有することがあり、他人の分を消費しうる。厳密にするには認証が必要で、C の仕様書で扱う。

### 4.9 Cloud Run の設定による制約

本番は最小 0 台・最大 1 台・リクエストベースの CPU。

| 設定 | 影響 | 対応 |
|---|---|---|
| 最大 1 台 | 全リクエストが同じインスタンスに届く。インメモリの状態を共有できる | **最大 1 台を変えない**（増やすとポーリングや通知が別の台に届いて 404 になる）。運用の手順に明記する |
| 最小 0 台 | アクセスが途絶えると 0 台になり、インメモリのジョブが消える | ジョブ処理中はスマホが 1 秒ごとにポーリングし、ワーカーも通知を送るので縮まない。消えた場合はジョブが 404 になり、スマホには「やり直してください」と出す（回数は DB にあり、返却はされない。まれなので許容）。R2 の残りは 1 日ルールで消える |
| リクエストベースの CPU | レスポンスを返した後と、リクエストが無い間は CPU がほぼ割り当てられない。`tokio::spawn` した後処理や 60 秒ごとの GC が進まない | R2 の削除は**レスポンスを返す前に await する**（通知のハンドラ内で入力ファイルを消す等）。期限の処理は動画 API のリクエスト処理中に行う（§4.4） |

補足（今回の範囲外）: 既存の利用ログ（`UsageRecorder`）も `tokio::spawn` したタスクで INSERT しているため、同じ理由で書き込みが遅れたり、
0 台への縮小で失われたりしうる。別の課題として扱う。

---

## 5. エラー・期限・削除

### 5.1 失敗コードと回数の扱い

原則: こちらの都合による失敗は回数を返す。入力が不正な失敗は返さない。

| 失敗コード | 起きる場所 | 回数 |
|---|---|---|
| `dispatch_failed`（Modal が応答しない、予算上限） | Rust | 返す |
| `timeout` | Rust | 返す |
| `internal_error` | ワーカー | 返す |
| `subject_not_found`（1 枚目で `object_score_logits ≤ 0`） | ワーカー | 返す（プレビューとモデルの違いによるズレの可能性があるため） |
| `unsupported_video`（デコード不可、映像なし） | ワーカー | 返さない |
| `range_out_of_bounds`（範囲が動画の長さを超える） | ワーカー | 返さない |
| `too_large` | Rust / ワーカー | 返さない（Rust で弾いた場合はそもそも消費しない） |

- `dispatch_failed` の表示は「混み合っています。時間をおいて試してください」（予算上限とは区別できないため）。
- 文言はコードごとに日本語でフロントに持つ。

### 5.2 期限

| 状態 | 期限 | 期限が来たら |
|---|---|---|
| `Created` | 作成から 30 分 | ジョブと入力ファイルを削除 |
| `Queued` | 5 分 | `Failed(timeout)`、回数を返す |
| `Downloading` / `Tracking` / `Finishing` | 最後の通知から 3 分 | `Failed(timeout)`、回数を返す |
| `Done` / `Failed` | 30 分 | ジョブと出力ファイルを削除 |

### 5.3 ファイル削除の 3 段構え

1. `Done` / `Failed` になった時点で、Rust が入力ファイルを削除する（失敗してもログだけ）。
2. `DeleteExpiredVideoJobs`（動画 API のリクエスト処理中に 60 秒に 1 回まで）が期限切れのジョブの入出力を削除する。既存の `TtlStore::delete_expired` は値を捨てるだけで R2 を消せないので、ジョブだけはこの手順にする。
3. R2 のライフサイクルルールで `video-jobs/` を 1 日で削除する（再起動でジョブを忘れた場合の漏れを拾う）。

ワーカーの `/tmp` は `finally` で削除する。

### 5.4 通知が届かないとき

- 進捗の通知は失敗しても無視して処理を続ける。`done` / `failed` だけ 1 秒・2 秒・4 秒の間隔で 3 回まで再送する。
- それでも届かなければ Rust のタイムアウトで `Failed(timeout)`（回数は返す）。作られた出力ファイルは期限の削除で消える。MVP ではこれを許容する。

---

## 6. ワーカー（Python）の設計

### 6.1 構成

```
worker/                       Python 3.12, uv
  cutout/
    contract.py     ジョブの依頼と通知の型
    video_io.py     FrameReader（ffmpeg デコード → パイプ）/ Mp4Writer（パイプ → ffmpeg エンコード + 音声）
    tracker.py      Sam2VideoTracker（5 つの ONNX セッションとメモリバンク）
    compositor.py   マスク → アルファ → 背景色で合成 → ロゴ
    notifier.py     HMAC 署名付きの通知
    pipeline.py     run_job(依頼, ダウンロード/アップロード, 通知) — 依存はすべて注入
    assets/logo.png ロゴのバッジ（logo-original.webp から生成）
  modal_app.py      Modal のアダプタ
  tests/
contracts/video_cutout/
  worker_job_request.json     Rust → ワーカー の依頼の例
  worker_event_*.json         ワーカー → Rust の通知の例（downloading / tracking / finishing / done / failed）
```

`cutout/` は Modal を import しない。RunPod などへ移るときは `modal_app.py` だけ書き直す。

### 6.2 依頼と通知の形（契約）

依頼（Rust → ワーカー）:

```json
{
  "job_id": "6f1c…",
  "input_url": "https://…r2…/video-jobs/6f1c…/input?X-Amz-…",
  "output_url": "https://…r2…/video-jobs/6f1c…/output.mp4?X-Amz-…",
  "range_ms": { "start": 1200, "end": 9200 },
  "points": [ { "x": 0.51, "y": 0.44, "label": "include" } ],
  "background_rgb": [0, 255, 0],
  "watermark": true,
  "max_input_bytes": 209715200,
  "callback_url": "https://chickren.goodbye-background.com/internal/video-jobs/6f1c…/events"
}
```

- `input_url` は 30 分有効の GET、`output_url` は 30 分有効の PUT。

通知（ワーカー → Rust）:

```json
{ "job_id": "6f1c…", "phase": "tracking", "progress": 0.45 }
{ "job_id": "6f1c…", "phase": "done", "frames": 240, "lost_frames": 3, "duration_ms": 8000 }
{ "job_id": "6f1c…", "phase": "failed", "code": "unsupported_video" }
```

`phase` は `downloading` / `tracking` / `finishing` / `done` / `failed`。
`contracts/` の JSON は Rust と Python の両方のテストで読み込んで検証する。

### 6.3 処理の流れ（`run_job`）

1. **downloading**: HEAD で `max_input_bytes` 以下を確認 → `/tmp` にダウンロード → `ffprobe` で長さ・fps・解像度・色空間を取る。
   範囲が長さを超えていれば `range_out_of_bounds`。
2. **tracking**: ffmpeg でデコードする。
   - `-ss 開始 -to 終了` で範囲を切る。
   - `fps` フィルタで `min(元の fps, 30)` の一定フレームレートに揃える（可変フレームレート対策）。
   - HDR（HLG / PQ）なら `zscale` + `tonemap` で SDR に変換する（iPhone 13 は標準で HEVC 10bit HLG）。
   - 長辺 1080px 以下・偶数サイズに縮小する。
   - 回転は ffmpeg の自動回転に任せる（ブラウザの表示と一致する）。
   - **1 フレームずつストリームで処理する**（1080p × 300 枚を RGB で持つと約 1.9GB になるため）。
   - フレーム数は 301 枚で打ち切る。
3. **トラッキング**（`bench.py` の `track` を移植。規則は 6.4）。
4. **合成**: `high_res_mask`（512px のロジット）→ 出力サイズにバイリニア拡大 → sigmoid → 3×3 の erode を 1 回 →
   `元のフレーム × α + 背景色 × (1 − α)`。見失ったフレームは α = 0（全面背景色）。ロゴのバッジを numpy で合成する。
5. **エンコード**（トラッキングと並行してストリームで流す）: libx264 `veryfast` / CRF 18 / yuv420p / `-movflags +faststart`。
   音声は元動画から同じ範囲を切って AAC（音声が無ければ映像のみ）。
6. **finishing**: エンコーダを閉じる → `output_url` に PUT → `done`。

進捗の通知は 0.5 秒に 1 回まで間引く。

### 6.4 トラッキングの規則（検証済み。引き継ぎ文書より）

- 前処理: フレームを 512×512 に引き伸ばし（余白なし）、RGB を 0〜1 にして `constants.json` の mean/std で正規化、CHW。
- 1 枚目: エンコーダ → デコーダ（`feats2_no_mem` とクリック点。詰め点は付けない）→ メモリエンコーダ（`binarize=1`）で条件メモリを作る。
  `object_score_logits ≤ 0` なら `subject_not_found`。
- 2 枚目以降:
  - メモリバンク: 条件メモリ（時間埋め込みの行 6）+ 最大 6 フレーム前までのメモリ（k フレーム前は行 k−1）。
    7 ブロックに満たなければ最新のものを複製して埋める。`memory_pos` には各ブロックの行の埋め込みを足す。
  - ポインタ: 条件フレーム（時間差 |t−0|）→ 1, 2, … フレーム前の順に最大 16 個。足りなければ最新を複製。
    時間差は `min(総フレーム数, 16) − 1` で割る。各ポインタ 256 次元を 4 トークン × 64 次元に分け、位置埋め込みは各ポインタを 4 回繰り返す。
  - `memory_attention` → デコーダ（詰め点 1 つ `(0,0)` ラベル −1 だけ）→ メモリエンコーダ（`binarize=0`）。
  - `object_score_logits ≤ 0` なら、そのフレームは見失い（α = 0）。

### 6.5 ロゴのバッジ

- `assets/brand/logo-original.webp`（2000×1999、背景 `#171717`）を角丸の正方形に切り抜き、`worker/cutout/assets/logo.png` として持つ。
- 出力の短辺の約 8% の大きさで、右下に余白（短辺の約 2%）をとって**不透明**で合成する。
- 黒（`#171717`）は緑・青・マゼンタのどのキーにも引っかからず、鳥の色が背景色と接しないため、クロマキーでロゴが抜けない。
- 同じ画像から動画ページの favicon を作る。

### 6.6 Modal のアダプタ

- イメージ: Debian + ffmpeg（`zscale` 入り）+ `onnxruntime-gpu` + numpy。モデルの 5 つの ONNX と `constants.json` は
  **ビルド時に Hugging Face からリビジョンを固定して**取り込む（起動時にダウンロードしない）。
- `@modal.enter` で ONNX セッション（CUDA EP）を読み込む。
- 関数: GPU=L4、`timeout=300`、`retries=0`、`scaledown_window` はまず標準の 60 秒で始め、計測で調整する。
- Web エンドポイント: プロキシ認証（`Modal-Key` / `Modal-Secret`）を必須にし、依頼を検証して `spawn` し、すぐ 202 を返す。
- `WORKER_CALLBACK_SECRET` は Modal の Secret に置く。

---

## 7. フロントエンド

- `frontend/video.html` と `frontend/js/video/*`（ビルド不要の Vanilla JS。既存と同じ方針）。
- 画面の流れ:
  1. 動画を選ぶ（`<input type="file" accept="video/*">`）
  2. `<video>` で再生し、スライダーで開始・終了を選ぶ（終了は開始 + 10 秒まで）
  3. `POST /video-jobs` → `XMLHttpRequest` で PUT（進捗バー）
  4. 並行して、開始位置のフレームを canvas に描いて画像にし、既存の `/upload` → `/segment` でプレビュー。
     タップの追加と取り消しは既存の undo/redo を使う。「プレビューは目安です」と表示
  5. 背景色を選び、送信 → `POST /video-jobs/{id}/start`
  6. 1 秒ごとに `GET /video-jobs/{id}`。`Queued` は「準備中（GPU を起動しています）」、`Tracking` は「処理中 NN%」
  7. `Done` でダウンロード。`Failed` はコードごとの文言と、回数が返ったかどうかを表示
- 残り回数を画面に出す（`POST /video-jobs` と `GET` の応答に `remaining_today` を含める）。

---

## 8. テスト

### 8.1 Rust（`cargo test`。外部サービスには繋がない）

- ドメインの値: `ClipRange` の境界（10,000ms は通る / 10,001ms は弾く）、`NormalizedPoint` の範囲、`JobPhase` の遷移（逆向きは無視、最終状態は不変）。
- 回数制限: `FakeClock` で日本時間 0 時をまたいでリセット、IP と `client-id` のどちらかが上限なら停止、返却で戻る。
- ユースケース（ストレージとワーカーは記録する偽物・必ず失敗する偽物）:
  `/start` の同時 2 回で投入は 1 回、投入失敗で回数が戻る、他人の `client-id` は 404、`FakeClock` を進めるとタイムアウト、期限切れで R2 の削除が呼ばれる。
- HMAC: 正しい署名は通る、本文の改ざん・5 分を超えた古いタイムスタンプ・パスと本文の `job_id` の不一致は拒否。
- 契約: `contracts/video_cutout/*.json` を Rust の型で読める。Rust が組み立てた依頼が例と同じキー構成になる。

### 8.2 Python（`pytest`）

- メモリバンクとポインタの組み立て: 6.4 の規則をフレーム番号ごとの表で確認（複製、行番号、時間差の正規化、4 トークン分割）。
- 合成: α の計算、見失ったフレームが全面背景色、ロゴの位置と大きさ。
- パイプライン（トラッカーは偽物、ffmpeg は本物。テスト動画は `testsrc` で生成）:
  60fps → 30fps、範囲どおりのフレーム数、音声あり、回転情報つき入力の向き、10bit HLG 入力が SDR で出る。
- 契約: `contracts/` の JSON を読める。
- 移植の正しさ（遅いテスト、普段は省略）: 実モデルを CPU で動かし、短い動画の 10 フレームで `bench.py` と IoU ≥ 0.99。

### 8.3 手作業（iPhone 13 / iOS 18 の Safari）

1. 選択 → 範囲 → プレビュー → 送信 → 途中で他のアプリに切り替えて戻っても進捗が続く
2. ダウンロードして写真アプリに保存できる
3. CapCut でクロマキーを掛け、背景だけが抜けてロゴが残る（緑・青・マゼンタ）

---

## 9. 実装の順番

| 順番 | タスク | 理由 |
|---|---|---|
| Task 0 | `src/image_cutout/` への移動（挙動を変えないコミット） | 移動と新機能を混ぜない |
| Task 1 | **Modal での計測**（下記） | 一番大きな不確実性を作り込む前に潰す |
| Task 2〜 | ワーカーの中核 → Rust 側 → フロントエンド → 手作業の確認 | |

### Task 1 の計測と判断基準

- Modal に最小のワーカー（モデル + ffmpeg 入りのイメージ、L4）を置き、次を測る:
  コールドスタート（起動 + モデル読み込み）、温まった状態での 1 フレームあたりの時間、
  10 秒・30fps・1080p の 1 本の合計時間と費用、HDR 動画の色。
- **コールドスタート ≤ 60 秒なら Modal で進める。** 超えたら Modal のメモリスナップショットを試し、それでも駄目なら RunPod を同じアダプタ構成で試す。
- 結果は `docs/video-cutout-measurements.md` に表で残す。

---

## 10. 運用の手順（コード外）

- Modal: Starter プランで、ワークスペースの月の予算上限を $30 に設定する。Secret に `WORKER_CALLBACK_SECRET` を置く。
- R2: 非公開のバケットを作る。CORS で本番オリジンからの `PUT` / `GET` を許可する。
  ライフサイクルルールで `video-jobs/` を 1 日で削除する。オブジェクトの読み書きだけを許可した API トークンを発行する。
- Rust の本番環境（Cloud Run）に 4.7 の環境変数を設定する。シークレットは Secret Manager 経由で渡す。
- Cloud Run の最大インスタンス数は 1 のまま変えない（§4.9）。
- R2 の CORS で許可するオリジンは `https://chickren.goodbye-background.com`。

---

## 11. リスクと確認事項

| 項目 | 内容 | 対応 |
|---|---|---|
| コールドスタート | 実測値が無い | Task 1 で計測し、60 秒の基準で判断 |
| Modal のクレジットの範囲 | 外向き転送やストレージが含まれるか、記事によって書き方が違う | Task 1 で請求画面を確認。ファイルは R2 に置くので影響は小さい見込み |
| プレビューと本番のずれ | hiera_small（1024）と tiny（512）で細部が違う | 「目安」と表示。困る例が出たら GPU プレビューに切り替える（フロントの流れは変わらない） |
| 縁取り | `high_res_mask` + erode でも残る可能性 | MVP 後に別課題として扱う |
| IP の共有 | モバイル回線で他人の分を消費しうる | C の仕様書（認証）で扱う |
| 再起動・0 台への縮小 | 処理中のジョブが消える（回数は DB にあるので消えない） | 許容（D7）。R2 は 1 日ルールで掃除される |
| IP の取り方 | Cloud Run での `X-Forwarded-For` の実際の形が未確認 | 実装時に本番で確かめて `TRUSTED_PROXY_HOPS` を確定する（§4.6） |
