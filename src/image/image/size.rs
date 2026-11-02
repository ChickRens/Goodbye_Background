//! 画像サイズ（高さ・幅）。生成時にバリデーションする値オブジェクト。
//!
//! 注意: 引数の順番は `new(height, width)`（幅が先ではない）。取り違えに注意。

use crate::image::image::error::ImageSizeError;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ImageSize {
    height: u16,
    width: u16,
}

impl ImageSize {
    // 許容範囲: 2〜8000px（縦横それぞれ）。
    // 注意: rule.txt には 3000px までと書かれているが、実装は 8000px。
    const MIN_DIMENSION: u16 = 2;
    const MAX_DIMENSION: u16 = 8000;

    pub fn new(height: u16, width: u16) -> Result<Self, ImageSizeError> {
        match (height, width) {
            (h, _) if h < Self::MIN_DIMENSION => Err(ImageSizeError::ShortHeight),
            (h, _) if h > Self::MAX_DIMENSION => Err(ImageSizeError::LongHeight),
            (_, w) if w < Self::MIN_DIMENSION => Err(ImageSizeError::ShortWidth),
            (_, w) if w > Self::MAX_DIMENSION => Err(ImageSizeError::LongWidth),
            _ => Ok(Self { height, width }),
        }
    }

    /// デコーダが返す u32 のサイズから作る。u16 に収まらない値は範囲外（Long*）として弾く。
    /// （`as u16` で先にキャストすると 70000 → 4464 のように回り込んで検証をすり抜けるため）
    pub fn from_u32(height: u32, width: u32) -> Result<Self, ImageSizeError> {
        let height = u16::try_from(height).unwrap_or(u16::MAX);
        let width = u16::try_from(width).unwrap_or(u16::MAX);
        Self::new(height, width)
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    /// アスペクト比を保って (max_height, max_width) に収まるよう拡大縮小したサイズを返す。
    /// バリデーションを通さないので MIN/MAX の範囲外になり得る。現状は未使用（テストのみ）。
    pub fn fit_within(&self, max_height: u16, max_width: u16) -> Self {
        let scale: f32 =
            (max_height as f32 / self.height as f32).min(max_width as f32 / self.width as f32);

        Self {
            height: (self.height as f32 * scale) as u16,
            width: (self.width as f32 * scale) as u16,
        }
    }
}

#[cfg(test)]
mod image_size_tests {
    use crate::image::image::{error::ImageSizeError, size::ImageSize};

    #[test]
    fn test_valid_size() {
        let size = ImageSize::new(200, 1000).unwrap();
        assert_eq!(size.height(), 200);
        assert_eq!(size.width(), 1000);
    }

    #[test]
    fn test_long_height() {
        let size = ImageSize::new(10000, 2000);
        assert_eq!(size, Err(ImageSizeError::LongHeight))
    }

    #[test]
    fn test_short_height() {
        let size = ImageSize::new(1, 3);
        assert_eq!(size, Err(ImageSizeError::ShortHeight))
    }

    #[test]
    fn test_long_width() {
        let size = ImageSize::new(200, 10000);
        assert_eq!(size, Err(ImageSizeError::LongWidth))
    }

    #[test]
    fn test_short_width() {
        let size = ImageSize::new(400, 0);
        assert_eq!(size, Err(ImageSizeError::ShortWidth))
    }

    #[test]
    fn test_fit_within_scale_down_height() {
        let size = ImageSize::new(100, 300).unwrap();
        let result = size.fit_within(50, 300);
        assert_eq!(result, ImageSize::new(50, 150).unwrap())
    }

    #[test]
    fn test_fit_within_scale_down_width() {
        let size = ImageSize::new(500, 300).unwrap();
        let result = size.fit_within(170, 50);
        assert_eq!(result, ImageSize::new(83, 50).unwrap())
    }

    #[test]
    fn test_fit_within_scale_up_height() {
        let size = ImageSize::new(100, 300).unwrap();
        let result = size.fit_within(500, 2500);
        assert_eq!(result, ImageSize::new(500, 1500).unwrap())
    }

    #[test]
    fn test_fit_within_scale_up_width() {
        let size = ImageSize::new(700, 1000).unwrap();
        let result = size.fit_within(2500, 1500);
        assert_eq!(result, ImageSize::new(1050, 1500).unwrap())
    }

    #[test]
    fn test_fit_within_no_change_scale() {
        let size = ImageSize::new(500, 700).unwrap();
        let result = size.fit_within(500, 700);
        assert_eq!(result, ImageSize::new(500, 700).unwrap())
    }
}
