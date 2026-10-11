use crate::{
    application::types::{
        editing_session::error::EditingSessionError,
        inference_context_history::InferenceContextHistory, point_history::PointHistory,
    },
    domain::value_object::point::Point,
};

#[derive(Debug, PartialEq, Clone)]
pub struct CommonEditingSession<StaticContext, InferenceContext> {
    point_history: PointHistory,
    static_context: StaticContext,
    inference_context_history: InferenceContextHistory<InferenceContext>,
    point_scale: f64,
}

pub trait EditingSession {
    type StaticContext;
    type InferenceContext;

    fn undo(&mut self) -> Result<(), EditingSessionError>;
    fn redo(&mut self) -> Result<(), EditingSessionError>;
    fn points(&self) -> &[Point];
    fn points_with(&self, point: Point) -> Vec<Point>;
    fn static_context(&self) -> &Self::StaticContext;
    fn inference_context(&self) -> &Self::InferenceContext;

    fn apply_edit(&mut self, point: Point, inference_context: Self::InferenceContext);
}

impl<S, I> EditingSession for CommonEditingSession<S, I> {
    type InferenceContext = I;
    type StaticContext = S;

    fn undo(&mut self) -> Result<(), EditingSessionError> {
        if self.inference_context_history.can_undo() != self.point_history.can_undo() {
            return Err(EditingSessionError::HistoryCorrupted);
        }

        if !(self.inference_context_history.can_undo() && self.point_history.can_undo()) {
            return Err(EditingSessionError::UndoFailed);
        }

        self.point_history.undo();
        self.inference_context_history.undo();
        Ok(())
    }

    fn redo(&mut self) -> Result<(), EditingSessionError> {
        if self.inference_context_history.can_redo() != self.point_history.can_redo() {
            return Err(EditingSessionError::HistoryCorrupted);
        }

        if !(self.inference_context_history.can_redo() && self.point_history.can_redo()) {
            return Err(EditingSessionError::RedoFailed);
        }

        self.point_history.redo();
        self.inference_context_history.redo();

        Ok(())
    }

    fn points(&self) -> &[Point] {
        self.point_history.current()
    }

    fn points_with(&self, point: Point) -> Vec<Point> {
        let mut points = self.points().to_vec();
        points.push(point / self.point_scale);
        points
    }

    fn apply_edit(&mut self, point: Point, inference_context: Self::InferenceContext) {
        self.point_history.add(point / self.point_scale);
        self.inference_context_history.add(inference_context);
    }

    fn inference_context(&self) -> &Self::InferenceContext {
        &self.inference_context_history.current()
    }

    fn static_context(&self) -> &Self::StaticContext {
        &self.static_context
    }
}

impl<S, I> CommonEditingSession<S, I> {
    pub fn new(
        history: PointHistory,
        static_context: S,
        inference_context_history: InferenceContextHistory<I>,
        point_scaler: f64,
    ) -> Self {
        Self {
            point_history: history,
            static_context: static_context,
            inference_context_history: inference_context_history,
            point_scale: point_scaler,
        }
    }
}
