# Goodbye Background フロントエンド (Vanilla JS)

ビルド不要。このフォルダを丸ごと静的配信するだけで動く。`fetch` を使うため、
バックエンド(Axum)と同一オリジンで配信すること。

## ページ構成

- `index.html` … ホーム画面。ここが入口。ファーストビューの大半が写真の選択エリアで、
  写真を選ぶ(またはドロップする)とそのままアプリ画面へ移り、アップロードが始まる。
- `app.html` … アプリ本体（Upload → Infer → Edit → Save の実際の編集画面）。
- `video.html` … 動画の切り抜き（10 秒まで・1日3回まで）。`index.html` のヘッダーの「動画の切り抜き」と
  特徴リストの下のリンクから遷移する。favicon は `video-favicon.png`。流れは後述「動画の切り抜き（video.html）」。
- `privacy.html` … プライバシーポリシー。

## ファイル構成

```
index.html   ホーム画面
app.html     アプリ本体
privacy.html プライバシーポリシー
css/
  tokens.css           サイト全体のカラーパレット（青ベース）と書体。色を変えるときはここだけ触る
  checker-colors.css   市松模様専用の2色
  style.css            アプリ本体(app.html)のレイアウト・コンポーネントのスタイル
  site.css             ホーム画面(index.html)とプライバシーポリシー(privacy.html)のスタイル
  video.css            動画の切り抜きページ(video.html)専用のスタイル（style.css の後に読む）
js/
  app.js       アプリ本体のエントリーポイント。strip.jsとeditor.jsを束ねるだけ
               （strip で選ばれた画像 → onSelect → editor.open、という一方向の配線）
  store.js     状態管理（pub/sub）
  api.js       バックエンドとの通信
  id.js        UI用ローカルIDの生成（リロードで消えてよい、画面内の区別用）
  client-id.js ユーザー識別用 client-id（UUID）の発行と保持（リロードしても不変）
  strip.js     上部の小さい画像ストリップ（アップロード受付・選択切り替え）
  editor.js    単一の大きな編集画面（ポイント編集・undo/redo・保存）
  canvas.js    canvas描画（画像・市松模様・ポイントマーカー）
  toast.js     エラー等の通知カード
  home.js      ホーム画面(index.html)の写真選択エリア（選択・ドロップ → handoff.js に保存 → app.html へ移動）
  handoff.js   ホーム画面で選んだ写真をアプリ画面へ渡すための一時置き場（IndexedDB）
  persist.js   リロードしても写真と編集状態を残す（IndexedDB。後述）
  video/       動画の切り抜きページ(video.html)専用（後述「動画の切り抜き」）
    page.js      エントリーポイント。画面の流れと送信ボタンの有効/無効
    range.js     範囲の丸め込み(clampRange)・開始位置のフレームの切り出し(captureFrame)
    preview.js   開始フレームのプレビュー（/video-jobs/{id}/preview-frame・/preview。本番と同じ SAM2.1 tiny）と toNormalized・applyMaskAlpha
    upload.js    署名付き URL への PUT（XMLHttpRequest で進捗を出す。{ promise, abort } を返す）
    trim.js      選んだ範囲だけの MP4 / MOV を作る（再エンコードしない。moov を書き直し、中身は元の File の slice）・toJobTime
    job.js       ジョブの状態のポーリング(pollJob)
    messages.js  失敗コード・状態・エラーコード → 日本語の文言
    selftest.html 純粋な関数(clampRange / toNormalized / applyMaskAlpha)の自己テスト。ブラウザで開いて確かめる
assets/        画像
  info.svg / warning.svg / danger.svg / success.svg / X.svg … トースト用アイコン
  before.png / after1.png … ホーム画面のビフォーアフター画像
    （拡張子は`.png`前提。透過を見せたいのでPNG推奨。after2/after3 は v1.5 では未使用）
```

## 更新時のキャッシュ対策（`?v=` の版番号）

CSS・JS は `css/site.css?v=1.5.3` のように版番号付きで読み込んでいる（HTML の `<link>` / `<script>` と、
JS どうしの `import` のすべて）。ブラウザが古い CSS/JS を使い続けて、新旧のファイルが混ざった状態で
表示されるのを防ぐため。**CSS や JS を変更して配信し直すときは、全ファイルの `?v=` を同じ新しい番号に
置き換えること**（例: `grep -rl "?v=1.5.3" . | xargs sed -i 's/?v=1.5.3/?v=1.5.3/g'`）。
JS の `import` で番号がそろっていないと、同じファイルが別物として2回読み込まれるので必ず全部そろえる。

## リロードしても写真を消さない（js/persist.js）

選んだ写真と編集の状態を、ブラウザ内の IndexedDB（`gb-sessions`）に保存し、`app.html` を開いたときに復元する。

- **保存するもの**: 元の写真ファイル（最初に1回だけ書く）と、`session_id` / `image_id` / タップした点 / 元に戻した点 /
  状態 / `touchedAt`（最後にサーバーで編集状態を触った時刻）、開いていた写真。`store` の変化を見て、
  変わった写真だけを 300ms まとめてから書く。アップロードに失敗した写真は保存しない。
- **サーバー側の期限（10分）との関係**: サーバーは「最後にアクセスしてから10分」で画像と編集状態を消す
  （`src/config.rs` の `TTL_SEC`）。編集状態と元画像の寿命が延びるのは upload / segment / undo / redo / save なので、
  その時刻を `touchedAt` に記録し、復元時に10分（30秒の余裕を引く）を過ぎていたら `expired` にする。
  `get-preview` は表示用のプレビューの寿命しか延ばさないので `touchedAt` は更新しない。
  **`TTL_SEC` を変えたら `persist.js` の `SERVER_TTL_MS` も合わせること。**
- **期限内の写真**: タップした点・元に戻す履歴も含めてそのまま復元し、続きから編集できる。
- **期限切れの写真**（サムネイルの点が灰色）: 開いたときに、保存しておいた元ファイルで自動的に送り直す
  （`editor.js` の `reupload`）。サーバーの編集状態は消えているので、点と履歴はリセットされ、トーストで知らせる。
  一度に全部送り直すとサーバーに負荷がかかるので、送り直すのは開いた写真だけ。
- **期限内のはずなのに消えていた場合**（サーバーの再起動など）: `get-preview` / `segment` / `undo` / `redo` / `save` が
  404 を返したら期限切れとみなし、同じように送り直す。編集中の操作が失敗した場合は「もう一度タップしてください」と伝える。
- **ブラウザ内の写真の削除**: 最後に編集してから24時間（`KEEP_MS`）を過ぎた写真は、次に開いたときに消す。
  プライバシーポリシーの「5. ブラウザに保存するデータ」にもこの内容を書いている。
- IndexedDB が使えない環境では何も保存しない（今までどおりリロードで消えるだけで、編集は普通にできる）。
- 「残す／消す」のモードは保存しない（リロード後は「残す」から始まる）。

## ダークモード

端末の設定（`prefers-color-scheme: dark`）に合わせて自動で切り替わる。仕組みは `css/tokens.css` と
`css/checker-colors.css` の末尾の `@media` で**色の変数だけ**を差し替えているだけなので、
部品のCSSに色を直接書かないこと（直接書いた色はダークモードで浮く）。

- トーストは明るい状態色の上に濃い文字（`--card-text`）を乗せる作りなので、ダークモードでも同じ見た目のまま。
  閉じるボタン（✕）も `--card-text` を明示している（指定しないとブラウザ既定のボタン文字色になり、
  ダークモードでは白くなって見えなくなる）。
- キャンバスの市松模様と印の色は `js/canvas.js` が変数から読んでキャッシュしている。
  編集中に端末の設定が切り替わったら、キャッシュを捨てて描き直す。
- 各HTMLの `<meta name="color-scheme" content="light dark">` で、スクロールバーなどブラウザの部品も暗くなる。

## 画面構成と基本フロー

`app.html` は1画面構成。画面上部に**画像の追加エリア**、その下に
**1枚だけの大きな編集エリア**（`editor.js`）がある。

追加エリアは状態によって見た目が変わる（どちらも `strip.js` が管理）。

- **画像が1枚も無い間**: 大きいドロップゾーン（`#dropzone`）を表示する。
  タップでファイル選択、ドラッグ＆ドロップにも対応。
- **画像が1枚でもあれば**: 小さい横スクロールのストリップ（`#strip-wrap`）に
  切り替わる。先頭の「＋」タイルから追加を続けられる（複数選択・ドラッグ＆
  ドロップ可）。サムネイルをクリックすると、その画像が下の編集エリアに読み込まれる。
- 写真を追加するたびに、いちばん新しく追加した写真が自動で編集エリアに表示される
  （2回目以降の追加でも。詳しくは「画像の追加エリア」の節）。
- PC: 写真があふれたときだけ、ストリップの両端に矢印ボタンが出て、クリックでスクロールする。
- スマホ（タッチ操作）: 矢印ボタンは非表示（`@media (pointer: coarse)`）。
  ストリップ自体が横スクロールできるので、直接スワイプする。

「複数枚を同時に編集できる」という点について: **画面に表示されるのは常に
1枚だけ**だが、ストリップで別の画像に切り替えても、切り替え前の画像の
処理（セグメンテーション等）が裏で進行中であれば中断されずそのまま完了する。
`session.busy` ロック（後述）が画像ごとに独立しているため、裏側では複数画像の
処理が並行して進んでいても構わない設計になっている。

「Upload → Infer → Edit → Save」のうち、Infer は初回のポイント追加時に自動発火し、
以降ポイントを追加/undo/redoするたびに再実行される。ボタンを分けず1つの流れにしている。

## 状態管理（js/store.js）

セッションは `localId`（クライアント生成のID）をキーにしたMapで持つ。
「いま開いている画像」は `store.activeLocalId` だけが持つ（strip.js が `setActive` で書き込み、
描画にも使う）。editor.js は非同期の取り違え防止のため、別に `currentLocalId` を持つ。

**なぜ `image_id` ではなく `localId` をキーにするか**: アップロード完了
（＝サーバーが `image_id`/`session_id` を発行する）前から、ストリップに
サムネイルを表示したい。サーバーIDが来るまでキーが存在しない状態を避けるため、
ファイル選択の瞬間にクライアント側でIDを払い出す。`sessionId`/`imageId` は
`/upload` のレスポンスが来た時点で埋める。

セッションが持つ主なフィールド:

| フィールド | 役割 |
|---|---|
| `localId` | UI上のキー。ローカル生成、サーバーとは無関係 |
| `sessionId` | 編集セッションのハンドル。`segment`/`undo`/`redo`/`save` が使う |
| `imageId` | 表示すべきスナップショットのポインタ。`get-preview` が使う |
| `points` | マーカー表示用のローカルなポイント履歴（後述の注意点あり） |
| `redoStack` | undoで取り除いた点。redoで戻すための一時退避 |
| `busy` | 変更系リクエストの直列化ロック（後述）。**画像ごとに独立** |
| `thumbUrl` | ストリップのサムネイル・元画像表示用。アップロードした`File`のBlob URL |
| `fullUrl` | 編集エリア用。`get-preview`の結果 |

`js/editor.js` は画面に1つしか無い共有のcanvas・ツールバーを、ストリップで
選ばれた画像に応じて使い回す（`currentLocalId` で「今どの画像を表示しているか」
を保持する）。非同期処理（segment/undo/redo/save）の結果を反映する際は、
必ず「呼び出し時点の `localId`」と、応答が返ってきた時点の `currentLocalId`
が一致するかを確認してからcanvasに描画する。一致しなければ、ユーザーは
既に別画像に切り替えているので描画を破棄する。これをしないと、A画像の
処理結果がB画像に切り替えた後の画面に紛れ込むレースコンディションが起きる。

## API契約（js/api.js）

`image_id` はスナップショットのポインタで、`segment`/`undo`/`redo` のたびに
新しい `image_id` が発行される。実質的にversionの役割を果たす。
`session_id` は編集セッションそのものを指すハンドルで、`segment`/`undo`/`redo`/
`save` はすべてこれで対象を特定する。

| メソッド | パス | リクエスト | レスポンス |
|---|---|---|---|
| POST | `/upload` | `multipart/form-data`, フィールド名 `file` | `{ session_id, image_id }` |
| POST | `/get-preview` | `{ image_id }` | `image/png` バイナリ |
| POST | `/segment` | `{ session_id, x, y, is_foreground }` | `{ image_id }` |
| POST | `/undo` | `{ session_id }` | `{ image_id }` |
| POST | `/redo` | `{ session_id }` | `{ image_id }` |
| POST | `/save` | `{ session_id }` | `{ image_id }`（フル画質版の実体ID） |
| POST | `/get-completed` | `{ image_id }` | バイナリ（保存用フル画質画像） |

**全リクエスト共通のheader `client-id`**: 上の7つすべてのリクエストに、
`client-id: <UUID>` ヘッダーを付ける（`api.js` の `buildHeaders`）。

- `client-id` は**ユーザーを識別するUUID**。1画像(1セッション)ごとではなく、
  **1ユーザー(=1ブラウザ)につき1つ**で、画像を何枚アップロードしても変わらない。
- 初回アクセス時に1度だけ発行して `localStorage`（キー
  `goodbye-background:client-id`）に保存する（`js/client-id.js`）。**リロードしても
  再発行しない**。同じブラウザの別タブでも同じ値になる。
- 値が変わるのは、ブラウザのサイトデータ（localStorage）を消したとき、別のブラウザ・
  端末から使ったとき、プライベートブラウズ等で localStorage が使えないとき
  （この場合はページを開いている間だけ同じ値）。
- 保存値がUUIDの形式でなければ、新しく発行して上書きする（壊れた値をサーバーに送らない）。
- `localId`（`js/id.js`、UI内のセッション区別用）とは別物。混同しないこと。
- `crypto.randomUUID()` が無い環境（LAN内のhttp配信＋iPhone Safari）でも、
  `getRandomValues` からUUID v4を組み立てるので動く。
- `/upload` は `multipart/form-data` なので `Content-Type` は指定せず、ブラウザに
  boundary 付きで決めさせている（`client-id` だけを付ける）。
- フロントを別ホストで配信する場合（`BASE_URL` を設定する場合）は、`client-id` が
  カスタムヘッダーなのでプリフライトが走る。バックエンドのCORS設定で
  `Access-Control-Allow-Headers` に `client-id` を含めること。

補足:

- `/segment` の `x`, `y` は整数（Rust側 `u16` 相当）。canvas上の座標計算は
  浮動小数点になるため、送信前に `Math.round` している。
- `/segment` は**1回の呼び出しにつき1点だけ**送る。累積したpoints配列は
  送らない（累積の管理はサーバー側が `session_id` に紐づけて持っている前提）。
- `/get-completed` は本来は参照系なのでGETが自然だが、`image_id` をJSONボディで
  渡す要件があるためPOSTにしている。**GETリクエストにJSONボディを持たせることは
  fetch APIの仕様上できない**（`TypeError: Request with GET/HEAD method cannot
  have body` で落ちる）ため、ここはPOST一択。
- `undo`/`redo` は `image_id` だけを返し、ポイント一覧は返さない。そのため
  マーカー表示用の `points` 配列はクライアント側で「1回のundo = 直前に
  追加した1点を取り除く」「1回のredo = それを戻す」という前提でベストエフォート
  同期している（`editor.js` の `popLastPoint` / `restoreLastPoint`）。この前提が
  崩れる場合（例: undoが複数点まとめて戻る等）はマーカーの見た目だけがズレ得るが、
  実際に表示される画像は毎回 `get-preview` で取得し直すので常に正しい。

### エラーレスポンスの`code`

バックエンドのエラーレスポンスJSONには一意のエラーコード `code` が入っている。
`api.js` の `throwApiError` がこれを読み取り、`console.error` に
`code=XXX (HTTP status)` の形で出力しつつ、投げる `Error` オブジェクトの
`err.code` にも格納する。呼び出し側（`editor.js`/`strip.js`）はこれを
トースト表示（後述）に渡している。

### 直列化について（`session.busy`）

`image_id` が実質versionの役割を果たすとはいえ、複数の変更系リクエスト
(`segment`/`undo`/`redo`/`save`)を同時に飛ばした場合、「どちらの `image_id` が
最終的に正しいか」はフロント側では判定できない。そのため `editor.js` では
`session.busy` をロックとして使い、変更系リクエストが処理中の間は次の操作を
一切受け付けない（ポイント入力は無視、Undo/Redo/保存ボタンは無効化）ことで、
そもそも同じ画像に対して同時に飛ばないようにしている。

このロックは画像（セッション）ごとに独立している。画面に表示されるのは
常に1枚だが、裏ではA画像の処理が進行中のままB画像に表示を切り替える、
といったことが起こり得る——その場合でもA画像の処理はA画像のセッションの
`busy` だけを見ているので、B画像の操作をブロックしない。

## 動画の切り抜き（video.html / js/video/*）

画面は縦に並んだ 6 つの手順（仕様: `docs/superpowers/specs/2026-11-05-video-cutout-design.md` §3・§7）。

1. **動画を選ぶ** … `<input type="file" accept="video/*">`。Content-Type は 1 回だけ決める:
   `file.type` が `video/mp4` / `video/quicktime` ならそれ、無ければ拡張子（`.mp4` `.m4v` → `video/mp4`、
   `.mov` → `video/quicktime`）、どちらでもなければその場で断る。200MB を超えるものもその場で断る。
2. **範囲を選ぶ** … `<video>` とスライダー 2 本。`clampRange` で「開始 0〜長さ、終了は 開始〜min(開始+10 秒, 長さ)」
   に丸める。「この範囲で決める」で `trimForUpload`（`trim.js`）→ `POST /video-jobs`。**ジョブを作った後は範囲を変えさせない**。
   - `trimForUpload` は「開始以下の最後のキーフレーム 〜 終わり + 1 秒」だけの動画を作る（仕様 `docs/superpowers/specs/2026-11-12-video-upload-trim-design.md`）。
     作れたら、**サーバーに送る時刻（範囲・preview-frame・preview・start の prompts）は全部 `元の時刻 − offsetMs`**（`toJobTime`。
     `frame_set.js` と `page.js` で変換する）。画面の時刻（スライダー・フレームの選択・タブの表示）は元の動画のまま。
     読めない形・ほとんど小さくならないときは元のファイルを丸ごと送り、offsetMs = 0
3. **アップロード** … 返ってきた `upload_url` に `XMLHttpRequest` で PUT（進捗バー。送る大きさも出す）。
   **Content-Type は 1. で決めた文字列をそのまま付ける**（署名の一部なので、違うと R2 が 403 を返す）。
   R2 は別オリジンなので `client-id` は付けない。失敗したら「アップロードをやり直す」（URL は 15 分有効）。
4. **残す所をタップ**（3. と並行）… `captureFrame` で開始位置のフレームを canvas に描き、長辺 2000px 以下に
   縮小して PNG にする（`/preview-frame` は長辺 2000px・10MB まで。10MB を超えたら JPEG にして送る）。iOS Safari で `seeked` がデコードより先に来て真っ黒になるのを避けるため、
   停止 → 移動 → `loadeddata`/`canplay`・`requestVideoFrameCallback` を待つ → 描いた結果が全部黒/透明なら
   ミュートで play()→pause() して 1 回だけやり直す（それでも駄目なら「プレビューをやり直す」）。それを `frame.png` として
   `POST /video-jobs/{id}/preview-frame` で登録する（サーバーが 1 回だけエンコードしてジョブごとに持つ）。
   タップのたびに今の点の一覧を丸ごと `POST /video-jobs/{id}/preview` に送り、返ってきた alpha マスク（グレースケール PNG）を
   フレームに合成して（`applyMaskAlpha`）`canvas.js` で市松模様の上に出す（消える所は薄いフレームが透けて見える）。
   Undo / Redo は手元の点の一覧を戻す・進めて送り直すだけ（画像機能の `/upload`・`/segment`・`/undo`・`/redo` は使わない）。
   - リクエストは同時に 1 つ。応答待ちの間に点が変わったら、その応答は描かずに最新の一覧で送り直す
   - `503 PREVIEW_BUSY`（登録はサーバー全体で同時 2 つまで）は「混み合っています…」を出して 1・2・4 秒待って登録し直す
   - `409 PREVIEW_FRAME_NOT_FOUND`（混雑や期限切れで特徴量が捨てられた）は同じフレームを登録し直して 1 回だけやり直す
   - `X-Object-Present: false` なら「対象が見つかりませんでした」の警告（`#preview-warning`）を出し、送信させない
   - それ以外の失敗はトーストと「プレビューをやり直す」（登録し直して今の一覧を計算し直す）
   プレビューは本番のワーカーと同じモデル（SAM2.1 tiny）の 1 フレーム目なので「目安」とは書かない。
   動きが速い所は途中で欠けることがある旨だけ出す。点は 8 個まで（サーバーの上限）。
5. **背景色を選んで送信** … グリーン（既定）/ ブルー / マゼンタ。送信ボタンが押せるのは
   「アップロード完了」かつ「点が 1 つ以上」かつ「今の点の一覧のプレビューが出ていて `X-Object-Present: true`」かつ「まだ送っていない」とき。
   送るのは undo/redo を反映した今の点の一覧を `toNormalized(フレームx, フレームy, フレーム画像の幅, 高さ)`
   で 0〜1 にしたもの（`label` は「残す」→ `include`、「消す」→ `exclude`）。
6. **結果** … `pollJob` が 1 秒ごとに `GET /video-jobs/{id}`。タブが表示に戻ったら（`visibilitychange`）すぐ 1 回取る。
   `queued` は「準備中（GPU を起動しています）」、`tracking` は「処理中 NN%」。`done` で結果の再生とダウンロード
   （URL は 10 分有効）、`failed` は失敗コードごとの文言と「回数は戻りました / 戻りません」。
   GET が 404 なら「サーバーが再起動したため、やり直してください」を出して止める。
   `/start` の応答が届かなかった（通信の切断・タイムアウト）ときや `VIDEO_JOB_INVALID_STATE` のときは、
   サーバーでは開始できている可能性があるので、成功時と同じくポーリングして本当の状態を見る。
   GET がまだ `created` なら送れていなかったので、送信ボタンを戻す。

残り回数は `POST /video-jobs` と GET の `remaining_today` で更新する（その前は「1日3回まで」とだけ出す。
作る前に残りを返す API は無い）。0 になったら動画を選べなくする。送信後は結果が出るまで別の動画を選ばせない。

### 動画の API（js/api.js）

エラーは他の API と同じく `{ code }`。文言は `js/video/messages.js` が持つ。

| メソッド | パス | リクエスト | レスポンス |
|---|---|---|---|
| POST | `/video-jobs` | `{ size_bytes, content_type, start_ms, end_ms }` | 201 `{ job_id, upload_url, remaining_today }` |
| PUT | `upload_url`（R2 の署名付き URL） | 動画そのもの。`Content-Type` は申告したものと同じ | 200 |
| POST | `/video-jobs/{id}/preview-frame` | `multipart/form-data`, フィールド名 `file`（PNG / JPEG、10MB 以下、長辺 2000px 以下） | 204 |
| POST | `/video-jobs/{id}/preview` | `{ points: [{ x, y, label }] }`（1〜8 個。`/start` と同じ形） | 200 `image/png`（グレースケール 8bit の alpha マスク、255 = 残す。大きさは登録したフレームと同じ）、ヘッダー `X-Object-Present: true \| false` |
| POST | `/video-jobs/{id}/start` | `{ points: [{ x, y, label }], background }` | 202 `{ phase, failure_code, remaining_today }` |
| GET | `/video-jobs/{id}` | なし | 200 `{ phase, progress, failure_code, quota_refunded, download_url, remaining_today }` |

- エラーコード: 400 `INVALID_REQUEST_BODY` / `INVALID_RANGE` / `INVALID_POINTS` / `TOO_LARGE` /
  `UNSUPPORTED_CONTENT_TYPE` / `MISSING_CLIENT_ID`、404 `VIDEO_JOB_NOT_FOUND`、409 `UPLOAD_NOT_FOUND` /
  `VIDEO_JOB_INVALID_STATE`、429 `QUOTA_EXCEEDED`、503 `VIDEO_CUTOUT_DISABLED`（動画の環境変数かプレビューのモデルが無いとき）。
- プレビューのエラーコード: 400 `INVALID_PREVIEW_IMAGE`（フレームが PNG / JPEG でない・大きすぎる）、
  409 `PREVIEW_FRAME_NOT_FOUND`（フレーム未登録か特徴量が捨てられた）/ `VIDEO_JOB_INVALID_STATE`（送信後）、
  500 `PREVIEW_FAILED`、503 `PREVIEW_BUSY`。プレビューの 2 つはジョブの持ち主だけ・送信前だけ使え、利用ログにも記録しない。
- 回数を使うのは `/start` のときだけ（作成・アップロード・プレビューでは減らない）。
- `phase`: `created` / `queued` / `downloading` / `tracking`（`progress` 0〜1）/ `finishing` / `done` / `failed`。

## エディタの見た目（透過領域の可視化、js/canvas.js）

canvas 1枚に以下を下から順に重ねて描画している。

1. **市松模様**（`css/checker-colors.css` の `--checker-light` / `--checker-dark`）。
   `globalAlpha` を2秒周期のサイン波で往復させ、脈動して見えるようにしている
   （`requestAnimationFrame` で常時再描画）。
2. **元画像を半透明**(alpha 0.35)で表示。アップロード時に作った `session.thumbUrl`
   （File由来のBlob URL）をそのまま使い回すので追加の通信は発生しない。
3. **プレビュー画像**（`/get-preview` の結果）。切り抜き済みで不透明な部分は
   1, 2 を覆い隠す。
4. **ポイントのマーカー**。

これにより、まだ選択されていない(=プレビュー画像側で透明な)領域には
「脈動する市松模様」と「薄い元画像」が透けて見え、切り抜けていない場所が
直感的にわかるようにしている。

ポイント入力は `pointerdown` イベントに一本化している（`click` + `touchstart`
を両方使うと、ブラウザによってはタッチ後に合成クリックが発生して二重登録に
なるため）。座標は常に画像本来のピクセル座標系で保持・送信する。

## エラー・通知の表示（トースト、js/toast.js）

`showToast({ type, message, code })` が、画面上部からスライドインするカードを
表示する。

- `type` は `info` / `warning` / `danger` / `success` の4種類。それぞれ
  `tokens.css` の `--info` / `--warning` / `--danger` / `--success` を
  **カード背景全体**の塗りに使う。
- これらの背景は明るめの色なので、上に乗せる文字は `tokens.css` の
  **カード専用のテキスト色 `--card-text` / `--card-text-muted`**（濃い紺）を使っている。
  同じ状態色を白い面の上に「文字として」置くとき（アプリの「残す／消す」ボタンと、キャンバス上のタップ位置の印）は、
  読める濃さにした `--success-ink` / `--danger-ink` を使う。
- アイコンは `assets/info.svg` `assets/warning.svg` `assets/danger.svg`
  `assets/success.svg`（先頭）、閉じるボタンは `assets/X.svg`（末尾）を参照。
  パスは HTML ページ(`app.html`)基準の相対パス（先頭に `/` を付けない。付けるとサブパス
  配信で壊れる）。拡張子は `.svg` 前提。
- アイコンは `<img>` ではなく **CSSの`mask-image`（`background-color: currentColor`
  と組み合わせ）** で描画している。`<img src="....svg">` は外部リソースとして
  扱われ、SVG内部が `fill="currentColor"` に対応していてもページ側の文字色を
  反映できない（`currentColor` はDOM内に埋め込まれた `<svg>` 要素にしか効かない）
  ため。またこの `mask-image` のURLは、CSS変数(`--icon-src`)経由ではなく
  **JS(`applyMaskIcon()`)がインラインスタイルとして直接設定**している。CSS変数に
  `url(...)`を入れて別のスタイルシート側で参照すると、URLの解決基準が
  「変数を定義した場所」ではなく「`var()`を使っている側のスタイルシート」に
  なってしまう（ブラウザの仕様上の挙動）ため、意図しないパスを探しにいくバグの
  原因になる。インラインスタイルとして直接設定すればHTMLページ(`app.html`)を
  基準に解決されるので、この問題を避けられる。
- 4種類とも一定時間で自動的に消える（`AUTO_DISMISS_MS`: `danger`は
  エラーコードを確認する時間を考慮して6000ms、`info`/`success`は4000ms、
  `warning`は6000ms）。いつ消えるかが分かるよう、カード背景の下端に
  カウントダウンの帯（`.toast-progress`、CSSアニメーション
  `toast-progress-shrink` で残り時間に応じて縮んでいく）を表示している。
  アニメーションの長さは表示時と同じ `autoDismissMs` を `animationDuration`
  としてインライン設定しており、実際に消えるタイミングと帯が縮み切る
  タイミングが一致する。

`#edit-status`（`editor.js` の `setStatus` / `renderStatus`）は、画像の上端中央に重ねる帯。
処理中は「切り抜き中…」などの進行状況を、何もしていないときはポイントが0個の間だけ
「残したいところをタップしてください」という次の操作の案内を出す（1点目を置いたら消える）。
空のときは CSS の `:empty` で非表示。失敗時の詳細表示はトースト側に一本化している。

## 編集画面のツールバー（app.html / js/editor.js）

`[残す｜消す]  [元に戻す][やり直す]  ……  [保存]` の1行。スマホでもこの1行に収まるよう、
640px 以下では「元に戻す」「やり直す」の文字を隠してアイコンだけにする（読み上げは `aria-label`）。

- **残す／消す**: 1つのトグルではなく2択のボタン（`#mode-keep` / `#mode-remove`、`aria-pressed`）。
  今どちらなのかが常に見えるようにするため。選択中を示す色の板（`.mode-switch::before`）が、
  切り替えると左右にスライドしながら色が変わる（残す＝`--success-ink`、消す＝`--danger-ink`。
  キャンバス上の印と同じ色）。板の位置は親の `data-mode` で決まり、`editor.js` の `applyMode` が設定する。
  「動きを減らす」設定ではアニメーションしない。API の `is_foreground` は 残す=true / 消す=false。
- **画面の高さ**: `#app` は画面の高さちょうど（`100dvh`）に固定している。`min-height` にすると
  縦長の写真で canvas が画面より高くなり、ページがスクロールしてツールバーが画面外に出てしまう。
- **タップ位置の印**: canvas は縮小表示されるので、印の半径は「画面上で7px以上」になるよう
  表示倍率から計算している（`js/canvas.js` の描画処理）。

## 画像の追加エリア（js/strip.js）について

- アップロードの入口は2つある: 画像が1枚も無い間だけ表示する大きい
  `#dropzone` と、画像が1枚でもあれば表示される「＋ 写真を追加」ボタン（`#strip-add`）。
  追加ボタンは一覧(`#strip`)の外の左端に置いているので、画像が増えてスクロールしても常に見える。
  見た目はホーム画面の入口・`#dropzone` と同じ青い破線で、ドロップも受け付ける。どちらも同じ `handleFiles()` に合流する。
  `store` のセッション数で `dropzoneEl.hidden` / `stripWrapEl.hidden` を
  切り替えており、「最初は大きく、画像が増えたら小さく」という見た目を
  実現している。
- どちらの入口もドラッグ＆ドロップに対応。
- **写真の削除**: 各サムネイルの右上の ✕ で1枚ずつ、帯の右端の「保存済みを削除（N）」で保存が済んだ写真を
  まとめて一覧から消せる（`strip.js` の `deletePhoto` / `removeAndReselect`）。
  - まだ保存していない切り抜き（タップした点がある写真）を消すときだけ確認する。保存済み・未編集の写真はすぐ消える。
  - 開いている写真を消したら、その後ろ（無ければ前）の写真を開く。1枚も無くなったら編集画面を閉じる（`editor.close`）。
  - アップロード中・処理中の写真の ✕ は出さない（終わってから消してもらう）。
  - ブラウザ内の保存（`persist.js`）は store の変化を見て一緒に消える。サーバーに削除APIは無いので、
    サーバー側の画像は期限（10分）で自然に消える。
  - ✕ はPC（マウス）ではカーソルを乗せたとき・キーボードで選んだときだけ表示し、見えない間は押せない。
    タッチ端末では常に表示する。押せる範囲はサムネイル右上の 26px 四方だけ（中央付近まで広げると、
    写真を開こうとしたタップで削除してしまうため）。
  - 640px 以下では「保存済みを削除」はゴミ箱アイコンと枚数だけになる（読み上げは `aria-label`）。
- 写真を追加したら、**いちばん新しく追加した写真を自動で編集画面に開く**（2回目以降の追加でも）。
  まとめて選んだときはその中の最後の1枚を、アップロードが終わった時点で1回だけ開く。
  最後の1枚が失敗したら、同じ回で成功した最後の1枚を開く。開く前に利用者がサムネイルを
  選んだら自動では開かない（`strip.js` の `autoOpenBatch` / `maybeAutoOpen`）。
- 左右の矢印ボタン（PC向け）は、サムネイルが帯の幅に収まらずあふれているときだけ表示する
  （`strip.js` の `updateArrows`。サムネイルの増減と、帯の幅の変化を `ResizeObserver` で見て判定し直す）。
  タッチ端末ではスワイプでスクロールできるので、あふれていても CSS（`pointer: coarse`）で常に隠す。
- サムネイルは `session.status` に応じて右下に小さい色付きドット
  （`.status-dot`）を出す。アップロード中は青、編集中は金、保存中は黄、
  保存済みは緑、エラーは赤。`ready`（アップロード直後でまだ一度も開いていない
  状態）はドット無し（目立たせすぎない）。
- アップロード中・エラーのサムネイルはクリック不可（`disabled`）。
- サムネイルは毎回のstore更新のたびに全部作り直すシンプルな実装（差分更新は
  していない）。枚数が極端に多くなると描画コストが気になる可能性はあるが、
  通常の利用枚数では問題にならない想定。

## モバイル対応で意識したこと

- ポイント入力はPC/スマホで操作方法を変えず、「残す／消す」の2択ボタンに統一
- canvasの入力は `pointerdown` に一本化（`click` + `touchstart` の二重発火を回避）
- canvasに `touch-action: none` を指定し、ポイント入力中に画面がスクロールしないようにしている
- ツールバーのボタンは最低 44px の高さを確保（親指で押しやすいサイズ）
- ストリップの矢印ボタンは `@media (pointer: coarse)` で非表示にし、スマホでは
  素のタッチスクロールに任せる（PCではマウス操作の補助として矢印を出す）

## セキュアコンテキストの制約について

`crypto.randomUUID()` は `https://` または `localhost` でしか使えない
（セキュアコンテキスト限定のAPI）。LAN内に `http://192.168.x.x:3000` の
ような形でホストし、iPhoneのSafariからアクセスすると `crypto.randomUUID`
自体が存在せず、呼び出した瞬間に例外で処理全体が止まる。PCのブラウザでは
気づきにくいが、LAN経由・スマホからのアクセスでは頻発するので注意。

`localId`（UI内でセッションを区別するためだけの値。暗号学的な安全性は不要）
の生成には `js/id.js` の `createLocalId()` を使い、`crypto.randomUUID()`
には依存しないようにしている。

`client-id`（`js/client-id.js`）は、使えるときは `crypto.randomUUID()` を使い、
無ければ `crypto.getRandomValues()`（セキュアコンテキスト以外でも使える）から
UUID v4 を組み立てる。

## ホーム画面からアプリ画面への写真の受け渡し（js/home.js / js/handoff.js）

ホーム画面で写真を選ぶと、アプリ画面でもう一度選ばずにそのままアップロードが始まる。

1. `home.js`: `<label class="start-zone">` の中の `<input type="file">`（エリアのどこを押しても開く）
   またはドロップで受け取った画像を `handoff.js` の `saveFiles` で IndexedDB に保存し、`app.html` へ移動する。
   画像以外しか選ばれなかったときは、移動せずにエリアの下にエラーを出す。
2. `app.js`: 起動時に `takeFiles` で取り出し（同時に削除）、`initStrip` の戻り値の
   `handleFiles` に渡す。以降は、アプリ画面で選んだときと全く同じ流れになる。
3. 取り出し後に削除するので、リロードしても二重にアップロードされない。
   取り出されなかった写真も、保存から5分を過ぎたら次の取り出し時に捨てる。

**受け渡しに失敗した場合**（IndexedDB が使えないプライベートブラウズなど）は
`app.html?handoff=failed` に移動し、アプリ画面でトーストを出して選び直してもらう。
写真はブラウザの中で受け渡すだけで、`/upload` 以外でサーバーに送ることはない。

## ホーム画面の見た目（index.html / css/site.css）

- ファーストビューの大半を写真の選択エリアにした「即スタート」型。説明は見出しと1行、
  使い方は1行の番号付きリストだけにしている。
- 元画像(`before.png`)の右上に素材由来の「Made with AI」表示があるため、`.photo-before img` で
  左下を基点に1.12倍に拡大して右上の角を枠の外へ押し出している（画像ファイル自体は変更しない）。
- 「ここにドラッグ＆ドロップもできます」はマウス操作の端末（`hover: hover` かつ `pointer: fine`）でだけ表示する。

## 検索エンジン対策（SEO / Bing・Edge検索）

公開URL: `https://chickren.goodbye-background.com/`（独自ドメイン。Cloud Run の `*.run.app` は使わない）
（Edgeの検索はBing。Googleでも同じ設定が効く）

- **`index.html`（ホーム画面）を検索の入口にする**: `<title>`・`description`・`canonical`・
  OGP・`robots`・構造化データ（JSON-LD、`WebApplication`）を `<head>` に入れてある。
  説明文は、ページに実際に書いてある機能（タップで指定 / Undo・Redo / 元の画質で保存）だけを
  述べている。「無料」などページに書いていないことは入れないこと。
- **`app.html`（編集画面）は `noindex`**: 中身が動的で検索に出す価値が薄く、紹介ページと
  評価が分散するため。検索に出したくなったら `<meta name="robots">` の行を消す。
- **`robots.txt` / `sitemap.xml`**: `index.html` と同じ場所に置く（サイトのルートから
  配信されること）。`robots.txt` では `app.html` を禁止しない（`noindex` は、ロボットが
  ページを読めて初めて効くため）。
- **ドメインを変えるとき**（独自ドメインにするなど）は、公開URLが書かれた次の5か所を
  すべて書き換える: `index.html` の `canonical` / `og:url` / JSON-LDの `url`、
  `robots.txt` の `Sitemap`、`sitemap.xml` の `<loc>`。
- **この設定だけでは検索に出ない**: Bing側にサイトを知らせる必要がある。
  1. Bing Webmaster Tools でサイトを登録し、所有権を確認する（HTMLのmetaタグか
     XMLファイルを `index.html` と同じ場所に置く方法が簡単）。
  2. 「サイトマップ」に `sitemap.xml` を送信し、「URL送信」でトップページを送信する。
  3. 反映には数日〜数週間かかる。確認は、Bingで `site:chickren.goodbye-background.com`
     を検索する（何も出なければ未登録）。
- 独自ドメインなので `*.run.app` のような共通ドメインの評価の付きにくさは無い。まずは
  「Goodbye Background」の完全一致での検索に出ることを目標にする。

## 既知の未対応

- 認証・エラーの詳細なリトライUIは未実装
- 画像の削除・並び替えUIは未実装
- ストリップにアップロードした画像は、現状いつまでも残り続ける（消す手段が無い）
