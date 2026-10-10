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

impl Mul<f64> for Coordinate {
    type Output = Self;

    fn mul(self, scale: f64) -> Self {
        Self {
            x: (self.x as f64 * scale) as u16,
            y: (self.y as f64 * scale) as u16,
        }
    }
}

impl Div<f64> for Coordinate {
    type Output = Self;

    fn div(self, scale: f64) -> Self {
        Self {
            x: (self.x as f64 / scale) as u16,
            y: (self.y as f64 / scale) as u16,
        }
    }
}
