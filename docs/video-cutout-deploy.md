# 動画切り抜きの本番の準備（オーナーの作業手順）

動画切り抜きを本番（Cloud Run・https://chickren.goodbye-background.com ）で動かすための手順です。
上から順に進めてください。各手順の最後の **「確認」** が満たせたら次へ進みます。

> - Windows の PowerShell・`gcloud`・`docker` を前提に書いています。`curl` は必ず **`curl.exe`** と書いてください
>   （PowerShell の `curl` は別のコマンドの別名です）
> - `<PROJECT>`・`<REGION>`・`<SERVICE>`・`<REPO>`・`<IMAGE>` は、**いつもデプロイに使っている名前**に置き換えてください
>   （プロジェクト ID・リージョン・Cloud Run のサービス名・Artifact Registry のリポジトリ名・イメージ名）
> - **秘密の値（R2 の Secret Access Key・Modal のトークン・`WORKER_CALLBACK_SECRET`）はチャットに貼らないでください。**
>   この手順では、値は PowerShell の入力欄から Secret Manager に直接入れます
> - Modal のワーカーはデプロイ済み（`https://chickrens--goodbye-background-video-web.us-east.modal.run`）、
>   プロキシ認証のトークン（Token ID `wk-…` / Token Secret `ws-…`）と `WORKER_CALLBACK_SECRET` の値は手元にある前提です
>   （まだなら [`worker/README.md`](../worker/README.md) の「デプロイの手順」）

最初に、この PowerShell の画面で使う値を変数に入れておきます（以降のコマンドはこの変数を使います）:

```powershell
$PROJECT = "<PROJECT>"     # 例: my-project-123
$REGION  = "<REGION>"      # 例: asia-northeast1
$SERVICE = "<SERVICE>"     # Cloud Run のサービス名
gcloud config set project $PROJECT
```

---

## 1. Cloudflare R2（動画の置き場所）

入力の動画と出力の動画を置くバケットです。ブラウザと Modal のワーカーは、サーバーが発行する **署名付き URL** で
R2 を直接読み書きします（Modal 側に R2 の設定は要りません）。

### 1-1. バケットを作る

1. https://dash.cloudflare.com にログイン → 左のメニューの **R2 Object Storage** → **Create bucket**
2. 名前: **`goodbye-background-video`**（別の名前にした場合は手順 3 の `R2_BUCKET` もその名前にする）
3. Location は Automatic、Storage class は Standard のままで作成
4. **公開しない**: バケットの **Settings** の **Public access**（Public Development URL・Custom Domains）は**無効のまま**にする

### 1-2. CORS（ブラウザからのアップロードとダウンロードを許可する）

バケットの **Settings** → **CORS Policy** → **Add CORS policy**（または Edit）を開き、JSON のタブに次を貼って保存します:

```json
[
  {
    "AllowedOrigins": ["https://chickren.goodbye-background.com"],
    "AllowedMethods": ["PUT", "GET"],
    "AllowedHeaders": ["content-type", "content-length"],
    "MaxAgeSeconds": 3600
  }
]
```

- アップロード用の URL は `content-type` と `content-length` の両方を署名に含めています。`content-length` はブラウザが自動で付けるヘッダーで、
  事前確認（preflight）には普通は載りませんが、載せるブラウザがあっても通るように許可しておきます（害はありません）
- `ExposeHeaders`（ブラウザに見せる応答ヘッダー）は不要です

### 1-3. ライフサイクルルール（1 日で自動削除）

バケットの **Settings** → **Object lifecycle rules** → **Add rule**:

| 項目 | 値 |
| --- | --- |
| Rule name | `delete-video-jobs` |
| Prefix（適用する範囲） | `video-jobs/` |
| 動作 | **Delete uploaded objects** を **1 day** 後 |

（サーバーも不要になった動画は消しますが、取りこぼしをこのルールで確実に消します）

### 1-4. API トークン（サーバーが署名付き URL を作るための鍵）

1. R2 の概要ページの右側 **Manage R2 API Tokens**（または **API** → **Manage API tokens**）→ **Create API token**
   （「User API token」と「Account API token」がある場合はどちらでも構いません）
2. Permissions: **Object Read & Write**
3. Specify bucket(s): **Apply to specific buckets only** → `goodbye-background-video`
4. TTL: Forever（期限を付けた場合は期限前に作り直しが必要）
5. 作成後に表示される **Access Key ID** と **Secret Access Key** を保存する（Secret Access Key は**この画面でしか表示されません**）

**Account ID** は R2 の概要ページの右側（**Account details** の **Account ID**）にあります。
S3 API のエンドポイント `https://<Account ID>.r2.cloudflarestorage.com` の `<Account ID>` の部分と同じ値です。

**確認**: バケット `goodbye-background-video` があり、CORS のルール 1 件・ライフサイクルルール `delete-video-jobs` が表示される。
Account ID・Access Key ID・Secret Access Key の 3 つが手元にある。

---

## 2. Secret Manager（秘密の値を Cloud Run に渡す）

秘密の値は Cloud Run の環境変数に直接書かず、Secret Manager に入れて参照させます。

### 2-1. API を有効にする（最初の 1 回だけ）

```powershell
gcloud services enable secretmanager.googleapis.com
```

### 2-2. シークレットを作る

次の関数を貼り付けてから（Enter を押すと定義されるだけで、まだ何も起きません）、シークレットごとに呼び出します。
値は **画面に表示されない入力欄**（`*` が出る）に貼り付けます。

```powershell
function New-VideoSecret([string]$Name) {
  $secure = Read-Host "$Name の値を貼り付けて Enter（表示されません）" -AsSecureString
  $plain  = [System.Net.NetworkCredential]::new("", $secure).Password
  $file   = New-TemporaryFile
  try {
    # 改行を付けずに書く（PowerShell のパイプで渡すと末尾に改行が付き、署名が合わなくなるため一時ファイルを使う）
    [System.IO.File]::WriteAllText($file.FullName, $plain.Trim())
    gcloud secrets create $Name --replication-policy=automatic --data-file="$($file.FullName)"
  } finally {
    Remove-Item $file.FullName -Force
    $plain = $null
  }
}
```

```powershell
New-VideoSecret R2_ACCESS_KEY_ID          # 手順 1-4 の Access Key ID
New-VideoSecret R2_SECRET_ACCESS_KEY      # 手順 1-4 の Secret Access Key
New-VideoSecret MODAL_KEY                 # Modal の Token ID（wk-…）
New-VideoSecret MODAL_SECRET              # Modal の Token Secret（ws-…）
New-VideoSecret WORKER_CALLBACK_SECRET    # Modal の Secret goodbye-background-video に入れたのと同じ値
```

- `R2_ACCESS_KEY_ID` と `MODAL_KEY` は厳密には秘密ではありません（対になる Secret が無いと使えない）が、まとめて管理するためここに入れます。
  環境変数に直接書いても動きます（その場合は手順 3 の `--update-env-vars` 側に移す）
- 間違えた値を入れたときは、同じ関数の `gcloud secrets create` を `gcloud secrets versions add` に変えたもので新しい版を足します
  （`:latest` を参照しているので、次のデプロイ／更新から新しい値になります）

### 2-3. Cloud Run のサービスアカウントに読み取りを許可する

Cloud Run が使っているサービスアカウントを調べます:

```powershell
$SA = gcloud run services describe $SERVICE --region $REGION --format="value(spec.template.spec.serviceAccountName)"
if (-not $SA) {
  # 空なら既定の Compute Engine のサービスアカウントが使われている
  $NUM = gcloud projects describe $PROJECT --format="value(projectNumber)"
  $SA = "$NUM-compute@developer.gserviceaccount.com"
}
$SA
```

5 つのシークレットに読み取り権限を付けます:

```powershell
foreach ($n in "R2_ACCESS_KEY_ID","R2_SECRET_ACCESS_KEY","MODAL_KEY","MODAL_SECRET","WORKER_CALLBACK_SECRET") {
  gcloud secrets add-iam-policy-binding $n --member="serviceAccount:$SA" --role="roles/secretmanager.secretAccessor"
}
```

**確認**: `gcloud secrets list` に 5 つの名前が出る。`gcloud secrets versions list WORKER_CALLBACK_SECRET` で版 `1` が `enabled`。
（値そのものを表示するコマンド `gcloud secrets versions access` は、画面に出るので使わないでください）

---

## 3. Cloud Run の環境変数

動画切り抜きの設定は次の通りです（`.env.example` と同じ）。**1 つでも欠けると動画の API は 503 になり**（画像機能はそのまま動く）、
起動ログに「動画切り抜き: 無効」と出ます。

| 名前 | 値 | 渡し方 |
| --- | --- | --- |
| `R2_ACCOUNT_ID` | 手順 1-4 の Account ID | 環境変数 |
| `R2_BUCKET` | `goodbye-background-video` | 環境変数 |
| `MODAL_ENDPOINT_URL` | `https://chickrens--goodbye-background-video-web.us-east.modal.run` | 環境変数 |
| `PUBLIC_BASE_URL` | `https://chickren.goodbye-background.com`（末尾の `/` は付けない） | 環境変数 |
| `TRUSTED_PROXY_HOPS` | `1`（**仮**。手順 5 で確かめて決める） | 環境変数 |
| `LOG_CLIENT_IP_DEBUG` | `1`（**一時的**。手順 4-3 の初回のデプロイでだけ付け、手順 5 の確認が終わったら外す） | 環境変数 |
| `R2_ACCESS_KEY_ID` | | シークレット（手順 2） |
| `R2_SECRET_ACCESS_KEY` | | シークレット（手順 2） |
| `MODAL_KEY` | | シークレット（手順 2） |
| `MODAL_SECRET` | | シークレット（手順 2） |
| `WORKER_CALLBACK_SECRET` | | シークレット（手順 2） |

手順 4 のデプロイのコマンドに付けるオプションを変数にしておきます（`<Account ID>` だけ置き換える）:

```powershell
$VIDEO_ENV = "R2_ACCOUNT_ID=<Account ID>,R2_BUCKET=goodbye-background-video,MODAL_ENDPOINT_URL=https://chickrens--goodbye-background-video-web.us-east.modal.run,PUBLIC_BASE_URL=https://chickren.goodbye-background.com"
$VIDEO_SECRETS = "R2_ACCESS_KEY_ID=R2_ACCESS_KEY_ID:latest,R2_SECRET_ACCESS_KEY=R2_SECRET_ACCESS_KEY:latest,MODAL_KEY=MODAL_KEY:latest,MODAL_SECRET=MODAL_SECRET:latest,WORKER_CALLBACK_SECRET=WORKER_CALLBACK_SECRET:latest"
```

- `TRUSTED_PROXY_HOPS=1`（仮）と `LOG_CLIENT_IP_DEBUG=1`（一時的）は `$VIDEO_ENV` に入れず、手順 4-3 の**初回のデプロイのコマンドにだけ**直接書きます。
  `$VIDEO_ENV` に入れておくと、後でデプロイし直したときに確認用のログが復活し、手順 5 で決めた値が `1` に戻ってしまうためです
- `--update-env-vars` / `--update-secrets` は **今ある設定（`DATABASE_URL_POSTGRES` など）を残したまま足す** オプションです。
  一度設定すれば、以後のデプロイでも残ります。**`--set-env-vars` や `--clear-env-vars` は使わないでください**（今ある設定が消えます）
- **最大インスタンス数は 1 のまま**にします（`--max-instances 1`）。動画のジョブはサーバーのメモリにあるので、
  2 台以上になると進捗の問い合わせやワーカーからの通知が別の台に届き、「見つからない」になります
- 最小インスタンス数 0・リクエストベースの CPU は今のままで構いません

**確認**: `$VIDEO_ENV` と `$VIDEO_SECRETS` を作った（`<Account ID>` を置き換えた）。

---

## 4. このブランチをビルドしてデプロイする

### 4-1. コードを取ってくる

```powershell
cd <Goodbye_Background のフォルダ>
git fetch origin
git switch claude/video-crop-handoff-1i1r2g
git pull
```

### 4-2. モデルがあることを確かめる

`DockerFile` は `models/` をイメージにコピーします（無いとビルドが失敗するか、起動時に落ちます）:

```powershell
Get-ChildItem models
```

`sam2.1_hiera_small_image_encoder.onnx`・`sam2.1_hiera_small_prompt_encoder.onnx`・`sam2.1_hiera_small_mask_decoder.onnx`・`image_pe.bin` の 4 つがあれば OK です。

### 4-3. ビルド・プッシュ・デプロイ（初回）

**いつもと同じ流れ**で行います。**このコマンドは動画の設定を入れる初回だけ**使います（2 回目以降は手順 5 の後の「以後のデプロイ」）。名前はいつも使っているものに置き換えてください（例は Artifact Registry の場合）:

```powershell
$TAG   = "video-" + (Get-Date -Format "yyyyMMdd-HHmm")
$IMAGE = "$REGION-docker.pkg.dev/$PROJECT/<REPO>/<IMAGE>:$TAG"

docker build -t goodbye-background:$TAG -f DockerFile .
docker tag goodbye-background:$TAG $IMAGE
docker push $IMAGE

gcloud run deploy $SERVICE --image $IMAGE --region $REGION `
  --update-env-vars "$VIDEO_ENV,TRUSTED_PROXY_HOPS=1,LOG_CLIENT_IP_DEBUG=1" `
  --update-secrets $VIDEO_SECRETS `
  --max-instances 1
```

- 最後の `TRUSTED_PROXY_HOPS=1,LOG_CLIENT_IP_DEBUG=1` は**初回だけ**のものです（手順 5 で値を決め、確認用のログを外します）

- いつものデプロイのコマンドに他のオプションがある場合は、それも付けたままにしてください（上の 3 つを**足す**だけ）
- Rust のビルドに数分〜十数分かかります
- `Permission denied on secret` のようなエラーが出たら、手順 2-3 の権限を確認してください

### 4-4. 起動ログを見る

https://console.cloud.google.com/run → サービス → **ログ** タブ（または **Logs Explorer**）で、新しいリビジョンの起動時の行を確認します。

**確認**: 起動ログに次の 2 行が出ている:

```
動画切り抜き: 有効
動画切り抜き: IP の確認用ログ（[client-ip-debug]）を出します
```

「動画切り抜き: 無効（環境変数が足りないため、動画の API は 503 を返す）」と出た場合は、手順 3 の値のどれかが欠けています
（`TRUSTED_PROXY_HOPS` が数値でない場合は、その前に「TRUSTED_PROXY_HOPS が数値ではないため…」の行が出ます）。
画像の切り抜き（https://chickren.goodbye-background.com/app.html ）がいつも通り動くことも確かめてください。

**マイグレーションのエラーで起動しない場合**（ログに `Failed to run PostgreSQL migrations: ...`）:

- `VersionMissing(<番号>)`: DB には適用済みなのに、デプロイしたコードに無いマイグレーションがある。
  以前、本番を Google ログインのブランチからデプロイしていたため `20261101090000` でこれが出た（このブランチで対応済み）。
  別のブランチからデプロイしていないか確認してください
- `VersionMismatch(<番号>)`: 適用済みのマイグレーションのファイルの中身（チェックサム）が変わっている。
  適用済みのファイルは編集してはいけないので、コードの側を元に戻します（DB を直さないでください）
- **このデプロイで消えるもの**: 初回の起動時に `20261107000000` が走り、Google ログインの古いテーブル
  （`users` と `login_sessions`。ログインした人のメールアドレスなど）が削除されます。元には戻せません（オーナー判断で削除）。
  利用ログ（`usage_records`）はそのまま残ります

---

## 5. `TRUSTED_PROXY_HOPS` を確かめて決める

回数制限（1 日 3 回）は利用者の IP でも数えます。IP は `X-Forwarded-For` ヘッダーの **右から N 番目**（N = `TRUSTED_PROXY_HOPS`）を使います。
左側は利用者が自由に書けるので信用できません。N を間違えると、全員が同じ IP 扱い（全員で 1 日 3 回）になったり、偽装できたりします。

### 5-1. 自分の本当のグローバル IP を調べる

```powershell
curl.exe -4 https://api.ipify.org
```

（`-4` と api.ipify.org で **IPv4** のアドレスを取ります。手順 5-2 も `-4` で IPv4 に揃えます。表示された IP は、この後の比較に使うだけです。チャットに貼る必要はありません）

### 5-2. 偽の `X-Forwarded-For` を付けてジョブを作る

```powershell
Set-Content -Path job.json -Value '{"size_bytes":1000000,"content_type":"video/mp4","start_ms":0,"end_ms":5000}' -Encoding ascii
$cid = [guid]::NewGuid().ToString()
curl.exe -4 -i -X POST https://chickren.goodbye-background.com/video-jobs `
  -H "Content-Type: application/json" `
  -H "client-id: $cid" `
  -H "X-Forwarded-For: 6.6.6.6" `
  --data-binary "@job.json"
Remove-Item job.json
```

- 1 行目が `HTTP/1.1 201 Created`（または `HTTP/2 201`）なら OK です
- このリクエストは **回数を消費しません**（回数を使うのは動画を送信して処理を始めるとき）。作られたジョブは使われないまま 30 分で自動的に消えます
- 応答の本文の `upload_url` は署名付き URL です。チャットには貼らないでください
- 503 なら手順 4-4 の「無効」です。500 なら回数の DB（PostgreSQL）への接続を疑い、起動ログ・エラーログを確認してください
  （このリクエストでは R2 に接続しません。アップロード用の URL はサーバーの中で計算するだけなので、R2 の設定の誤りは
  手順 7 でブラウザから動画をアップロードしたときに初めて分かります）

### 5-3. ログの `[client-ip-debug]` の行を見る

```powershell
gcloud run services logs read $SERVICE --region $REGION --limit 200 | Select-String "client-ip-debug"
```

（`gcloud run services logs read` が無いと言われたら `gcloud beta run services logs read` に変えるか、
Logs Explorer で `textPayload:"client-ip-debug"` と検索してください）

次のような行が出ます:

```
[client-ip-debug] xff="6.6.6.6, 203.0.113.7" peer=169.254.1.1:12345 hops=1 chosen=203.0.113.7
```

| 項目 | 意味 |
| --- | --- |
| `xff` | 届いた `X-Forwarded-For` そのもの（左端の `6.6.6.6` が自分で付けた偽の値） |
| `peer` | サーバーに直接つないできた相手（Cloud Run の中継の内部アドレス） |
| `hops` | 今の `TRUSTED_PROXY_HOPS` |
| `chosen` | 回数制限に使われる IP |

### 5-4. N を決める

`xff` をカンマで区切り、**右から数えて何番目が手順 5-1 の自分の IP か** を見ます:

| `xff` の形 | 決める値 |
| --- | --- |
| `"6.6.6.6, <自分の IP>"`（右端が自分の IP。`chosen` = 自分の IP） | `TRUSTED_PROXY_HOPS=1`（見込み通り） |
| `"6.6.6.6, <自分の IP>, <別の IP>"`（右から 2 番目が自分の IP。前段に Cloudflare などの中継がある） | `TRUSTED_PROXY_HOPS=2` |
| 右から N 番目が自分の IP | `TRUSTED_PROXY_HOPS=N` |
| 自分の IP がどこにも無い・`chosen` が `6.6.6.6` になる（IPv6 のアドレスで届いていて IPv4 と比べている可能性もある。`-4` を付け忘れていないか確認） | **決めずに**、その行（IP は伏せて可）をチャットに貼って相談 |

決めた値を設定し、**同時に確認用のログを外します**（`N` を置き換える）:

```powershell
gcloud run services update $SERVICE --region $REGION `
  --update-env-vars TRUSTED_PROXY_HOPS=N `
  --remove-env-vars LOG_CLIENT_IP_DEBUG
```

**確認**:
- 新しいリビジョンの起動ログに「動画切り抜き: 有効」があり、「IP の確認用ログ」の行が**無い**
- 手順 5-2 をもう一度実行して 201 が返り、手順 5-3 で**新しい** `[client-ip-debug]` の行が増えていない
- `N` を 1 以外にした場合は、外す前にもう一度 5-2・5-3 を行い、`chosen` が自分の IP になることを確かめてから外してもよい

### 以後のデプロイ（2 回目以降はこちら）

環境変数とシークレットはサービスに保存されていて、デプロイし直しても残ります。**2 回目以降は動画の設定を付けずに**デプロイしてください
（手順 4-3 のコマンドを使い回すと、確認用のログが復活し、`TRUSTED_PROXY_HOPS` が `1` に戻ります）:

```powershell
$TAG   = "video-" + (Get-Date -Format "yyyyMMdd-HHmm")
$IMAGE = "$REGION-docker.pkg.dev/$PROJECT/<REPO>/<IMAGE>:$TAG"
docker build -t goodbye-background:$TAG -f DockerFile .
docker tag goodbye-background:$TAG $IMAGE
docker push $IMAGE
gcloud run deploy $SERVICE --image $IMAGE --region $REGION --max-instances 1
```

（いつものデプロイのコマンドに他のオプションがあれば付けたままで構いません。`--set-env-vars`・`--clear-env-vars` は使わないこと）

---

## 6. 移植の正しさのテスト（自分の PC で）

ワーカーの追跡（`cutout/tracker.py`）が、スパイクの `bench.py` と同じマスクを出すことを確かめる遅いテストです。
Hugging Face からモデルを取得するので、開発のセッションでは実行できませんでした。自分の PC で一度だけ実行してください。

前提: `uv` と **`ffmpeg`**（PATH が通っていること。テスト用の短い動画を作るのに使う）が入っている。
`goodbye-background-tmp` フォルダ（`bench.py` と、それが読み込む `convert.py` がある）が手元にある。

```powershell
cd <Goodbye_Background のフォルダ>\worker

# モデルの置き場所（どこでもよい。リポジトリの外を推奨）
$env:SAM2_MODEL_DIR = "$HOME\sam2-tiny-video-models"
uv run python -c "import os; from pathlib import Path; from cutout.models import fetch_models, MODEL_REVISION; fetch_models(Path(os.environ['SAM2_MODEL_DIR']), MODEL_REVISION)"

# bench.py の場所
$env:BENCH_PY = "<goodbye-background-tmp のフォルダ>\bench.py"

uv run pytest -m slow tests/test_tracker_parity.py -v -rs
```

- モデル（`constants.json` と `onnx/` の 5 つ）の取得は初回だけ時間がかかります。2 回目以降は取得済みのファイルを飛ばします
- `SKIPPED` は合格ではありません。`-rs` で表示される理由（`BENCH_PY が未設定`・`bench.py を import できない: …` など）を直して実行し直してください。
  `bench.py` が必要とするパッケージが worker の環境に無い場合は import できずに SKIPPED になります（その場合は理由の行をチャットに貼ってください）

**確認**: `1 passed`（全 10 フレームで IoU ≥ 0.99）。`FAILED` の場合は `フレーム i: IoU 0.xxxx` の行をチャットに貼ってください。

---

## 7. iPhone での通しの確認

iPhone 13 / iOS 18 の Safari で https://chickren.goodbye-background.com/video.html を開きます
（トップページからのリンクはまだ隠してあります。この確認が終わったら戻します）。

用意した 4 種類の動画（HDR・60fps・被写体が途中から入る・緑っぽい服）で、次を確認します:

- [ ] 動画を選ぶ → 範囲を決める → プレビューの静止画が**空（真っ黒・真っ白）にならず**表示される → 被写体をタップ → 送信できる
- [ ] 処理中に**ホーム画面や他のアプリに切り替えて**しばらくしてから戻っても、進捗の表示が続き、最後まで完了する
- [ ] 完了後にダウンロードし、**写真アプリに保存**できる（保存までの操作手順も控えておく）
- [ ] 保存した動画を **CapCut** に入れてクロマキーを掛け、**背景だけが抜けて被写体とロゴが残る**。背景色 **緑・青・マゼンタ** の 3 種類で確認
- [ ] **4 回目**の送信（同じ日・同じ iPhone）で「今日の回数を使い切りました（1日3回まで）。明日また試してください」が出る
- [ ] 送信してから受け取れるまでの時間を測る:
  - **コールド**（しばらく（10 分以上）使っていない状態からの 1 本目）: ___ 秒
  - **ウォーム**（続けて送った 2 本目）: ___ 秒

（任意）回数が本番の PostgreSQL に記録されていることの確認。いつも使っている方法で DB につなぎ、次を実行します:

```sql
SELECT day, key_kind, count FROM video_quota_usage ORDER BY day DESC, key_kind;
```

今日の `day` に `client` と `ip` の行があり、送信した回数（失敗して回数が戻った分を除く）が `count` に入っていれば OK です
（`key` の列には IP と client-id が入るので、チャットには貼らないでください）。

**確認**: 上のチェックがすべて付いた。うまくいかなかった項目は、画面に出た文言・何本目の動画か・どの動画かを控える。

---

## 8. チャットに貼るもの

**貼るもの**（秘密は含まれません）:

1. 手順 4-4: 起動ログの「動画切り抜き: 有効」が出たか
2. 手順 5: 決めた `TRUSTED_PROXY_HOPS` の値と、`[client-ip-debug]` の行の **形** だけ。
   本当の IP は不要です。例: `xff="6.6.6.6, <自分の IP>" hops=1 chosen=<自分の IP>` → 「chosen は自分の IP と一致した」
3. 手順 5 の後: `LOG_CLIENT_IP_DEBUG` を外したか
4. 手順 6: `1 passed` か、`FAILED` / `SKIPPED` ならその行
5. 手順 7: チェックリストの結果、コールド / ウォームの時間、うまくいかなかった項目の文言

**貼らないもの**: R2 の Secret Access Key、Modal の Token Secret（`ws-…`）、`WORKER_CALLBACK_SECRET`、
署名付き URL（`X-Amz-Signature` を含む URL）、自分のグローバル IP、DB の `key` 列の値。
