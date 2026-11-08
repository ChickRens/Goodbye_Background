"""案 C の比較用: SAM2 の「どのフレームを記憶に使うか」を変えた追跡（2026-11-08）。

本番の追跡は tracker.py（Sam2VideoTracker）。こちらは計測と診断でだけ使う。モデル（ONNX の 5 グラフ）は同じで、
記憶の組み立てと候補マスクの選び方だけを、次の論文の実装（どちらも Apache-2.0）に合わせて変える:

- sam2: 本番と同じ（memory.memory_blocks / pointer_list）。この実装の正しさの確認用
- dam4sam: DAM4SAM（Videnovic ほか、CVPR 2025。github.com/jovanavidenovic/DAM4SAM の dam4sam_tracker.py と
  sam2_base.py）。記憶を「最近の見た目（RAM）」と「紛らわしいものを見分ける用（DRM）」に分ける。
  候補マスクのうち選ばれなかったものが、選ばれたマスクの外に大きくはみ出すフレーム（= 似た物体がいる）を DRM に足す
- samurai: SAMURAI（Yang ほか、2024。github.com/yangchris11/samurai の sam2_base.py）。被写体の箱の動きを
  カルマンフィルタで予測し、候補マスクの選択と、記憶に使うフレームの選別（確からしさ・被写体らしさ・動きの一致）に使う

必要な値: マスクデコーダが内部で計算している候補マスク 4 つ（0 番 = 単一出力用、1〜3 番 = 複数出力用）と
その IoU の予測。グラフの途中のテンソル（CANDIDATE_OUTPUTS）を出力に足して取り出す（models.with_extra_outputs）。
点の無い追跡中のフレームでは、デコーダの出力は 1〜3 番のうち IoU の予測が最大のもの（= 本番と同じ）。

論文の実装との違い（ONNX の制約。結果の比較では、この違いを差し引いて見る）:
- 記憶の入力は 7 フレーム分・物体ポインタ 16 個で固定。足りない分は本番と同じく最新のものを複製して埋める
- SAMURAI が IoU 最大以外の候補を選んだとき、物体ポインタはデコーダが出した「IoU 最大の候補」のもののまま
- SAMURAI の「被写体がいる」の閾値（min_obj_score_logits=-1）は、候補の選択と出力にだけ使う。
  デコーダ内部の物体ポインタの扱い（被写体なしの埋め込みの混ぜ方）は閾値 0 のまま
- 箱は候補マスクの解像度（tiny 128、small 以上 256）で求める（IoU とカルマンフィルタは拡大縮小で変わらない）
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

import cv2
import numpy as np

from cutout.contract import NormalizedPoint
from cutout.memory import COND_ROW, MAX_PTRS, MEM_FRAMES, MemoryEntry, pointer_pos_tokens, pointer_tokens
from cutout.tracker import DEFAULT_IMAGE_SIZE, FrameResult, Sam2Sessions, chw_to_tokens, to_pixel_values

Policy = Literal["sam2", "dam4sam", "samurai"]
POLICIES: tuple[Policy, ...] = ("sam2", "dam4sam", "samurai")

# マスクデコーダの途中のテンソル: 候補マスク [1,4,h,w]（fp16）と IoU の予測 [1,4]（fp16）。
# diffusionstudio の tiny / small / base-plus / large で同じ名前（2026-11-08 に確認）
CANDIDATE_OUTPUTS = ["/Gather_8_output_0", "/Gather_6_output_0"]
NO_OBJ_LOGIT = -1024.0  # SAM2 が「被写体なし」のマスクに入れる値

# 何フレーム前まで記憶の候補として持っておくか（DAM4SAM の 5 フレームおきの探索・SAMURAI の 15 個の選別に足りる長さ）。
# DRM のフレームとフレーム 0 はこれより古くても持つ
HISTORY_FRAMES = 64

# DAM4SAM（dam4sam_config / sam21pp_hiera_*.yaml と dam4sam_tracker.py の値）
DAM_MAX_COND = 4  # max_cond_frames_in_attn（DRM から記憶に入れる最大数。フレーム 0 を含む）
DAM_STRIDE = 5  # memory_temporal_stride_for_eval（RAM は 5 フレームおき）
DAM_MIN_IOU = 0.8
DAM_SIZE_RATIO = (0.8, 1.2)
DAM_MIN_GAP = 5  # 前回 DRM に足してから何フレーム空けるか
DAM_MAX_BOX_IOU = 0.7

# SAMURAI（configs/samurai/sam2.1_hiera_s.yaml の値。t・b+ も同じで、l だけ kf_score_weight=0.15）
SAM_STABLE_FRAMES = 15
SAM_STABLE_IOU = 0.3
SAM_KF_WEIGHT = 0.25
SAM_MEM_IOU = 0.5
SAM_MEM_OBJ = 0.0
SAM_MEM_KF = 0.0
SAM_MIN_OBJ = -1.0


@dataclass
class _Frame:
    entry: MemoryEntry
    ptr: np.ndarray  # [256]
    n_pixels: int  # 拡大したマスクの正の画素数（被写体が見えているか）
    best_iou: float  # 選んだ候補の IoU の予測
    obj_score: float
    kf_score: float | None  # SAMURAI: 選んだ候補の箱とカルマンフィルタの予測の IoU（安定するまでは None）


class BoxKalman:
    """箱 (中心 x, 中心 y, 縦横比, 高さ) と、その速度の 8 次元の定速度カルマンフィルタ。

    SAMURAI が使う deep_sort 型のフィルタと同じ式・同じ雑音の重み（位置 1/20・速度 1/160、高さに比例）で書いたもの。
    """

    _W_POS = 1.0 / 20
    _W_VEL = 1.0 / 160

    def __init__(self) -> None:
        self._F = np.eye(8)
        self._F[:4, 4:] = np.eye(4)
        self._H = np.eye(4, 8)

    def initiate(self, z: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
        h = z[3]
        std = [2 * self._W_POS * h, 2 * self._W_POS * h, 1e-2, 2 * self._W_POS * h,
               10 * self._W_VEL * h, 10 * self._W_VEL * h, 1e-5, 10 * self._W_VEL * h]
        return np.r_[z, np.zeros(4)], np.diag(np.square(std))

    def predict(self, x: np.ndarray, P: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
        h = x[3]
        std = [self._W_POS * h, self._W_POS * h, 1e-2, self._W_POS * h,
               self._W_VEL * h, self._W_VEL * h, 1e-5, self._W_VEL * h]
        return self._F @ x, self._F @ P @ self._F.T + np.diag(np.square(std))

    def update(self, x: np.ndarray, P: np.ndarray, z: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
        h = x[3]
        R = np.diag(np.square([self._W_POS * h, self._W_POS * h, 1e-1, self._W_POS * h]))
        S = self._H @ P @ self._H.T + R
        K = np.linalg.solve(S, self._H @ P).T  # P H^T S^-1（S は対称）
        return x + K @ (z - self._H @ x), P - K @ S @ K.T


def mask_box(mask: np.ndarray) -> list[float]:
    """正の画素を囲む箱 [x1, y1, x2, y2]（画素の添字。無ければ [0,0,0,0]）。"""
    ys, xs = np.nonzero(mask > 0)
    if len(xs) == 0:
        return [0.0, 0.0, 0.0, 0.0]
    return [float(xs.min()), float(ys.min()), float(xs.max()), float(ys.max())]


def xyxy_to_xyah(b: list[float]) -> np.ndarray:
    w, h = b[2] - b[0], b[3] - b[1]
    h = h if h != 0 else 1.0
    return np.array([(b[0] + b[2]) / 2, (b[1] + b[3]) / 2, w / h, h])


def xyah_to_xyxy(x: np.ndarray) -> list[float]:
    xc, yc, a, h = x[:4]
    return [xc - a * h / 2, yc - h / 2, xc + a * h / 2, yc + h / 2]


def box_iou(a: list[float], b: list[float]) -> float:
    ix = max(0.0, min(a[2], b[2]) - max(a[0], b[0]))
    iy = max(0.0, min(a[3], b[3]) - max(a[1], b[1]))
    inter = ix * iy
    union = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - inter
    return inter / union if union > 0 else 0.0


def largest_component(mask: np.ndarray) -> np.ndarray:
    n, labels, stats, _ = cv2.connectedComponentsWithStats(mask.astype(np.uint8), connectivity=8)
    if n <= 1:
        return mask.astype(np.uint8)
    k = 1 + int(np.argmax(stats[1:, cv2.CC_STAT_AREA]))
    return (labels == k).astype(np.uint8)


def dam_has_distractor(chosen: np.ndarray, alternatives: list[np.ndarray]) -> bool:
    """DAM4SAM の 2 つ目の条件: 選ばれなかった候補のうち、選んだマスクの外にはみ出す部分（最大の塊）を
    足した箱が、選んだマスクの箱と IoU 0.7 以下になるものがあるか（= 近くに似た物体がいる）。"""
    chosen = chosen.astype(np.uint8)
    chosen_box = mask_box(chosen)
    outside = [np.logical_and(m, np.logical_not(chosen)).astype(np.uint8) for m in alternatives]
    outside = [largest_component(m) for m in outside if m.sum() >= 1]
    if not outside:
        return False
    ious = [box_iou(chosen_box, mask_box(np.logical_or(m, chosen))) for m in outside]
    return min(ious) <= DAM_MAX_BOX_IOU


def select_closest_cond(t: int, cond: list[int], max_n: int) -> list[int]:
    """DAM4SAM の select_closest_cond_frames: フレーム 0・t-1 より前で最も近いもの・残りを近い順（t-1 は除く）。"""
    selected: list[int] = []
    before = max((f for f in cond if f < t - 1), default=None)
    if before is not None and before >= 0:
        selected.append(before)
    if 0 not in selected:
        selected.append(0)
    rest = sorted((f for f in cond if f not in selected and f != t - 1), key=lambda f: abs(f - t))
    return selected + rest[: max(0, max_n - len(selected))]


class VariantTracker:
    """Sam2VideoTracker と同じ使い方（process を 1 フレームずつ）。sessions のマスクデコーダは
    CANDIDATE_OUTPUTS を出力に足したもの（load_sessions(..., decoder_extra_outputs=CANDIDATE_OUTPUTS)）。"""

    def __init__(self, sessions: Sam2Sessions, constants: dict, total_frames: int, policy: Policy):
        if policy not in POLICIES:
            raise ValueError(f"policy は {POLICIES} のどれか: {policy!r}")
        self._s = sessions
        self._policy = policy
        self._mean = constants["image_mean"]
        self._std = constants["image_std"]
        self._size = int(constants.get("image_size", DEFAULT_IMAGE_SIZE))
        self._tpos = np.asarray(constants["memory_temporal_positional_encoding"], np.float32).reshape(MEM_FRAMES, 64)
        self._total = total_frames
        self._t = 0
        self._hist: dict[int, _Frame] = {}
        self._drm: list[int] = []  # DAM4SAM: DRM に足したフレーム（フレーム 0 は常に条件メモリなので含めない）
        self._dam_last_added = -1
        self._dam_sizes: list[int] = []
        self._kf = BoxKalman()
        self._kf_state: tuple[np.ndarray, np.ndarray] | None = None
        self._stable = 0
        self.stats = {"drm_added": [], "samurai_switched": 0}  # 結果の確認用（DRM に足したフレーム・SAMURAI が候補を変えた回数）

    # ---- 記憶の組み立て（(フレーム, 時間埋め込みの行) を MEM_FRAMES 個）----

    def _pad(self, blocks: list[tuple[int, int]]) -> list[tuple[int, int]]:
        """本番の memory_blocks と同じく、足りない分を最新のブロック（無ければ条件メモリ）の複製で埋める。"""
        newest = blocks[1] if len(blocks) > 1 else blocks[0]
        return blocks + [newest] * (MEM_FRAMES - len(blocks))

    def _blocks_sam2(self, t: int) -> list[tuple[int, int]]:
        blocks = [(0, COND_ROW)]
        for k in range(1, MEM_FRAMES):
            f = t - k
            if f < 0:
                break
            if f != 0 and f in self._hist:
                blocks.append((f, k - 1))
        return self._pad(blocks)

    def _blocks_dam4sam(self, t: int) -> tuple[list[tuple[int, int]], list[int], set[int]]:
        cond = [0] + self._drm
        selected = select_closest_cond(t, cond, DAM_MAX_COND)
        cond_set = set(cond)

        def noncond(f: int) -> _Frame | None:
            return self._hist.get(f) if f not in cond_set else None

        def ok(fr: _Frame | None, f: int) -> bool:
            return fr is not None and fr.n_pixels >= 1 and f not in selected

        keyed: list[tuple[int, int | None]] = [(0, f) for f in selected]  # (並べ替えのキー, フレーム)。DRM はキー 0
        prev = -1
        for i in range(MEM_FRAMES - len(selected)):
            if i == 0:
                prev = t - 1
                fr = self._hist.get(prev) if prev in cond_set else noncond(prev)
                if not ok(fr, prev):
                    fr = None
                    while True:
                        prev -= 1
                        if prev <= 0:
                            break
                        cand = noncond(prev)
                        if ok(cand, prev):
                            fr = cand
                            break
            elif prev >= 0:
                prev = ((prev - 1) // DAM_STRIDE) * DAM_STRIDE
                fr = noncond(prev)
                if not ok(fr, prev):
                    fr = None
                    while True:
                        prev -= DAM_STRIDE
                        if prev <= 0:
                            break
                        cand = noncond(prev)
                        if ok(cand, prev):
                            fr = cand
                            break
            else:
                fr = None
            keyed.append((prev, prev if fr is not None else None))
        keyed.sort(key=lambda x: x[0])  # DRM（キー 0）が先、RAM は古い順（Python の sort は安定）
        total = len(keyed)
        blocks = []
        for pos, (key, f) in enumerate(keyed):
            if f is None:
                continue  # 論文の実装の「padding frames」（使わない）
            t_pos = 0 if key == 0 else pos
            blocks.append((f, total - t_pos - 1))
        # 条件メモリ（行 6）を先頭に、RAM の最新を「最新のブロック」として複製に使う
        blocks.sort(key=lambda b: (b[1] != COND_ROW, -b[0]))
        unselected = cond_set - set(selected)
        return self._pad(blocks), selected, unselected

    def _blocks_samurai(self, t: int) -> list[tuple[int, int]]:
        valid: list[int] = []
        if t > 1:
            for i in range(t - 1, 1, -1):
                fr = self._hist.get(i)
                if fr is None:
                    break  # HISTORY_FRAMES より古い
                if fr.best_iou > SAM_MEM_IOU and fr.obj_score > SAM_MEM_OBJ and (
                    fr.kf_score is None or fr.kf_score > SAM_MEM_KF
                ):
                    valid.insert(0, i)
                if len(valid) >= MAX_PTRS - 1:
                    break
        if t - 1 not in valid:
            valid.append(t - 1)
        blocks = [(0, COND_ROW)]
        for t_pos in range(1, MEM_FRAMES):
            idx = t_pos - MEM_FRAMES
            if idx < -len(valid):
                continue
            f = valid[idx]
            if f == 0 or f not in self._hist:
                continue  # フレーム 0 は条件メモリ（非条件メモリには無い）
            blocks.append((f, MEM_FRAMES - t_pos - 1))
        blocks.sort(key=lambda b: (b[1] != COND_ROW, -b[0]))
        return self._pad(blocks)

    # ---- 物体ポインタ（16 個とフレーム差）----

    def _pointers(self, t: int, dam: tuple[list[int], set[int]] | None) -> tuple[np.ndarray, np.ndarray]:
        max_ptrs = min(self._total, MAX_PTRS)
        denom = max(max_ptrs - 1, 1)
        if dam is None:  # sam2 / samurai（SAMURAI はポインタの選び方を変えない）
            lst = [(self._hist[0].ptr, t)]
            for d in range(1, max_ptrs):
                f = t - d
                if f < 0:
                    break
                if f != 0 and f in self._hist:
                    lst.append((self._hist[f].ptr, d))
            newest = lst[1] if len(lst) > 1 else lst[0]
        else:
            selected, unselected = dam
            lst = [(self._hist[f].ptr, abs(t - f)) for f in selected if f <= t]
            n_cond = len(lst)
            cond_set = set(selected) | unselected
            for d in range(1, max_ptrs):
                f = t - d
                if f < 0:
                    break
                if f in selected:
                    continue
                fr = self._hist.get(f)
                if fr is not None and (f not in cond_set or f in unselected) and fr.n_pixels >= 1:
                    lst.append((fr.ptr, d))
            lst = lst[:MAX_PTRS]
            newest = lst[n_cond] if len(lst) > n_cond else lst[0]
        lst += [newest] * (MAX_PTRS - len(lst))
        ptrs = np.stack([p for p, _ in lst])
        diffs = np.array([d / denom for _, d in lst], np.float32)
        return ptrs, diffs

    # ---- 候補の選択 ----

    def _choose(self, cand: np.ndarray, ious: np.ndarray) -> tuple[int, float | None]:
        """1〜3 番の候補（cand [3,h,w]・ious [3]）から 1 つ選ぶ。戻り値は (添字, SAMURAI の動きの一致度)。"""
        best = int(np.argmax(ious))
        if self._policy != "samurai":
            return best, None
        kf_score = None
        if self._kf_state is None or self._stable == 0:
            self._kf_state = self._kf.initiate(xyxy_to_xyah(mask_box(cand[best])))
            self._stable += 1
            return best, None
        x, P = self._kf.predict(*self._kf_state)
        if self._stable < SAM_STABLE_FRAMES:
            idx = best
            if ious[idx] > SAM_STABLE_IOU:
                x, P = self._kf.update(x, P, xyxy_to_xyah(mask_box(cand[idx])))
                self._stable += 1
            else:
                self._stable = 0
        else:
            boxes = [mask_box(c) for c in cand]
            kf_ious = np.array([box_iou(xyah_to_xyxy(x), b) for b in boxes])
            weighted = SAM_KF_WEIGHT * kf_ious + (1 - SAM_KF_WEIGHT) * ious
            idx = int(np.argmax(weighted))
            if idx != best:
                self.stats["samurai_switched"] += 1
            if ious[idx] < SAM_STABLE_IOU:
                self._stable = 0
            else:
                x, P = self._kf.update(x, P, xyxy_to_xyah(boxes[idx]))
            kf_score = float(kf_ious[idx])
        self._kf_state = (x, P)
        return idx, kf_score

    # ---- 1 フレーム ----

    def process(self, frame_rgb: np.ndarray, points: list[NormalizedPoint] | None) -> FrameResult:
        t = self._t
        first = t == 0
        if first and not points:
            raise ValueError("1 枚目にはクリック点が必要")
        if not first and points is not None:
            raise ValueError("2 枚目以降は points を渡さない")
        S = self._size
        enc = self._s.vision_encoder.run(pixel_values=to_pixel_values(frame_rgb, self._mean, self._std, S))
        dam = None
        if first:
            feats_cond = enc["feats2_no_mem"]
            coords = np.array([[[[p.x * S, p.y * S] for p in points]]], np.float32)
            labels = np.array([[[1 if p.label == "include" else 0 for p in points]]], np.int32)
        else:
            if self._policy == "dam4sam":
                blocks, selected, unselected = self._blocks_dam4sam(t)
                dam = (selected, unselected)
            elif self._policy == "samurai":
                blocks = self._blocks_samurai(t)
            else:
                blocks = self._blocks_sam2(t)
            entries = [(self._hist[f].entry, row) for f, row in blocks]
            mem = np.concatenate([e.tokens.reshape(-1, 64) for e, _ in entries])
            pos = np.concatenate([e.pos.reshape(-1, 64) + self._tpos[row][None] for e, row in entries])
            ptrs, diffs = self._pointers(t, dam)
            ppos = self._s.pointer_tpos.run(normalized_diffs=diffs)["pointer_pos"]
            mem = np.concatenate([mem, pointer_tokens(ptrs)])
            pos = np.concatenate([pos, pointer_pos_tokens(ppos)])
            feats_cond = self._s.memory_attention.run(
                current_vision_features=chw_to_tokens(enc["feats2"]),
                current_vision_position_embeddings=chw_to_tokens(enc["vision_pos_embed"]),
                memory=mem[:, None, :],
                memory_pos=pos[:, None, :],
            )["conditioned_feats"]
            coords = np.zeros((1, 1, 1, 2), np.float32)
            labels = np.full((1, 1, 1), -1, np.int32)
        dec = self._s.mask_decoder.run(
            feats0=enc["feats0"], feats1=enc["feats1"], feats2_cond=feats_cond,
            input_points=coords, input_labels=labels,
        )
        score = float(np.asarray(dec["object_score_logits"]).reshape(-1)[0])
        cand_all = np.asarray(dec[CANDIDATE_OUTPUTS[0]], np.float32).reshape(4, *dec[CANDIDATE_OUTPUTS[0]].shape[-2:])
        ious_all = np.asarray(dec[CANDIDATE_OUTPUTS[1]], np.float32).reshape(4)
        cand, ious = cand_all[1:], ious_all[1:]  # 追跡中と 1 点の 1 枚目は複数出力（1〜3 番）から選ぶ
        if first and self._is_single_output(dec, cand_all):
            # 除外以外の点が 2 つ以上だと、デコーダは単一出力（0 番）を使う。規則を推測せず、実際の出力と照らして決める
            cand, ious = cand_all[:1], ious_all[:1]
        idx, kf_score = self._choose(cand, ious)
        threshold = SAM_MIN_OBJ if self._policy == "samurai" else 0.0
        present = score > threshold
        if present:
            high = cv2.resize(cand[idx], (S, S), interpolation=cv2.INTER_LINEAR)
        else:
            high = np.full((S, S), NO_OBJ_LOGIT, np.float32)
        me = self._s.memory_encoder.run(
            feats2=enc["feats2"], high_res_mask=high[None, None],
            object_score_logits=[[score]], binarize=1 if first else 0,
        )
        n_pixels = int((high > 0).sum())
        self._hist[t] = _Frame(
            entry=MemoryEntry(tokens=me["memory_tokens"], pos=me["memory_pos"]),
            ptr=np.asarray(dec["object_pointer"]).reshape(-1)[:256],
            n_pixels=n_pixels, best_iou=float(ious[idx]), obj_score=score, kf_score=kf_score,
        )
        if self._policy == "dam4sam" and not first:
            self._dam_update(t, cand, ious, idx, n_pixels)
        old = t - HISTORY_FRAMES
        if old > 0 and old not in self._drm:
            self._hist.pop(old, None)
        self._t += 1
        return FrameResult(high_res_logits=high, object_present=present)

    def _is_single_output(self, dec: dict, cand_all: np.ndarray) -> bool:
        """1 枚目でデコーダの出力が 0 番の候補（単一出力）と一致するか。"""
        S = self._size
        high = np.asarray(dec["high_res_mask"], np.float32).reshape(S, S)
        diffs = [np.abs(high - cv2.resize(c, (S, S), interpolation=cv2.INTER_LINEAR)).max() for c in cand_all]
        return int(np.argmin(diffs)) == 0

    def _dam_update(self, t: int, cand: np.ndarray, ious: np.ndarray, idx: int, n_pixels: int) -> None:
        """dam4sam_tracker.track の DRM に足すかの判定（候補マスクの解像度で比べる）。"""
        self._dam_sizes.append(n_pixels)
        recent = [s for s in self._dam_sizes[-300:] if s >= 1][-10:]
        ratio = n_pixels / np.median(recent) if len(self._dam_sizes) > 1 and n_pixels >= 1 else -1
        lo, hi = DAM_SIZE_RATIO
        if not (
            ious[idx] > DAM_MIN_IOU and lo <= ratio <= hi and n_pixels >= 1
            and (t - self._dam_last_added > DAM_MIN_GAP or self._dam_last_added == -1)
        ):
            return
        chosen = cand[idx] > 0
        alternatives = [cand[j] > 0 for j in range(len(cand)) if j != idx]
        if dam_has_distractor(chosen, alternatives):
            self._dam_last_added = t
            self._drm.append(t)
            self.stats["drm_added"].append(t)
