"""postprocess.DetachedBlobFilter（体から離れた塊を消す後処理）のテスト。"""

import numpy as np

from cutout.postprocess import REMOVED_LOGIT, DetachedBlobFilter

S = 200  # NEAR_RATIO 0.06 → 12 画素以内なら「近い」


def logits(*boxes) -> np.ndarray:
    """(y0, y1, x0, x1) の箱を正（+5）、残りを負（-5）にしたロジット。"""
    a = np.full((S, S), -5.0, np.float32)
    for y0, y1, x0, x1 in boxes:
        a[y0:y1, x0:x1] = 5.0
    return a


BODY = (50, 150, 80, 120)


def test_first_frame_is_untouched_even_with_far_blobs():
    f = DetachedBlobFilter()
    x = logits(BODY, (10, 20, 10, 20))
    out, removed = f.apply(x)
    assert removed == 0 and out is x  # プレビューと一致させるため 1 枚目は触らない


def test_far_new_blob_is_removed_and_body_kept():
    f = DetachedBlobFilter()
    f.apply(logits(BODY))
    x = logits(BODY, (10, 20, 10, 20))  # 体から遠くに急に出た塊（隣の人の切れ端）
    out, removed = f.apply(x)
    assert removed == 100
    assert (out[10:20, 10:20] <= REMOVED_LOGIT).all()
    assert (out[50:150, 80:120] == 5.0).all()
    assert (x[10:20, 10:20] == 5.0).all()  # 入力は書き換えない


def test_blob_near_previous_mask_is_kept():
    # 前のフレームで体の右にあった手が、今のフレームで体から少し離れて写る（速い手）→ 残す
    f = DetachedBlobFilter()
    f.apply(logits(BODY, (60, 70, 120, 135)))  # 体につながった手
    out, removed = f.apply(logits(BODY, (60, 70, 128, 140)))  # 体から 8 画素離れたが、前の手の近く
    assert removed == 0
    assert (out[60:70, 128:140] == 5.0).all()


def test_largest_blob_is_always_kept_even_if_it_moved_far():
    f = DetachedBlobFilter()
    f.apply(logits(BODY))
    out, removed = f.apply(logits((50, 150, 10, 50), (160, 170, 160, 170)))  # 体が大きく動いた
    assert (out[50:150, 10:50] == 5.0).all()
    assert removed == 100  # 遠くの小さい塊だけ消える


def test_removed_blob_does_not_anchor_the_next_frame():
    f = DetachedBlobFilter()
    f.apply(logits(BODY))
    f.apply(logits(BODY, (10, 20, 10, 20)))  # 消される
    _, removed = f.apply(logits(BODY, (12, 22, 12, 22)))  # 同じ所に出続けても、消したものは手がかりにしない
    assert removed == 100


def test_single_blob_and_empty_mask_pass_through():
    f = DetachedBlobFilter()
    f.apply(logits(BODY))
    x = logits(BODY)
    assert f.apply(x) == (x, 0)
    empty = np.full((S, S), -1024.0, np.float32)  # 被写体なし
    out, removed = f.apply(empty)
    assert removed == 0 and out is empty
