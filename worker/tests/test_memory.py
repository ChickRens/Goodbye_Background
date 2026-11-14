import numpy as np

from cutout.memory import (
    MemoryEntry,
    assemble_memory,
    memory_blocks,
    pointer_list,
    pointer_pos_tokens,
    pointer_tokens,
)


def entry(value=0.0):
    return MemoryEntry(
        tokens=np.full((1024, 1, 64), value, np.float32),
        pos=np.full((1024, 1, 64), value, np.float32),
    )


def e():
    return entry()


def p(v):
    return np.full(256, v, np.float32)


def test_blocks_frame1_pads_with_cond_when_no_recent():
    assert memory_blocks(1, {}) == [("cond", 6)] * 7


def test_blocks_frame3_uses_recent_rows_and_pads_with_newest():
    assert memory_blocks(3, {1: e(), 2: e()}) == [("cond", 6), (2, 0), (1, 1)] + [(2, 0)] * 4


def test_blocks_frame10_uses_six_previous():
    recent = {f: e() for f in range(3, 10)}
    assert memory_blocks(10, recent) == [("cond", 6), (9, 0), (8, 1), (7, 2), (6, 3), (5, 4), (4, 5)]


def test_assemble_memory_adds_row_embedding():
    tpos = np.arange(7 * 64, dtype=np.float32).reshape(7, 64)
    mem, pos = assemble_memory(1, entry(value=1.0), {}, tpos)
    assert mem.shape == (7168, 64) and np.all(mem == 1.0)
    assert np.allclose(pos[0], 1.0 + tpos[6])


def test_pointer_list_normalizes_by_min_total_16_minus_1():
    ptrs, diffs = pointer_list(3, 100, p(0), {1: p(1), 2: p(2)})
    assert ptrs.shape == (16, 256)
    assert np.allclose(diffs[:3], [3 / 15, 1 / 15, 2 / 15]) and np.allclose(diffs[3:], 1 / 15)


def test_pointer_list_short_video_denominator():
    _, diffs = pointer_list(2, 5, p(0), {1: p(1)})
    assert np.allclose(diffs[:2], [2 / 4, 1 / 4])


def test_pointer_tokens_split_and_repeat():
    ptrs = np.arange(16 * 256, dtype=np.float32).reshape(16, 256)
    assert np.array_equal(pointer_tokens(ptrs)[1], ptrs[0, 64:128])
    pp = np.arange(16 * 64, dtype=np.float32).reshape(16, 64)
    assert np.array_equal(pointer_pos_tokens(pp)[0:4], np.repeat(pp[:1], 4, axis=0))
