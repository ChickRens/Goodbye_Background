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
