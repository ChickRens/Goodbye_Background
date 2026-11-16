//! SAM2 前処理の入力。

use crate::{image_cutout::image::image::id::ImageId, image_cutout::session::id::SessionId};

pub struct PrepareSegmentInput {
    image_id: ImageId,
    session_id: SessionId,
    /// プレビュー座標 → 元画像座標の変換に使う倍率（元画像座標 = プレビュー座標 / point_scale）。
    point_scale: f64,
}

impl PrepareSegmentInput {
    pub fn new(
        image_id: ImageId,
        session_id: SessionId,
        preview_to_original_point_scale: f64,
    ) -> Self {
        Self {
            image_id,
            session_id,
            point_scale: preview_to_original_point_scale,
        }
    }

    pub fn image_id(&self) -> ImageId {
        self.image_id
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn point_scale(&self) -> f64 {
        self.point_scale
    }
}
