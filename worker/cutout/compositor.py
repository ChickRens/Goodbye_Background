"""マスクの合成とロゴのバッジ。入出力は RGB uint8 の numpy 配列。"""

import math
from pathlib import Path

import cv2
import numpy as np

# alpha を硬くする鋭さ。alpha = sigmoid(EDGE_SHARPNESS * ロジット)。
# 16 だと alpha 0.05〜0.95 はロジット約 ±0.18 だけ（ほぼ二値、サブピクセルのアンチエイリアス）。
# ロジット ±0.25 で alpha 約 0.02 / 0.98。
# 本番の計測（踊っている動画 576x1024・300 フレーム、1 フレームあたりの緑かぶり画素の平均）:
#   元の柔らかい alpha 4573 → EDGE_SHARPNESS=4 で 2206。
#   完全に硬い合成を同じ libx264 yuv420p でエンコードし直しても約 620 画素は残る（クロマ間引きのため）。
#   つまり 4 でも約 1600 画素は、ブレた領域で SAM2 のロジットが 0 付近に留まる半透明の alpha だった → 16 に上げた。
# 残る約 1px の緑の縁は yuv420p のクロマ間引きによるもの。
# CapCut / iPhone / SNS との互換性のため yuv420p は変えない。
EDGE_SHARPNESS = 16.0

# 縁を縮めて滑らかにする（shrink_edge。2026-11-11。オーナー判断: 輪郭のギザギザと、縁に残る背景の白い線を消す）。
# 出力の画素（長辺 1080 以下）での値。D_man（576x1024）で 1.0/1.0・1.0/1.5・1.5/2.0 を比べ、1.5/2.0 で白い線が消え、
# 拳や腕は残った。プレビュー（Rust の video_cutout/preview/image_math.rs）にも同じ値がある。変えるときは両方直す
EDGE_SMOOTH_SIGMA_PX = 1.5  # 縁を滑らかにするガウスぼかしの σ
EDGE_SHRINK_PX = 2.0  # 縁を内側へ縮める量


def alpha_from_logits(logits_512: np.ndarray, width: int, height: int) -> np.ndarray:
    """512x512 のロジットを [H,W] float32 の alpha にする。

    バイリニア拡大 → ロジット空間で EDGE_SHARPNESS 倍して sigmoid（= 硬めの alpha）。
    erode はしない。理由（本番のフィードバック、2026-11-07）:
    - CapCut のクロマキーで抜くので、縁が半透明だと背景色（緑）と混ざって穴が開く。
      動きが速くてブレた手などは確率が 0.3〜0.6 の中間になりやすく、確率をそのまま alpha にすると半透明になる。
    - 3x3 の erode は指や手首を細くしてしまう。
    ロジット空間で鋭くするだけなら、境界（ロジット 0 = alpha 0.5）の位置は動かず、内側へ縮まない。
    """
    up = cv2.resize(logits_512.astype(np.float32), (width, height), interpolation=cv2.INTER_LINEAR)
    # SAM2 は -1024 のような極端なロジットを出す。exp のオーバーフロー警告を避けるためクリップする
    up = np.clip(up, -50.0, 50.0)
    # 16 倍すると float32 の exp が溢れる（exp(800)）ので、鋭くした後にもう一度クリップする
    sharp = np.clip(EDGE_SHARPNESS * up, -50.0, 50.0)
    return (1.0 / (1.0 + np.exp(-sharp))).astype(np.float32)


def _normal_cdf(z: float) -> float:
    return 0.5 * (1.0 + math.erf(z / math.sqrt(2.0)))


def _normal_pdf(z: float) -> float:
    return math.exp(-0.5 * z * z) / math.sqrt(2.0 * math.pi)


def gaussian_ksize(sigma: float) -> int:
    """ぼかしの窓の大きさ（±3σ を含む奇数）。Rust のプレビューと同じ式。"""
    return 2 * math.ceil(3.0 * sigma) + 1


def shrink_edge(alpha: np.ndarray, px_scale: float = 1.0) -> np.ndarray:
    """alpha の縁を EDGE_SHRINK_PX だけ内側へ縮め、階段状のギザギザを滑らかにする。[H,W] float32 を返す。

    1. alpha を σ = EDGE_SMOOTH_SIGMA_PX でぼかす（g）。まっすぐな縁なら g = Φ(縁からの距離 / σ) になる
    2. g が Φ(EDGE_SHRINK_PX / σ) の所を新しい縁にする（= ちょうど EDGE_SHRINK_PX 内側）。
       その前後 1 画素で 0→1 になるよう傾きを付ける（半透明は 1〜2 画素だけ。CapCut のクロマキーで穴が開かない）
    ロジットの閾値をずらすのでは駄目だった: 縁でのロジットの傾きがフレームで違い、同じ値で 0.6〜17 画素と縮み方がばらついた。
    px_scale は出力より大きい画像（プレビュー）で同じ見た目にするための倍率（出力では 1）。
    """
    sigma = EDGE_SMOOTH_SIGMA_PX * px_scale
    z = EDGE_SHRINK_PX / EDGE_SMOOTH_SIGMA_PX
    level = _normal_cdf(z)
    gain = sigma / _normal_pdf(z)  # 縁での g の傾きは pdf(z)/σ（1 画素あたり）→ 1 画素で 0→1
    k = gaussian_ksize(sigma)
    g = cv2.GaussianBlur(alpha.astype(np.float32), (k, k), sigma, sigmaY=sigma, borderType=cv2.BORDER_REFLECT_101)
    return np.clip(0.5 + (g - level) * gain, 0.0, 1.0).astype(np.float32)


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
