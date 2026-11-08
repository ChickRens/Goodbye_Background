# Modal での計測手順（Task 1）

動画切り抜きのワーカーを Modal の GPU（L4）で動かしたときの
**コールドスタート（GPU の起動 + モデルの読み込み）** と **1 本あたりの処理時間・費用** を測ります。
本番と同じ部品（`worker/cutout/`）で、デコード → SAM2 の追跡 → 緑背景との合成 + ロゴ → mp4 への書き出し を 1 本通します。

この計測用のコードは使い捨てです（後の作業には使いません）。

> Windows 11 の PowerShell を前提に書いています。コマンドはすべて **リポジトリの直下**（`Goodbye_Background` フォルダ）で実行してください。

---

## 1. 準備（最初の 1 回だけ）

### 1-1. Modal を入れてログインする

```powershell
python --version          # 3.10 以上なら OK（手元の Python は Modal の CLI を動かすだけ）
pip install "modal>=1.6,<2"
modal setup               # ブラウザが開くので Modal のアカウントでログインして許可する
```

`modal setup` がうまく動かないときは `python -m modal setup` を試してください。

### 1-2. 予算の上限を $30 にする（必須）

使いすぎを防ぐため、計測の前にワークスペースの月の上限を設定します。

1. ブラウザで https://modal.com にログインし、右上のワークスペース名から **Settings** を開く
2. **Usage / Billing**（「Usage」「Billing」「Plan」などの名前のことがあります）を開く
3. **Budget limit**（予算の上限）に **30**（USD）を入れて保存する

> Modal の画面は変わることがあるので、メニューの名前は実際の画面で確かめてください。
> 「上限に達したら止まる」設定になっていることを確認してください。

### 1-3. 文字化け対策

結果の表は日本語です。PowerShell で文字化けする場合は、計測の前に次を実行してください。

```powershell
$env:PYTHONUTF8 = "1"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
```

---

## 2. 計測に使う動画（4 本）

iPhone 13 で撮った動画を 4 本用意してください（長さは 10〜20 秒程度で十分です。計測は先頭 10 秒だけ使います）。

| # | 内容 | 撮り方 |
|---|---|---|
| A | 人物（HDR） | カメラの標準設定のまま撮る（iPhone の標準は HDR / Dolby Vision） |
| B | 60fps | 設定 → カメラ → ビデオ撮影 で「1080p HD/60 fps」にして撮る |
| C | 途中から人が入ってくる | 最初は人が画面の端に少しだけ写っていて、途中で真ん中に歩いてくる |
| D | 緑っぽい服 | 緑系の服を着た人を撮る（緑背景との合成で服が抜けないかを見る） |

PC に移すときは「写真」アプリや iCloud から **元のファイル（.MOV）** を取り出してください（LINE などで送ると再圧縮されて HDR が消えます）。

### 被写体の位置 `--x` `--y` の決め方

最初のフレーム（`--start` の時刻のフレーム）で、**切り抜きたい人物の体の真ん中あたり**を 0〜1 の割合で指定します。

- `--x` … 左端 = 0、右端 = 1
- `--y` … 上端 = 0、下端 = 1
- 例: 縦長の動画で人物が真ん中に立っている → `--x 0.5 --y 0.5`
- 例: 人物が右寄り・胸のあたりが画面の上から 4 割 → `--x 0.7 --y 0.4`

動画 C は、最初のフレームで写っている体の一部（画面の端）を指定してください。
動画を「フォト」アプリで一時停止し、スクリーンショットを見て目分量で決めれば十分です。

---

## 3. 計測する

### 3-1. 1 回目（イメージのビルド）

最初の実行では Modal 側でイメージ（CUDA・ffmpeg・onnxruntime-gpu・モデル）を作るので、**数分かかります**。
これは 1 回きりで、コールドスタートには数えません。

```powershell
modal run worker/measure/measure_app.py::main --video "C:\Users\<あなた>\Videos\A_hdr.MOV" --x 0.5 --y 0.5
```

`モデルを取得しました: diffusionstudio/sam2.1-tiny-video-onnx-fp16@<sha>` がビルドのログに出れば成功です。
ビルド直後の 1 回はイメージの配布が終わっていないことがあり、遅めに出ることがあります。

### 3-2. 本番の計測

`modal run` は実行ごとに新しいコンテナを起動するので、**各実行の 1 回目 = コールド、2 回目 = ウォーム** になります。
動画ごとに **3 回ずつ** 実行してください（コールド 3 回・ウォーム 3 回が取れます）。
前の実行が終わってから **1 分以上あけて** 次を実行すると確実です。

```powershell
# A: HDR（標準設定）
modal run worker/measure/measure_app.py::main --video "C:\Users\<あなた>\Videos\A_hdr.MOV" --x 0.5 --y 0.5 --out measure_out\A

# B: 60fps
modal run worker/measure/measure_app.py::main --video "C:\Users\<あなた>\Videos\B_60fps.MOV" --x 0.5 --y 0.5 --out measure_out\B

# C: 途中から入ってくる（x, y は 1 枚目に写っている体の一部）
modal run worker/measure/measure_app.py::main --video "C:\Users\<あなた>\Videos\C_enter.MOV" --x 0.1 --y 0.6 --out measure_out\C

# D: 緑っぽい服
modal run worker/measure/measure_app.py::main --video "C:\Users\<あなた>\Videos\D_green.MOV" --x 0.5 --y 0.5 --out measure_out\D
```

指定できるオプション:

| オプション | 既定値 | 意味 |
|---|---|---|
| `--video` | （必須） | 動画のパス。空白を含むときは `"..."` で囲む |
| `--start` / `--end` | 0 / 10 | 使う区間（秒）。最大 10 秒 |
| `--x` / `--y` | 0.5 / 0.5 | 1 枚目の被写体の位置（0〜1） |
| `--runs` | 2 | 続けて処理する回数（1 回目がコールド） |
| `--out` | `measure_out` | 結果の保存先フォルダ |
| `--image` | `cuda` | `cuda` = 1 回目の計測と同じ大きい CUDA イメージ / `slim` = 軽いイメージ（下の「再計測」を参照） |

> ファイルに入口が 2 つ（`main` と `probe`）あるので、`measure_app.py::main` のように **`::main` を付けて** 実行してください。

`measure_out\` はリポジトリにコミットしないでください（動画が入ります）。

---

## 4. 結果をチャットに貼る

実行の最後に `## 計測結果: ...` から始まる表が出ます。**表をそのまま全部コピーしてチャットに貼ってください**
（同じ内容が `--out` のフォルダの `<動画名>_report.md` にも保存されています）。
表の先頭の **「モデル sha」** は後の作業でモデルの版を固定するのに使うので、必ず含めてください。

あわせて、次も教えてください。

- Modal の画面（Usage / Billing）での、計測した日の **実際の請求額**（表の「推定費用」は GPU 分だけの概算です）
- Modal の料金説明や請求画面で、**外向きの通信（egress）やストレージがクレジットに含まれるか**分かれば、その内容

---

## 5. 出力動画を確認する

`--out` のフォルダに `<動画名>_run1.mp4`, `_run2.mp4` ができます。再生して次を確認してください。

- [ ] **色が褪せていない**（特に動画 A。元の動画と並べて、肌や空の色が白っぽく・灰色っぽくなっていないか）
- [ ] **右下にロゴ**が出ている
- [ ] 背景が **緑一色** になり、人物が切り抜かれている
- [ ] 動画 C: 人が入ってきた後も追えているか（見失ったフレームは全面が緑になります）
- [ ] 動画 D: 緑の服の部分が背景と混ざって消えていないか
- [ ] 音声が付いているか（元の動画に音声がある場合）

気づいたことを一言ずつ書いて、表と一緒に貼ってください。

---

## 再計測（原因の切り分け）

1 回目の計測では、コールドスタートが 150〜724 秒かかったのに、モデルの読み込みは 1〜3 秒でした。
時間は **Python が動く前**（Modal の画面で Pending の間）にかかっています。原因の候補は 2 つです。

- **GPU（L4）の割り当て待ち** … 空いている L4 が見つかるまで待たされている
- **イメージの起動** … CUDA の公式イメージ（数 GB）をサーバーに配って起動するのに時間がかかっている

これを切り分けるため、計測のコードを次のように変えました。

- GPU は **L4 → A10G → T4** の順に空いているものを使う（実際に付いた GPU 名は表に出ます）
- 軽いイメージ（`slim`。Debian slim + pip の CUDA / cuDNN）を追加
- 表に **「呼び出し→コンテナ起動(s)」**（呼んでからコンテナで Python が動き出すまで）の列を追加
  （計測ではこの値に動画のアップロード時間が入ることがあります）

Modal の CLI は前と同じものを使います（入っていなければ `uv tool install "modal>=1.6,<2"`）。
文字化けする場合は「1-3. 文字化け対策」を先に実行してください。

### 手順 (a): 切り分け用の小さな関数を 3 回ずつ呼ぶ（30〜60 分以上かかることがある）

```powershell
modal run worker/measure/measure_app.py::probe --rounds 3
```

- 4 つの関数を毎回この順に呼び、回の間は 90 秒待ちます（コンテナを一度落として、毎回コールドにするため）
  1. **最小イメージ・GPU あり** … GPU の割り当て待ちだけにかかる時間
  2. **CUDA イメージ・GPU あり** … 1 回目の計測と同じイメージ。1. との差がこのイメージの起動の上乗せ
  3. **slim イメージ・GPU あり** … 軽いイメージ。1. との差がこのイメージの上乗せ（モデルを CUDA で読めるかの確認も兼ねる）
  4. **CUDA イメージ・GPU なし（参考）** … 判定には使いません
- **時間について**: 1 回目の計測と同じように Pending で待たされると、1 回の呼び出しに数分〜10 分以上かかります。
  4 つ × 3 回で **合計 30〜60 分以上** になることもあります。止まっているように見えても **そのまま待って大丈夫** です
  （初回は軽いイメージのビルドも入ります。ビルドの時間は表には入りません）
- **進んでいるかの見分け方**: 呼び出しごとに `1/3 回目 gpu_tiny_image: 呼び出し中…` と
  `1/3 回目 gpu_tiny_image: 終了 全体 ... 秒` の行が出ます。最後の行が「呼び出し中…」なら、その関数を待っているところです
- 最後に `## 原因の切り分け（probe）` から始まる表と、「GPU の割り当て待ち（tiny）」「CUDA イメージの上乗せ」
  「slim イメージの上乗せ」「判定: ...」の行が出ます。**表から下をそのままチャットに貼ってください**
- 手元の PC の時計がずれていると値が数秒ずれます（上乗せは差なので打ち消されます）。気になる場合は、
  計測の前に **管理者の PowerShell** で `w32tm /resync` を実行してください（任意）

### 手順 (b): 動画 A を軽いイメージで 1 回計測する

```powershell
modal run worker/measure/measure_app.py::main --video "C:\Users\<あなた>\Videos\A_hdr.MOV" --x 0.5 --y 0.5 --image slim --out measure_out\A_slim
```

`--x` `--y` は 1 回目の計測で動画 A に使った値にしてください。
最後に出る `## 計測結果: ...` の表を **そのまま全部** チャットに貼ってください（「イメージ: slim」と GPU 名の行も含めて）。

### （任意）動画 A を HDR で撮り直す

1 回目の動画 A は HDR のつもりでしたが、入力は SDR でした。iPhone の設定を確認してください。

1. iPhone の **設定 → カメラ → ビデオ撮影 → HDR ビデオ** を開く
2. **オフ** になっていたら **オン** にして、動画 A を撮り直す（オンだった場合は撮り直し不要）
3. 撮り直したら、手順 (b) のコマンドをその動画で実行して表を貼る（表の「入力」の行が `HDR=はい` になっていれば HDR です）

---

## うまくいかないとき

| 症状 | 対処 |
|---|---|
| `modal: command not found` / 認識されない | `python -m modal run ...` で実行する |
| `CUDA を使えない` という例外 | GPU で動いていません。エラー全文を貼ってください |
| ビルドで Hugging Face からの取得に失敗 | 時間をおいて再実行。続く場合はエラー全文を貼ってください |
| 予算の上限に達した | それ以上は計測せず、そこまでの表を貼ってください |
| `has the following functions and local entrypoints` と出て止まる | コマンドに `::main` か `::probe` を付ける |
| probe の表の備考に `失敗: ...` が出た | 表と一緒に、その上に出ているエラー全文を貼ってください |
| `--image slim` で `ffmpeg に zscale が無い` という例外 | 軽いイメージの ffmpeg が HDR の変換に対応していません。エラー全文を貼ってください |

---

## 案 C: small の時間と費用を測る（2026-11-08 の追加）

速く動く腕が消える問題は、SAM2.1 **small**（入力 1024）なら直ることを確かめました（`docs/video-cutout-measurements.md`）。
CPU では tiny の約 7 倍遅かったものの、GPU での倍率は分からないので、本番と同じ GPU で測ります。
本番のワーカーは変えません（この計測用のアプリだけで tiny と small を切り替えます）。

イメージに tiny と small の両方を入れるように変えたので、**1 回目はイメージを作り直す数分**がかかります。
同じ動画・同じ点で、tiny と small を続けて測ってください（`--image slim` は本番と同じ軽いイメージ）:

```powershell
modal run worker/measure/measure_app.py::main --video "C:\Dev\goodbye_background\Goodbye_Background\worker\D_man.mp4" --start 10.7 --end 20.7 --x 0.5 --y 0.42 --image slim --model tiny --out measure_out\C_tiny
```
```powershell
modal run worker/measure/measure_app.py::main --video "C:\Dev\goodbye_background\Goodbye_Background\worker\D_man.mp4" --start 10.7 --end 20.7 --x 0.5 --y 0.42 --image slim --model small --out measure_out\C_small
```

**貼るもの**: 2 つの `*_report.md`（画面に出る結果の表と同じ）。出力の動画（`*_run2.mp4`）は貼らなくて構いません。
見るのは、**ウォーム（2 回目）の全体の秒数**と、付いた GPU の名前（L4 / A10G / T4。費用の単価が違う）です。

---

## 案 C の続き: DAM4SAM・SAMURAI・base-plus を比べる（2026-11-08 の追加）

small でも残る「同じ服の隣の人を拾う」問題に向けて、記憶の選び方を変えた 2 つの手法と、大きいモデルを比べます。

| 指定 | 中身 |
|---|---|
| `--model small --policy sam2` | 今の比較の基準（small をそのまま） |
| `--model small --policy dam4sam` | DAM4SAM（CVPR 2025）: 似た物体がいるフレームを「見分ける用の記憶」に入れる |
| `--model small --policy samurai` | SAMURAI: 被写体の動きを予測して、候補の選択と記憶の選別に使う |
| `--model base-plus --policy sam2` | 大きいモデル（入力 1024・画像エンコーダ 156MB） |

モデルの ONNX は同じで、DAM4SAM・SAMURAI は `worker/cutout/tracker_variants.py` で記憶の組み立てだけを変えています
（論文の実装との違いはそのファイルの先頭に書きました）。イメージに base-plus と onnx が増えたので、**1 回目はイメージを作り直す数分**がかかります。

同じ動画・同じ点で、**1 つずつ**実行してください（前の結果が出てから次へ）:

```powershell
modal run worker/measure/measure_app.py::main --video "C:\Dev\goodbye_background\Goodbye_Background\worker\D_man.mp4" --start 10.7 --end 20.7 --x 0.5 --y 0.42 --image slim --model small --policy sam2 --out measure_out\V
```
```powershell
modal run worker/measure/measure_app.py::main --video "C:\Dev\goodbye_background\Goodbye_Background\worker\D_man.mp4" --start 10.7 --end 20.7 --x 0.5 --y 0.42 --image slim --model small --policy dam4sam --out measure_out\V
```
```powershell
modal run worker/measure/measure_app.py::main --video "C:\Dev\goodbye_background\Goodbye_Background\worker\D_man.mp4" --start 10.7 --end 20.7 --x 0.5 --y 0.42 --image slim --model small --policy samurai --out measure_out\V
```
```powershell
modal run worker/measure/measure_app.py::main --video "C:\Dev\goodbye_background\Goodbye_Background\worker\D_man.mp4" --start 10.7 --end 20.7 --x 0.5 --y 0.42 --image slim --model base-plus --policy sam2 --out measure_out\V
```

- 出力は `measure_out\V\` に、`D_man_<モデル>_<記憶>_report.md`・`_results.json`・`_run1.mp4`・`_run2.mp4` の名前でできます
- 画面の「1 回目: … 秒 / {...}」の `{...}` は、DAM4SAM が見分ける用の記憶に足したフレーム（`drm_added`）と、
  SAMURAI が候補を選び直した回数（`samurai_switched`）です。0 や空なら、その手法はこの動画では働いていません
- もし `crash-looping` が出たら、すぐ Ctrl+C で止めて、最初の 30 行くらいを貼ってください

**貼るもの**: 4 つの `*_report.md`、4 つの `*_run1.mp4`（画質を比べるため）。`*_results.json` は無くても構いません。
