//! 編集セッション本体。1アップロード（= 1 SessionId）につき1つ。
//!
//! 保持するもの:
//! - `static_context`  : SAM2 の画像特徴量。`/upload` 時に1回だけ計算し、以後不変（約16MB）
//! - `point_history`   : ユーザーが打ったポイント（**元画像座標**で保存）
//! - `inference_context_history` : 各時点の SAM2 出力マスク（次の推論のヒントとして使う）
//! - `point_scale`     : プレビュー座標 → 元画像座標の変換倍率（`point / point_scale`）
//!
//! 2つの履歴は常に同時に add/undo/redo され、同じ長さ・同じ cursor 位置を保つ。
//!
//! `EditingSession` トレイトはジェネリクスの関連型を外に見せるためのもの（実装は `CommonEditingSession` だけ）。

use crate::{
    image_cutout::editing::session::{
        error::EditingSessionError, inference_context_history::InferenceContextHistory,
    },
    image_cutout::point::{point::Point, point_history::PointHistory},
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

    /// 2つの履歴を同時に1つ戻す。可否が食い違っていたら履歴破損（バグ）として扱う。
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

    /// 既存ポイント + 新しいポイント（プレビュー座標 → 元画像座標に変換して追加）。
    /// セッション自体は変更しない（推論が成功してから `apply_edit` で確定するため）。
    fn points_with(&self, point: Point) -> Vec<Point> {
        let mut points = self.points().to_vec();
        points.push(point / self.point_scale);
        points
    }

    /// 推論成功後に呼ぶ。ポイント（プレビュー座標で受け取り元画像座標で保存）と
    /// 新しいマスクを、それぞれの履歴に追加する（redo 履歴は破棄される）。
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
