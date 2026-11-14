//! SAM2 へのプロンプト1点 = 座標 + ラベル（前景/背景）。

use std::ops::{Div, Mul};

use crate::image_cutout::point::coordinate::Coordinate;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Point {
    coordinate: Coordinate,
    label: PointLabel,
}

/// ポイントの種類。数値は SAM2 の `point_labels` 入力にそのまま渡される（`as i64`）。
/// 1 = 前景（残す）, 0 = 背景（消す）。SAM2 公式の定義と同じ値なので変更しないこと。
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum PointLabel {
    BACKGROUND = 0,
    FOREGROUND = 1,
}

impl Point {
    pub fn new(coordinate: Coordinate, label: PointLabel) -> Self {
        Self { coordinate, label }
    }

    pub fn coordinate(&self) -> Coordinate {
        self.coordinate.clone()
    }

    pub fn label(&self) -> PointLabel {
        self.label.clone()
    }
}

impl Mul<f64> for Point {
    type Output = Self;

    fn mul(self, scale: f64) -> Self {
        Self {
            coordinate: self.coordinate * scale,
            label: self.label,
        }
    }
}

impl Div<f64> for Point {
    type Output = Self;

    fn div(self, scale: f64) -> Self {
        Self {
            coordinate: self.coordinate / scale,
            label: self.label,
        }
    }
}
