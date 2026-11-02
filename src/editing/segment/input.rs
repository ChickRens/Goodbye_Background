//! segment の入力。`point` の座標は **プレビュー画像（長辺600px）上のピクセル座標**。

use crate::{point::point::Point, session::id::SessionId};

pub struct SegmentInput {
    session_id: SessionId,
    point: Point,
}

impl SegmentInput {
    pub fn into_parts(self) -> (SessionId, Point) {
        (self.session_id, self.point)
    }

    pub fn new(session_id: SessionId, point: Point) -> Self {
        Self { session_id, point }
    }
}
