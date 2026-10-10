use std::ops::{Div, Mul};

use crate::point::coordinate::Coordinate;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Point {
    coordinate: Coordinate,
    label: PointLabel,
}

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
