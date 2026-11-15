# Cloud Run への自動デプロイ（GitHub Actions）

`master` にマージされると、`.github/workflows/deploy-cloud-run.yml` が自動で次を行います:

1. GitHub の Variables が揃っているか確かめる
2. 本番が適用したマイグレーション 3 つが同じバイト列か確かめる（CLAUDE.md の 13。違えばデプロイしない）
3. Google Cloud に認証する（Workload Identity 連携。鍵ファイルは使わない）
4. 画像のモデルを GCS から、動画のプレビュー用モデルを Hugging Face から取ってくる
5. `DockerFile` でビルドして Artifact Registry にプッシュする（タグはコミットの SHA）
6. `gcloud run deploy`（環境変数・シークレットの参照を毎回入れ直す。`--max-instances 1`）
7. トップページが 200 を返すか、起動ログに「動画切り抜き: 有効」が出たかを確かめる

途中で失敗したら、その後の手順は動かないので、**本番は前のリビジョンのまま**です。
Actions の画面（リポジトリの **Actions** タブ → **Deploy to Cloud Run**）から手動で流すこともできます（**Run workflow**）。

以下は **最初の 1 回だけ** の準備です。Windows の PowerShell と `gcloud` を前提に書いています。
秘密の値は GitHub にも置きません（Secret Manager の名前で参照させるだけ。`docs/video-cutout-deploy.md` の手順 2 で作ったもの）。

---

## 0. 変数を入れる

```powershell
$PROJECT = "<PROJECT>"     # いつも使っているプロジェクト ID
$REGION  = "<REGION>"      # 例: asia-northeast1
$SERVICE = "<SERVICE>"     # Cloud Run のサービス名
$REPO    = "<REPO>"        # Artifact Registry のリポジトリ名
$GH_REPO = "ChickRens/Goodbye_Background"   # 大文字・小文字もこの通り
gcloud config set project $PROJECT
$NUM = gcloud projects describe $PROJECT --format="value(projectNumber)"
```

## 1. 画像のモデルを GCS に置く

画像のモデル（`scripts/` の Python で作ったもの）は git に入っていないので、GitHub Actions からは GCS で受け取ります。
**モデルを作り直したときは、このアップロードだけやり直してください。**

```powershell
$BUCKET = "gs://$PROJECT-goodbye-background-models"
gcloud storage buckets create $BUCKET --location=$REGION --uniform-bucket-level-access
gcloud storage cp models\sam2.1_hiera_small_image_encoder.onnx models\sam2.1_hiera_small_prompt_encoder.onnx `
  models\sam2.1_hiera_small_mask_decoder.onnx models\image_pe.bin "$BUCKET/models/"
gcloud storage ls "$BUCKET/models/"
```

**確認**: 4 つのファイルが出る。

- 動画のプレビュー用モデル（`models/sam2.1-small-video/`）は上げなくて構いません。
  ワーカーと同じリビジョン（`worker/cutout/models.py` の `MODEL_REVISION`）を毎回 Hugging Face から取ります（2 回目以降はキャッシュ）

## 2. デプロイ用のサービスアカウントを作る

```powershell
gcloud iam service-accounts create github-deployer --display-name="GitHub Actions deploy"
$DEPLOYER = "github-deployer@$PROJECT.iam.gserviceaccount.com"

# Cloud Run を更新する
gcloud projects add-iam-policy-binding $PROJECT --member="serviceAccount:$DEPLOYER" --role="roles/run.developer"
# イメージをプッシュする
gcloud artifacts repositories add-iam-policy-binding $REPO --location=$REGION `
  --member="serviceAccount:$DEPLOYER" --role="roles/artifactregistry.writer"
# モデルを読む
gcloud storage buckets add-iam-policy-binding $BUCKET --member="serviceAccount:$DEPLOYER" --role="roles/storage.objectViewer"
# 起動ログを読む（最後の確認用。無くてもデプロイはできる）
gcloud projects add-iam-policy-binding $PROJECT --member="serviceAccount:$DEPLOYER" --role="roles/logging.viewer"
```

Cloud Run が動くときのサービスアカウント（実行用）として振る舞う権限も要ります:

```powershell
$SA = gcloud run services describe $SERVICE --region $REGION --format="value(spec.template.spec.serviceAccountName)"
if (-not $SA) { $SA = "$NUM-compute@developer.gserviceaccount.com" }   # 空ならデフォルトのもの
gcloud iam service-accounts add-iam-policy-binding $SA --member="serviceAccount:$DEPLOYER" --role="roles/iam.serviceAccountUser"
```

- シークレットを読む権限は実行用のサービスアカウントに既に付いています（`docs/video-cutout-deploy.md` の手順 2-3）。デプロイ用には要りません

## 3. GitHub から入れるようにする（Workload Identity 連携）

**この `master` ブランチのこのリポジトリからだけ** 入れるように条件を付けます。

```powershell
gcloud services enable iamcredentials.googleapis.com sts.googleapis.com

gcloud iam workload-identity-pools create github --location=global --display-name="GitHub Actions"

gcloud iam workload-identity-pools providers create-oidc github `
  --location=global --workload-identity-pool=github `
  --issuer-uri="https://token.actions.githubusercontent.com" `
  --attribute-mapping="google.subject=assertion.sub,attribute.repository=assertion.repository,attribute.ref=assertion.ref" `
  --attribute-condition="assertion.repository=='$GH_REPO' && assertion.ref=='refs/heads/master'"

gcloud iam service-accounts add-iam-policy-binding $DEPLOYER `
  --role="roles/iam.workloadIdentityUser" `
  --member="principalSet://iam.googleapis.com/projects/$NUM/locations/global/workloadIdentityPools/github/attribute.repository/$GH_REPO"

# 手順 4 で使う 2 つの値を表示する
"GCP_WIF_PROVIDER = projects/$NUM/locations/global/workloadIdentityPools/github/providers/github"
"GCP_DEPLOY_SA    = $DEPLOYER"
"MODELS_GCS_URI   = $BUCKET/models"
```

## 4. GitHub の Variables を入れる

GitHub のリポジトリ → **Settings** → **Secrets and variables** → **Actions** → **Variables** タブ → **New repository variable**。
（**Secrets** タブではなく **Variables** タブ。ここに入れるのは秘密ではない値だけです）

| 名前 | 値 |
|---|---|
| `GCP_PROJECT` | `$PROJECT` の値 |
| `GCP_REGION` | `$REGION` の値 |
| `CLOUD_RUN_SERVICE` | `$SERVICE` の値 |
| `AR_REPO` | `$REPO` の値 |
| `AR_IMAGE` | いつものイメージ名 |
| `GCP_WIF_PROVIDER` | 手順 3 の最後に表示された値 |
| `GCP_DEPLOY_SA` | 手順 3 の最後に表示された値 |
| `MODELS_GCS_URI` | 手順 3 の最後に表示された値（`gs://…/models`） |
| `R2_ACCOUNT_ID` | R2 の Account ID |
| `R2_BUCKET` | `goodbye-background-video` |
| `MODAL_ENDPOINT_URL` | `https://chickrens--goodbye-background-video-web.us-east.modal.run` |
| `PUBLIC_BASE_URL` | `https://chickren.goodbye-background.com` |
| `TRUSTED_PROXY_HOPS` | `1`（本番の値。CLAUDE.md の 15） |
| `LOG_CLIENT_IP_DEBUG` | **入れない**（IP の確認をするときだけ `1`。確認が終わったら消す。消してから次のデプロイで外れる） |

- 値は今の Cloud Run の設定と同じにしてください（`gcloud run services describe $SERVICE --region $REGION` で見られます）。
  デプロイのたびに **この値で上書き** されます
- `DATABASE_URL_POSTGRES` などここに無い設定は触りません（`--update-env-vars` は足す・上書きするだけ）

## 5. 初回を手動で流す

リポジトリの **Actions** タブ → **Deploy to Cloud Run** → **Run workflow**（ブランチは `master`）。

**確認**:

- 全部の手順が緑になる（最後の「起動ログで動画切り抜きが有効か確かめる」だけは、黄色の警告でもデプロイ自体は済んでいます）
- https://chickren.goodbye-background.com が開ける

これで、以後は **PR をマージするだけ** でデプロイされます。

---

## うまくいかないとき

| 出た場所 | 症状 | 見るところ |
|---|---|---|
| 設定の確認 | `GitHub の Variables が足りない: …` | 手順 4。名前のつづり（Secrets タブに入れていないか） |
| マイグレーションの確認 | `FAILED` | `migrations/postgres/` の古い 3 つが書き換えられた。**デプロイしてはいけない**（CLAUDE.md の 13） |
| 認証 | `unauthorized_client` / `attribute condition` | 手順 3 の `$GH_REPO`（大文字・小文字）、マージ先が `master` か |
| 認証 | `Permission 'iam.serviceAccounts.getAccessToken' denied` | 手順 3 の最後の `add-iam-policy-binding` |
| モデル | `AccessDenied` / `モデルが足りない` | 手順 1 のアップロードと、手順 2 の `storage.objectViewer` |
| プッシュ | `denied: Permission "artifactregistry.repositories.uploadArtifacts"` | 手順 2 の `artifactregistry.writer` |
| デプロイ | `iam.serviceaccounts.actAs` | 手順 2 の `serviceAccountUser` |
| デプロイ | `Permission denied on secret` | 実行用のサービスアカウントの権限（`docs/video-cutout-deploy.md` の手順 2-3） |
| 起動ログ | `動画切り抜きが有効になっていない` | その上に足りない環境変数・モデルの名前が出ている |

- 一つ前の状態に戻すには、Cloud Run のコンソール → サービス → **リビジョン** → 前のリビジョンにトラフィックを 100% 向ける
- ビルドは Rust のコンパイルで 1 回 15〜25 分ほどかかります。非公開リポジトリだと GitHub Actions の無料枠（Free プランで月 2,000 分）を使います
