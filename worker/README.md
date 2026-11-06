# 動画切り抜きワーカー（Modal）

動画の背景を SAM2.1-tiny（ONNX・fp16）のビデオトラッキングで消して、緑などの単色背景と合成した mp4 を作るワーカーです。
Rust のサーバーから依頼を受け、Modal の GPU で 1 本ずつ処理し、結果を R2 に置いて Rust に通知します。

```
Rust ──POST（プロキシ認証）──▶ web（CPU。検証して 202）──spawn──▶ Worker.process（GPU）
                                                                  │ R2 から入力を GET
                                                                  │ 推論・合成・エンコード
                                                                  │ R2 へ出力を PUT
Rust ◀──── 進捗・完了の通知（HMAC 署名付き。callback_url へ POST）───┘
```

| ファイル | 役割 |
| --- | --- |
| `modal_app.py` | Modal のアダプタ（イメージ・GPU・Secret・Web エンドポイントの定義）。デプロイするのはこのファイル |
| `cutout/accept.py` | 受け付け（依頼の検証 → spawn → 202 / 400）と作業ディレクトリ。Modal に依存しない |
| `cutout/transfer.py` | R2 の署名付き URL との送受信（`HttpTransfer`）。URL はログ・例外に出さない |
| `cutout/pipeline.py` | 1 本の処理全体（`run_job`）。例外を外に出さず、必ず完了の通知を 1 回送る |
| `cutout/contract.py` | Rust との契約（JSON の正は `contracts/video_cutout/*.json`） |
| `measure/` | Task 1 の計測用（使い捨て。本番では使わない） |

- 計測の結果: [`docs/video-cutout-measurements.md`](../docs/video-cutout-measurements.md)
  （GPU を `["L4", "A10G", "T4"]` のフォールバックにした理由・slim イメージを使う理由・費用の見積もり）
- モデルは `cutout/models.py` の `MODEL_REVISION`（Hugging Face のコミットハッシュ）に固定し、イメージのビルド時に取得します

## テスト

```bash
cd worker && uv run pytest -v            # 通常のテスト（Modal・GPU・モデル不要）
```

移植の正しさの遅いテスト（IoU ≥ 0.99。モデルが必要）は `tests/test_tracker_parity.py` の先頭の説明を参照。

---

## デプロイの手順（オーナーの作業）

> Windows の PowerShell を前提に書いています。コマンドはすべて **リポジトリの直下**（`Goodbye_Background` フォルダ）で実行してください。
> Modal の CLI（`modal>=1.6,<2`）が入っていて `modal setup` でログイン済み、予算の上限（$30）を設定済みであることが前提です
> （まだなら [`measure/README.md`](measure/README.md) の「1. 準備」）。

### 1. 通知の署名用の Secret を作る（最初の 1 回だけ）

ワーカーが Rust に送る通知の署名（HMAC）に使う秘密の値です。**同じ値を後で Cloud Run（Rust）にも設定します。**

ランダムな値を作る（64 文字の 16 進数）:

```powershell
$b = New-Object byte[] 32
[System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($b)
$secret = -join ($b | ForEach-Object { $_.ToString("x2") })
$secret    # 表示された値をパスワードマネージャーなどに保存しておく（Cloud Run の設定で使う）
```

Modal の Secret を作る（名前とキーはこの通りにすること。`modal_app.py` がこの名前で読みます）:

```powershell
modal secret create goodbye-background-video WORKER_CALLBACK_SECRET=$secret
```

- Secret の名前: `goodbye-background-video`
- キー: `WORKER_CALLBACK_SECRET`
- 値を変えたいときは Modal の画面（Secrets）で編集するか、`modal secret create --force ...` で作り直し、Cloud Run 側も同じ値に揃えます

### 2. プロキシ認証のトークンを作る（最初の 1 回だけ）

Web エンドポイントは **Modal のプロキシ認証が必須**（`Modal-Key` / `Modal-Secret` ヘッダーが無いリクエストは Modal が拒否）です。
Rust がこのヘッダーを付けて呼ぶためのトークンを作ります。

1. ブラウザで https://modal.com にログインし、右上のワークスペース名から **Settings** を開く
2. **Proxy Auth Tokens** を開き、**New Token** で作る
3. 表示された **Token ID（`wk-...`）と Token Secret（`ws-...`）** を保存する
   - Rust では Token ID を `MODAL_KEY`、Token Secret を `MODAL_SECRET` として使います
   - Token Secret は作成時にしか表示されないことがあるので、必ずその場で保存してください

> **トークン（`MODAL_KEY` / `MODAL_SECRET`）と `WORKER_CALLBACK_SECRET` の値は、チャットに貼らないでください。**
> 後で Cloud Run の設定（Secret Manager など）に直接入れます。

### 3. デプロイする

```powershell
modal deploy worker/modal_app.py
```

- 初回はイメージのビルド（ffmpeg・CUDA / cuDNN の wheel・モデルの取得）で数分かかります。2 回目以降はコードを変えた部分だけ作り直します
- 終わると、次のような行が出ます。この URL が Rust の **`MODAL_ENDPOINT_URL`** です

  ```
  ✓ Created web function web
    https://<ワークスペース名>--goodbye-background-video-web.modal.run 🔑
  ```

  （URL の後ろが 🔑 = プロキシ認証が必須、であることも確認してください。`(unauthenticated)` と出ていたら誤りです）
- `Secret goodbye-background-video not found` のようなエラーが出たら、手順 1 の Secret の名前を確認してください

### 4. 疎通を確認する

URL とトークンを PowerShell の変数に入れます（この PowerShell の画面の中だけで使われ、保存はされません）:

```powershell
$url = "https://<ワークスペース名>--goodbye-background-video-web.modal.run"   # 手順 3 の URL
$key = "wk-..."     # 手順 2 の Token ID
$sec = "ws-..."     # 手順 2 の Token Secret
```

**(1) 認証ヘッダー無し → 401 か 403 が返ること**（Modal が入口で拒否する）:

```powershell
curl.exe -i -X POST $url
```

**(2) 正しいヘッダー + 契約違反の本文 → 400 が返ること**（ワーカーまで届き、検証で拒否される。GPU は起動しない）:

```powershell
Set-Content -Path bad.json -Value '{"job_id":"check"}' -Encoding ascii
curl.exe -i -X POST $url -H "Content-Type: application/json" -H "Modal-Key: $key" -H "Modal-Secret: $sec" --data-binary "@bad.json"
Remove-Item bad.json
```

期待する応答（1 行目と本文）:

```
HTTP/1.1 400 Bad Request
{"accepted":false,"error":"input_url がない"}
```

- 初回はコンテナの起動で数秒かかることがあります
- 正しい本文（`contracts/video_cutout/worker_job_request.json` の形）は送らないでください。GPU のジョブが動き、存在しない URL に通知を送ろうとします
  （本当のジョブの確認は Rust 側と繋いだ後に行います）

### 5. 結果を伝える

チャットには次を貼ってください:

- 手順 3 の **エンドポイントの URL**（URL 自体は秘密ではありません。認証はトークンで行うため）
- 手順 4 の (1)(2) のステータスコード（1 行目）と (2) の本文

**貼らないもの**: `MODAL_KEY` / `MODAL_SECRET`（プロキシ認証のトークン）、`WORKER_CALLBACK_SECRET` の値。

---

## 本番の設定のまとめ

| 項目 | 値 | 場所 |
| --- | --- | --- |
| Modal のアプリ名 | `goodbye-background-video` | `modal_app.py` の `APP_NAME` |
| GPU | `["L4", "A10G", "T4"]`（空いているものを順に使う） | `GPU_FALLBACK` |
| 1 本の制限時間 / 再試行 | 300 秒 / 0 回（失敗は Rust にそのまま通知） | `@app.cls(timeout=300, retries=0)` |
| 待機してから止めるまで | 60 秒（続けて来たジョブはモデルの読み込みを省ける） | `scaledown_window=60` |
| Secret | `goodbye-background-video`（キー `WORKER_CALLBACK_SECRET`） | Modal の Secrets |
| Rust 側に設定するもの | `MODAL_ENDPOINT_URL`・`MODAL_KEY`・`MODAL_SECRET`・`WORKER_CALLBACK_SECRET` | Cloud Run |
| モデル | `diffusionstudio/sam2.1-tiny-video-onnx-fp16` @ `MODEL_REVISION` | `cutout/models.py` |

ログ（`modal app logs goodbye-background-video` または Modal の画面）には、署名付き URL を出さない作りになっています
（送受信の失敗は HTTP ステータスと例外の型だけ）。
