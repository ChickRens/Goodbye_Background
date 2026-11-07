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

**アップロードの途中で CORS エラーが出るとき**: 多くの場合、本当の原因は **R2 が返したエラー**です。
R2 がエラー（403 など）を返すと、その応答には CORS のヘッダーが付かないので、ブラウザは中身を見せず「CORS エラー」とだけ表示します。
CORS の設定を疑う前に、ブラウザを通さずに `curl.exe` で同じ URL へ PUT して、R2 の応答を直接見てください
（Windows の PowerShell では `curl` ではなく **`curl.exe`**）:

1. ブラウザの開発者ツール → Network で `POST /video-jobs` の応答を開き、`upload_url` の値をコピーする
2. 小さいファイルで PUT する（`content-type` は動画を選んだときと同じものに）:

```powershell
curl.exe -i -X PUT -H "content-type: video/mp4" --data-binary "@C:\path\to\small.mp4" "<upload_url>"
```

- `200`: R2 は受け付けた。CORS の設定（`AllowedOrigins` が `https://chickren.goodbye-background.com` と一致しているか）を見直す
- `InvalidArgument`（Authorization の形式が違う、など）: **アクセスキー（`R2_ACCESS_KEY_ID`）が違う、または短い**（手順 2-2 の貼り付けの問題で 1 文字になっていた）
- `SignatureDoesNotMatch`: **`R2_SECRET_ACCESS_KEY` が違う**（長さが 64 か確かめる）。`content-type` が署名と違う場合も同じ
- 応答の本文（XML）に原因が書かれています。`upload_url` には署名が含まれるので、チャットに貼らないでください

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

### 2-2. シークレットを作る（クリップボードから登録する）

**注意**: `Read-Host -AsSecureString` に Ctrl+V で貼り付ける方法は、Windows の PowerShell では **1 文字しか入らない**ことがあります
（本番で、全てのシークレットの長さが 1 になった）。ここではクリップボードから直接読む関数を使います。

次の関数を貼り付けます（Enter を押すと定義されるだけで、まだ何も起きません）:

```powershell
function Add-VideoSecretVersion([string]$Name, [int]$ExpectedLength = 0) {
  $plain = "$(Get-Clipboard -Raw)".Trim()
  if ($plain.Length -eq 0) {
    Write-Host "クリップボードが空です（値をコピーしてから実行）" -ForegroundColor Red
    return
  }
  Write-Host "$Name : $($plain.Length) 文字"
  if ($ExpectedLength -gt 0 -and $plain.Length -ne $ExpectedLength) {
    Write-Host "長さが $ExpectedLength ではないので登録しません" -ForegroundColor Red
    Set-Clipboard -Value " "
    return
  }
  $file = New-TemporaryFile
  try {
    [System.IO.File]::WriteAllText($file.FullName, $plain)
    gcloud secrets versions add $Name --data-file="$($file.FullName)"
  } finally {
    Remove-Item $file.FullName -Force
    $plain = $null
    Set-Clipboard -Value " "
  }
}
```

- 改行を付けずに一時ファイルへ書きます（PowerShell のパイプで渡すと末尾に改行が付き、署名が合わなくなるため）
- 登録が終わると（長さが合わずに登録をやめたときも）クリップボードを空白 1 文字で上書きします（秘密が残らないように）

**使い方は「コピーしてから実行」の繰り返しです**: 1 つ目の値をコピー → 実行 → 次の値をコピー → 実行。
実行する前に必ず、その値をコピーし直してください（クリップボードは実行のたびに空白 1 文字で上書きされるので、コピーし忘れると「クリップボードが空です」と出て、何も登録されません）。

**初めて作るとき**は、先に空のシークレットを作ってから版を足します:

```powershell
gcloud secrets create R2_ACCESS_KEY_ID --replication-policy=automatic      # 値は入れない（空の入れ物だけ作る）
# R2 の Access Key ID をコピーしてから:
Add-VideoSecretVersion R2_ACCESS_KEY_ID 32
```

5 つ全てについて同じ手順です（`gcloud secrets create` は最初の 1 回だけ。2 回目以降の値の差し替えは `Add-VideoSecretVersion` だけ）:

| 名前 | 中身 | 期待する長さ（`Add-VideoSecretVersion` の第 2 引数） |
| --- | --- | --- |
| `R2_ACCESS_KEY_ID` | 手順 1-4 の Access Key ID | 32 |
| `R2_SECRET_ACCESS_KEY` | 手順 1-4 の Secret Access Key | 64 |
| `WORKER_CALLBACK_SECRET` | Modal の Secret `goodbye-background-video` に入れたのと同じ値（`worker/README.md` の手順 1 で、ランダムな 32 バイトを 16 進数にした 64 文字を作っている） | 64 |
| `MODAL_KEY` | Modal の Token ID（`wk-…`） | 決まった長さは無い（指定しない） |
| `MODAL_SECRET` | Modal の Token Secret（`ws-…`） | 決まった長さは無い（指定しない） |

- `R2_ACCESS_KEY_ID` と `MODAL_KEY` は厳密には秘密ではありません（対になる Secret が無いと使えない）が、まとめて管理するためここに入れます。
  環境変数に直接書いても動きます（その場合は手順 3 の `--update-env-vars` 側に移す）

**登録した長さの確認**（値は表示されず、長さだけが出ます）:

```powershell
foreach ($n in "R2_ACCESS_KEY_ID","R2_SECRET_ACCESS_KEY","MODAL_KEY","MODAL_SECRET","WORKER_CALLBACK_SECRET") {
  "$n : $((gcloud secrets versions access latest --secret=$n).Length)"
}
```

R2 の 3 つが 32 / 64 / 64 になっていれば OK です（`1` になっていたら、上の貼り付けの問題です）。

**値を間違えたとき・差し替えるとき**:

1. 正しい値をコピーして `Add-VideoSecretVersion <名前> [長さ]` を実行する（新しい版が足される）
2. 古い版を無効にする: `gcloud secrets versions disable <版の番号> --secret=<名前>`（版の番号は `gcloud secrets versions list <名前>`）
3. **Cloud Run は `:latest` を起動時に読むので、動いているサービスに新しい版を反映させるには更新が必要**です（新しいリビジョンが作られます）:

```powershell
gcloud run services update $SERVICE --region $REGION --update-secrets "R2_ACCESS_KEY_ID=R2_ACCESS_KEY_ID:latest,R2_SECRET_ACCESS_KEY=R2_SECRET_ACCESS_KEY:latest"
```

（差し替えたシークレットだけを `名前=名前:latest` の形で並べる。手順 3 の `$VIDEO_SECRETS` を全部渡しても構いません）

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

**確認**: `gcloud secrets list` に 5 つの名前が出る。`gcloud secrets versions list WORKER_CALLBACK_SECRET` で有効な版がある。
（値そのものを表示する `gcloud secrets versions access` は、画面に出るので、手順 2-2 の長さだけを出す形以外では使わないでください）

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

**起動ログに「動画切り抜き: 無効」と出たとき**:

- 無効の行の直前に「動画切り抜き: 環境変数が足りない: R2_ACCOUNT_ID, MODAL_KEY」のように、**足りない変数の名前**が出ます（値は出ません）。
  その名前を、上の表の渡し方（環境変数／シークレット）で足してください
- **`--update-env-vars` / `--update-secrets` を付けないデプロイのコマンドは、これらを設定しません**。本番ではこれが原因で無効になりました
  （手順 4-3 の初回のコマンドに `$VIDEO_ENV` / `$VIDEO_SECRETS` を付け忘れた）。
  後から足すなら `gcloud run services update $SERVICE --region $REGION --update-env-vars $VIDEO_ENV --update-secrets $VIDEO_SECRETS`
- 以前の Google ログインのブランチの名残の **`GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET` は今のコードでは使いません**。残っていたら消します:
  `gcloud run services update $SERVICE --region $REGION --remove-env-vars GOOGLE_CLIENT_ID,GOOGLE_CLIENT_SECRET`
  （シークレットとして渡している場合は `--remove-secrets GOOGLE_CLIENT_ID,GOOGLE_CLIENT_SECRET`。付いているかは `gcloud run services describe $SERVICE --region $REGION` で見られます）

---

## 4. このブランチをビルドしてデプロイする

### 4-1. コードを取ってくる

```powershell
cd <Goodbye_Background のフォルダ>
git fetch origin
git switch claude/video-crop-handoff-1i1r2g
git pull
```

**マイグレーションのファイルを取り直して、本番と同じバイト列か確かめる**（ビルドの前に必ず）:

本番の DB は、適用したマイグレーションのファイルのチェックサム（SHA-384）を覚えていて、起動時にイメージの中のファイルと比べます。
1 文字でも（改行コードの CRLF / LF の違いでも）違うと起動しません。このブランチには改行を変換しない設定（`.gitattributes`）が
入っていますが、**既にチェックアウト済みのファイルには効かない**ので、一度消して取り直します:

```powershell
Remove-Item -Recurse -Force migrations
git checkout -- migrations
Get-FileHash -Algorithm SHA384 migrations\postgres\20260919032531_usage_records.sql
Get-FileHash -Algorithm SHA384 migrations\postgres\20261004122625_add_client_id_to_usage_records.sql
Get-FileHash -Algorithm SHA384 migrations\postgres\20261101090000_users_and_login_sessions.sql
```

**確認**: `Hash` の**先頭 16 文字**が次と同じ（PowerShell は大文字で表示します。大文字・小文字は区別しません）:

| ファイル | 先頭 16 文字 |
|---|---|
| `20260919032531_usage_records.sql` | `7D1303A9617D0B61` |
| `20261004122625_add_client_id_to_usage_records.sql` | `2085933DA5D32124` |
| `20261101090000_users_and_login_sessions.sql` | `D7CBB94D73748D77` |

- 3 つとも一致したらビルドに進んでください。違う場合はビルドせず、`git status` に migrations の変更が出ていないか確認し、チャットで相談してください
- 新しく足した `20261106000000` / `20261107000000` は、最初にデプロイしたときのバイト列で本番に記録され、以後は変えてはいけません
  （`.gitattributes` があるので、Windows でもリポジトリと同じバイト列になり、変わりません）

### 4-2. モデルがあることを確かめる

`DockerFile` は `models/` をイメージにコピーします（無いとビルドが失敗するか、起動時に落ちます）:

```powershell
Get-ChildItem models
```

`sam2.1_hiera_small_image_encoder.onnx`・`sam2.1_hiera_small_prompt_encoder.onnx`・`sam2.1_hiera_small_mask_decoder.onnx`・`image_pe.bin` の 4 つがあれば OK です。

**動画のプレビュー用のモデル（SAM2.1 tiny）を取得する**（初回だけ。手順 9-1 と同じ）。無いと動画機能ごと無効になります:

```powershell
cd worker
uv run python scripts/fetch_preview_models.py
cd ..
Get-ChildItem -Recurse models\sam2.1-tiny-video
```

`constants.json`・`onnx\vision_encoder.onnx`・`onnx\mask_decoder.onnx` の 3 つがあれば OK です。

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

「動画切り抜き: 無効（環境変数かプレビューのモデルが足りないため、動画の API は 503 を返す）」と出た場合は、その前の行に理由が出ています:
「動画切り抜き: 環境変数が足りない: …」なら手順 3 の値のどれかが欠けている、
「動画切り抜き: プレビューのモデルが無い: …」なら手順 4-2 のプレビュー用のモデルが足りない
（`TRUSTED_PROXY_HOPS` が数値でない場合は「TRUSTED_PROXY_HOPS が数値ではないため…」の行が出ます）。
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
（手順 4-3 のコマンドを使い回すと、確認用のログが復活し、`TRUSTED_PROXY_HOPS` が `1` に戻ります）。
ビルドの前に、手順 4-1 の「マイグレーションのファイルの確認」（`Get-FileHash` の 3 行）も行ってください:

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
6. 手順 9: 起動ログの「動画切り抜き: 有効」が出たか、9-2 を行ったならその結果、9-4 のチェックの結果とプレビューにかかった時間

**貼らないもの**: R2 の Secret Access Key、Modal の Token Secret（`ws-…`）、`WORKER_CALLBACK_SECRET`、
署名付き URL（`X-Amz-Signature` を含む URL）、自分のグローバル IP、DB の `key` 列の値。

---

## 9. プレビューを本番と同じモデルにする（2026-11-07 の更新。デプロイ済みならここだけ）

動画ページのプレビュー（手順 4 の「残す所をタップ」）を、画像機能のモデル（SAM2.1 small）から、
本番の GPU ワーカーと同じ **SAM2.1 tiny** に変えました。プレビューで残った範囲が、そのまま出力の 1 フレーム目になります
（以前は、ベストを 1 点タップするとプレビューは人物全体・本番はベストだけ、と食い違いました）。
tiny は Cloud Run の CPU で動かすので、モデルのファイルをイメージに入れる必要があります。

### 9-1. プレビュー用のモデルを取得する

ワーカーと同じリビジョンに固定したモデルから、プレビューで使う 3 つだけを `models\sam2.1-tiny-video\` に取得します
（取得済みのファイルは飛ばすので、何度実行しても構いません）:

```powershell
cd <Goodbye_Background のフォルダ>
git switch claude/video-crop-handoff-1i1r2g
git pull

cd worker
uv run python scripts/fetch_preview_models.py
cd ..
Get-ChildItem -Recurse models\sam2.1-tiny-video
```

**確認**: 次の 3 つがあり、どれも 0 バイトではない:

```
models\sam2.1-tiny-video\constants.json
models\sam2.1-tiny-video\onnx\vision_encoder.onnx
models\sam2.1-tiny-video\onnx\mask_decoder.onnx
```

（`models\` の中の small の 4 つ（手順 4-2）もそのまま残しておきます。`models\` は git 管理外です）

### 9-2. （任意）自分の PC で実モデルのテストを動かす

**Rust（`cargo`）が入っている場合だけ**行います。入っていなければ飛ばして 9-3 へ進んでください:

```powershell
$env:SAM2_TINY_MODEL_DIR = "models/sam2.1-tiny-video"
cargo test -- --ignored tiny_model
Remove-Item Env:SAM2_TINY_MODEL_DIR
```

**確認**: `test result: ok. 1 passed`。

### 9-3. ビルド・デプロイ

手順 4-1 の「マイグレーションのファイルの確認」（`Get-FileHash` の 3 行）をしてから、手順 5 の後の
**「以後のデプロイ」のコマンド**でビルド・デプロイします（動画の設定は付けません）。

**確認**: 新しいリビジョンの起動ログに `動画切り抜き: 有効` が出ている。
`動画切り抜き: プレビューのモデルが無い: …` と出たら、9-1 のファイルが無い状態でビルドしています（9-1 からやり直す）。

### 9-4. プレビューと仕上がりが一致するかの確認

iPhone の Safari で https://chickren.goodbye-background.com/video.html を開き、**以前プレビューと仕上がりが食い違った同じ動画**で:

- [ ] 前と同じ所（例: ベスト）を 1 点タップすると、プレビューで残る範囲が**タップした物だけ**になる
- [ ] 送信して出来上がった動画の **1 フレーム目**で残っている範囲が、プレビューで残った範囲と**同じ**
- [ ] 「この点では切り抜く対象が見つかりませんでした…」が出たときは、送信ボタンが押せない
  （背景をタップすると出ることがあります。背景でも何かを選んで出ないこともあるので、出なければ飛ばして構いません）
- [ ] 点を打つ・Undo・Redo のたびにプレビューが数秒以内に変わる（かかった時間の目安: ___ 秒）

**確認**: 上のチェックがすべて付いた。食い違った場合は、動画・タップした位置・プレビューと 1 フレーム目のスクリーンショットを控える。
結果は手順 8 の 6 のとおりチャットに貼ってください。
