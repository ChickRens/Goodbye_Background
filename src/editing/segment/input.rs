use crate::domain::value_object::{point::Point, session_id::session_id::SessionId};

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
