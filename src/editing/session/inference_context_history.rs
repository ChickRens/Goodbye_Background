//! 推論コンテキスト（SAM2 では直前のマスク）の undo/redo 履歴。
//!
//! `PointHistory` と同じ考え方だが、**初期要素を1つ持つ**点が違う:
//! ```text
//! PointHistory:            items=[]            cursor=0  (current = [] 空)
//! InferenceContextHistory: items=[ctx0(None)]  cursor=0  (current = ctx0)
//! 1点追加後:
//! PointHistory:            items=[p0]          cursor=1  (current = [p0])
//! InferenceContextHistory: items=[ctx0, ctx1]  cursor=1  (current = ctx1)
//! ```
//! つまり cursor の値は両者で常に一致する。`can_undo`/`can_redo` の結果も一致するはずで、
//! `CommonEditingSession` がそれを検査している。
//!
//! 注意: `undo`/`redo` は境界チェックをしない。必ず `can_undo`/`can_redo` を先に確認すること。

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct InferenceContextHistory<T> {
    items: Vec<T>,
    cursor: usize,
    base_len: usize,
    max_undo: usize,
}

impl<T> InferenceContextHistory<T> {
    pub fn new(max_history: usize, initial: T) -> Self {
        Self {
            items: vec![initial],
            cursor: 0,
            base_len: 0,
            max_undo: max_history,
        }
    }

    pub fn add(&mut self, item: T) {
        // current_index以降の履歴を削除（redo履歴を破棄）
        self.items.truncate(self.cursor + 1);
        self.items.push(item);
        self.cursor = self.items.len() - 1;

        if self.cursor - self.base_len > self.max_undo {
            self.base_len += 1;
        }
    }

    pub fn current(&self) -> &T {
        &self.items[self.cursor]
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > self.base_len
    }

    pub fn undo(&mut self) {
        self.cursor -= 1;
    }

    pub fn can_redo(&self) -> bool {
        self.cursor + 1 < self.items.len()
    }

    pub fn redo(&mut self) {
        self.cursor += 1;
    }
}
