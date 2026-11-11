"""postprocess.keep_largest_blob（いちばん大きい塊だけを残す後処理）のテスト。"""

import numpy as np

from cutout.postprocess import REMOVED_LOGIT, keep_largest_blob

S = 200


def logits(*boxes) -> np.ndarray:
    """(y0, y1, x0, x1) の箱を正（+5）、残りを負（-5）にしたロジット。"""
    a = np.full((S, S), -5.0, np.float32)
    for y0, y1, x0, x1 in boxes:
        a[y0:y1, x0:x1] = 5.0
    return a


BODY = (50, 150, 80, 120)


def test_only_the_largest_blob_is_kept():
    x = logits(BODY, (10, 20, 10, 20), (60, 70, 125, 135))  # 遠くの塊と、体のすぐ近く（5 画素）の塊
    out, removed = keep_largest_blob(x)
    assert removed == 200
    assert (out[10:20, 10:20] <= REMOVED_LOGIT).all()
    assert (out[60:70, 125:135] <= REMOVED_LOGIT).all()  # 近くても、つながっていなければ消す
    assert (out[50:150, 80:120] == 5.0).all()
    assert (x[10:20, 10:20] == 5.0).all()  # 入力は書き換えない


def test_diagonal_touch_counts_as_connected():
    x = logits(BODY, (150, 160, 120, 130))  # 体の右下の角と斜めに接する（8 近傍でつながる）
    out, removed = keep_largest_blob(x)
    assert removed == 0 and out is x


def test_single_blob_and_empty_mask_pass_through():
    x = logits(BODY)
    assert keep_largest_blob(x) == (x, 0)
    empty = np.full((S, S), -1024.0, np.float32)  # 被写体なし
    out, removed = keep_largest_blob(empty)
    assert removed == 0 and out is empty
