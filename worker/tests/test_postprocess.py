"""postprocess.keep_largest_region（見える範囲のうち、いちばん大きい塊だけを残す後処理）のテスト。"""

import numpy as np

from cutout.postprocess import keep_largest_region


def alpha(*boxes, value=1.0) -> np.ndarray:
    a = np.zeros((200, 200), np.float32)
    for y0, y1, x0, x1 in boxes:
        a[y0:y1, x0:x1] = value
    return a


BODY = (50, 150, 80, 120)


def test_only_the_largest_region_is_kept():
    a = alpha(BODY, (10, 20, 10, 20), (60, 70, 125, 135))  # 遠くの塊と、体のすぐ近く（5 画素）の塊
    out = keep_largest_region(a)
    assert out[10:20, 10:20].max() == 0.0
    assert out[60:70, 125:135].max() == 0.0  # 近くても、つながっていなければ消す
    assert (out[50:150, 80:120] == 1.0).all()
    assert a[10:20, 10:20].min() == 1.0  # 入力は書き換えない


def test_faint_pixels_count_as_visible():
    # 半透明のかすかな塊（ロジットは 0 未満でも見えてしまう所）も消す
    a = alpha(BODY)
    a[10:20, 10:20] = 0.05
    out = keep_largest_region(a)
    assert out[10:20, 10:20].max() == 0.0


def test_faint_bridge_connects_to_body():
    # かすかな画素でつながっていれば同じ塊（体の縁の半透明は消さない）
    a = alpha(BODY, (60, 70, 125, 135))
    a[64, 120:125] = 0.05
    out = keep_largest_region(a)
    assert out[60:70, 125:135].min() == 1.0


def test_diagonal_touch_counts_as_connected():
    a = alpha(BODY, (150, 160, 120, 130))  # 体の右下の角と斜めに接する（8 近傍でつながる）
    out = keep_largest_region(a)
    assert out is a


def test_single_region_and_empty_pass_through():
    a = alpha(BODY)
    assert keep_largest_region(a) is a
    empty = np.zeros((20, 20), np.float32)
    assert keep_largest_region(empty) is empty
