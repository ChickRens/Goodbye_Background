"""ロゴ画像からバッジ用 PNG とファビコンを生成する（開発時に 1 回だけ実行。Pillow は dev 依存）。

使い方: cd worker && uv run python scripts/make_logo.py
"""

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
SRC = ROOT / "assets" / "brand" / "logo-original.webp"
OUT_LOGO = ROOT / "worker" / "cutout" / "assets" / "logo.png"
OUT_FAVICON = ROOT / "frontend" / "video-favicon.png"
RADIUS_RATIO = 0.18  # 角丸の半径 = 一辺の 18%
SS = 4  # 角丸マスクのスーパーサンプリング倍率


def rounded(img: Image.Image, size: int) -> Image.Image:
    """正方形にリサイズし、角丸の外側だけ透明にする（内側は完全に不透明）。"""
    img = img.resize((size, size), Image.LANCZOS).convert("RGBA")
    big = size * SS
    mask = Image.new("L", (big, big), 0)
    r = round(big * RADIUS_RATIO)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, big - 1, big - 1), radius=r, fill=255)
    img.putalpha(mask.resize((size, size), Image.LANCZOS))
    return img


def main() -> None:
    src = Image.open(SRC).convert("RGB")
    w, h = src.size
    side = min(w, h)
    left, top = (w - side) // 2, (h - side) // 2
    square = src.crop((left, top, left + side, top + side))
    OUT_LOGO.parent.mkdir(parents=True, exist_ok=True)
    rounded(square, 256).save(OUT_LOGO)
    rounded(square, 32).save(OUT_FAVICON)


if __name__ == "__main__":
    main()
