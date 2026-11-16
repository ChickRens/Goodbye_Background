//! 出力動画の背景色（クロマキー用の単色）。

use crate::video_cutout::job::error::VideoJobError;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Background {
    Green,
    Blue,
    Magenta,
}

impl Background {
    pub fn rgb(&self) -> [u8; 3] {
        match self {
            Background::Green => [0, 255, 0],
            Background::Blue => [0, 0, 255],
            Background::Magenta => [255, 0, 255],
        }
    }

    pub fn from_str(s: &str) -> Result<Self, VideoJobError> {
        match s {
            "green" => Ok(Background::Green),
            "blue" => Ok(Background::Blue),
            "magenta" => Ok(Background::Magenta),
            _ => Err(VideoJobError::InvalidBackground),
        }
    }
}

#[cfg(test)]
mod background_test {
    use super::*;

    #[test]
    fn parses_and_maps_rgb() {
        assert_eq!(Background::from_str("green").unwrap().rgb(), [0, 255, 0]);
        assert_eq!(Background::from_str("blue").unwrap().rgb(), [0, 0, 255]);
        assert_eq!(Background::from_str("magenta").unwrap().rgb(), [255, 0, 255]);
    }

    #[test]
    fn rejects_unknown() {
        assert_eq!(Background::from_str("red"), Err(VideoJobError::InvalidBackground));
        assert_eq!(Background::from_str("Green"), Err(VideoJobError::InvalidBackground));
    }
}
