# 動画: 点を打ったフレームで「前回のマスク」を使って 1 点ずつ積み上げる（設計）

日付: 2026-11-16 / 状態: オーナー承認済み（案 A・置き場所は Modal Volume）

## 1. 問題

動画の手順 4 で、スタンド・袈裟・ケーブルの 3 点を打つと、マイクだけが残り人物が消えた（画像の切り抜きでは同じ 3 点で人物とマイクが残る）。
本番と同じモデル・同じ計算で再現した結果（フレーム 0、720x1280）:

| 渡し方 | 残った画素 |
|---|---|
| 3 点をまとめて（今の動画） | 26,563（マイク＋ケーブル）→ いちばん大きい塊だけ残して 19,703（マイク） |
| 袈裟の 1 点だけ | 617,218（人物） |

原因: 画像の切り抜きは点を足すたびに前回のマスクをデコーダに渡して積み上げるが、動画（diffusionstudio の
`mask_decoder.onnx`）にはマスクを入れる口が無く、毎回全部の点をまとめてマスク無しで計算している。
SAM2 は離れた複数の点を 1 つの物として解釈しようとし、細いマイクとケーブルを選んだ。
公式の SAM2 の動画 predictor（`add_new_points_or_box`）は、点を足すとき前回の low-res マスク（-32〜32 に丸めたもの）を渡す。

`keep_largest_region` は主因ではない（ケーブルの 6,687 画素を消しただけ）。今回は変えない。

## 2. 方針

マスクデコーダだけを `transformers` の `Sam2VideoModel`（`facebook/sam2.1-hiera-small`）から自分で書き出し直し、
入力 `mask_input` / `has_mask` と、名前の付いた出力 `candidate_masks` / `candidate_ious` を足す。
ほかの 4 グラフ（vision_encoder・memory_encoder・memory_attention・pointer_tpos）は diffusionstudio のまま。

### 2.1 新しいマスクデコーダ（`mask_decoder_with_mask_v1.onnx`、fp32、約 22MB）

- 入力: diffusionstudio と同じ `feats0` `feats1` `feats2_cond` `input_points [1,1,N,2]` `input_labels [1,1,N] int32` に、
  `mask_input [1,1,256,256] float32`（前回の `low_res_mask` を -32〜32 に丸めたもの）、`has_mask [1] float32`（1 で使う、0 で使わない）
- 出力: diffusionstudio と同じ `low_res_mask` `high_res_mask` `iou` `object_score_logits` `object_pointer` に、
  `candidate_masks [1,4,256,256]`（object score で打ち消す前）、`candidate_ious [1,4]`
- 選び方は transformers と同じ: 本物の点（label != -1）が 1 つ以下なら候補 1〜3 の IoU 予測最大、2 つ以上なら候補 0
  （安定度 < 0.98 なら候補 1〜3 の最大）。`object_score_logits <= 0` ならマスク -1024・no_object_pointer
- 検証済み（2026-11-16）: `has_mask=0` で diffusionstudio 版と比べ、マスク IoU 0.9986〜1.0、object score・IoU 予測は小数 2 桁で一致、
  object pointer の cos 類似度 1.0。追跡のダミー点（label -1）は IoU 1.0。差は diffusionstudio が fp16 であることによる
- SHA-256: `ba8862fdba8cb0027f7314bfb649fe18c16852a792ad6ca0dc76818aacac6ba6`（2026-11-16 に書き出したもの。これを本番に使う）

### 2.2 1 点ずつ積み上げる（プレビューとワーカーで同じ計算）

点を打ったフレーム（1 枚目と、複数フレームの指定フレーム）では、点の列 `p1..pn`（打った順。フロントは既にこの順で送っている）に対して:

```
prev = なし
for i in 1..n:
    out = decoder(points = p1..pi, mask_input = clip(prev, -32, 32), has_mask = prev があれば 1)
    prev = out.low_res_mask
結果 = 最後の out（high_res_mask・object_score_logits・object_pointer・候補）
```

- API・フロント・サーバーに持たせる状態は変えない。点の列が変われば最初から積み上げ直すので、元に戻す・やり直すもそのまま正しい
- 追跡中のフレーム（点なし）は `has_mask=0` でダミー点 1 つ。今と同じ結果
- 検証済み: スタンド → 袈裟 → ケーブルで 15,423 → 641,046 → 649,358 画素（マイク＋人物＋ケーブル）

### 2.3 置き場所と版の固定（Modal Volume）

- Modal Volume `goodbye-background-models` の `sam2/mask_decoder_with_mask_v1.onnx`。オーナーが 1 回だけ `modal volume put` する
- ワーカー: Volume を `/vol` にマウントして読む。ほかの 4 グラフは今までどおりイメージのビルド時に Hugging Face から取得
- プレビュー: `worker/scripts/fetch_preview_models.py` が `modal volume get` で `models/sam2.1-small-video/onnx/` に取得する
- 両方とも読み込む前に SHA-256 を照合し、違えば起動に失敗する（プレビューとワーカーで別のファイルを使う事故を防ぐ。CLAUDE.md 前提 16）
- 書き出しのスクリプトは `worker/scripts/export_mask_decoder_with_mask.py`（torch 2.15.0・transformers 5.17.0。ワーカーの依存には入れない）

## 3. やらないこと

- `keep_largest_region` の変更（別の話。今回の主因ではない）
- fp16 への変換（デコーダは軽いので fp32 のまま。速度が問題になったら別途）
- diffusionstudio の他の 4 グラフの書き出し直し
