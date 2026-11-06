//! ピクセル座標 (x, y)。左上原点、x が右、y が下。
//!
//! どの座標系の値かは文脈次第なので注意（`docs/ARCHITECTURE.md` の「座標系」参照）:
//! - ハンドラで作られた直後: プレビュー画像（長辺600px）座標
//! - `PointHistory` に保存された後: 元画像座標
//! - SAM2 に渡す直前（`_scale_prompt` 後）: 1024x1024 座標（ここは f32）

use std::ops::{Div, Mul};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Coordinate {
    x: u16,
    y: u16,
}

impl Coordinate {
    pub fn new(x: u16, y: u16) -> Self {
        Self { x, y }
    }

    pub fn x(&self) -> u16 {
        self.x
    }

    pub fn y(&self) -> u16 {
        self.y
    }
}

/// 座標の拡大縮小。結果は `as u16` で **切り捨て**（四捨五入ではない）。
/// 負数や 65535 超は飽和する（Rust の float→int `as` キャストの仕様）。
impl Mul<f64> for Coordinate {
    type Output = Self;

    fn mul(self, scale: f64) -> Self {
        Self {
            x: (self.x as f64 * scale) as u16,
            y: (self.y as f64 * scale) as u16,
        }
    }
}

/// 座標の拡大縮小（割り算版）。`Mul` と同じく切り捨て。
/// プレビュー座標 → 元画像座標の変換（`point / point_scale`）で使われる。
impl Div<f64> for Coordinate {
    type Output = Self;

    fn div(self, scale: f64) -> Self {
        Self {
            x: (self.x as f64 / scale) as u16,
            y: (self.y as f64 / scale) as u16,
        }
    }
}
