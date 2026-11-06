"""マスクの合成とロゴのバッジ。入出力は RGB uint8 の numpy 配列。"""

from pathlib import Path

import cv2
import numpy as np

_ERODE_KERNEL = np.ones((3, 3), np.uint8)


def alpha_from_logits(logits_512: np.ndarray, width: int, height: int) -> np.ndarray:
    """512x512 のロジットを [H,W] float32 の alpha にする。

    バイリニア拡大 → sigmoid → 3x3 の erode を 1 回（縁の色にじみを内側へ削る）。
    """
    up = cv2.resize(logits_512.astype(np.float32), (width, height), interpolation=cv2.INTER_LINEAR)
    # SAM2 は -1024 のような極端なロジットを出す。exp のオーバーフロー警告を避けるためクリップする
    up = np.clip(up, -50.0, 50.0)
    prob = (1.0 / (1.0 + np.exp(-up))).astype(np.float32)
    return cv2.erode(prob, _ERODE_KERNEL)


def composite(
    frame_rgb: np.ndarray,
    alpha: np.ndarray | None,
    background_rgb: tuple[int, int, int],
) -> np.ndarray:
    """前景を背景色に合成する。alpha=None は見失ったフレーム（全面背景色）。"""
    bg = np.empty_like(frame_rgb)
    bg[:] = np.array(background_rgb, np.uint8)
    if alpha is None:
        return bg
    a = alpha.astype(np.float32)[..., None]
    out = frame_rgb.astype(np.float32) * a + bg.astype(np.float32) * (1.0 - a)
    return np.clip(np.rint(out), 0, 255).astype(np.uint8)


class LogoBadge:
    """右下に置く角丸のロゴバッジ。角丸の内側は不透明なのでクロマキーで消えない。"""

    def __init__(self, logo_rgba: np.ndarray):
        self._logo = logo_rgba  # RGBA uint8
        self._cache: dict[int, tuple[np.ndarray, np.ndarray]] = {}

    @classmethod
    def load(cls, path: Path) -> "LogoBadge":
        bgra = cv2.imread(str(path), cv2.IMREAD_UNCHANGED)
        if bgra is None or bgra.ndim != 3 or bgra.shape[2] != 4:
            raise ValueError(f"ロゴ PNG を読めません: {path}")
        return cls(cv2.cvtColor(bgra, cv2.COLOR_BGRA2RGBA))

    def _scaled(self, size: int) -> tuple[np.ndarray, np.ndarray]:
        if size not in self._cache:
            small = cv2.resize(self._logo, (size, size), interpolation=cv2.INTER_AREA)
            alpha = small[..., 3:4].astype(np.float32) / 255.0
            self._cache[size] = (small[..., :3].astype(np.float32), alpha)
        return self._cache[size]

    def apply(self, frame_rgb: np.ndarray) -> np.ndarray:
        """バッジを重ねた新しいフレームを返す（入力は変更しない）。"""
        h, w = frame_rgb.shape[:2]
        short = min(h, w)
        size = max(16, round(short * 0.08))
        margin = round(short * 0.02)
        y0, x0 = h - margin - size, w - margin - size
        if y0 < 0 or x0 < 0:
            return frame_rgb.copy()
        rgb, alpha = self._scaled(size)
        out = frame_rgb.copy()
        region = out[y0 : y0 + size, x0 : x0 + size].astype(np.float32)
        blended = rgb * alpha + region * (1.0 - alpha)  # alpha は角丸の切り抜きにだけ使う
        out[y0 : y0 + size, x0 : x0 + size] = np.clip(np.rint(blended), 0, 255).astype(np.uint8)
        return out
