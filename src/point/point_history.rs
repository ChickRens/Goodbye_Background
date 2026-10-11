use crate::domain::value_object::point::Point;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct PointHistory {
    items: Vec<Point>,
    cursor: usize,
    max_undo: usize,

    // 確定されたitemsのlen
    base_len: usize,
}

impl PointHistory {
    pub fn new(max_undo: usize) -> Self {
        Self {
            items: vec![],
            cursor: 0,
            max_undo,
            base_len: 0,
        }
    }

    pub fn add(&mut self, item: Point) {
        // current_index以降の履歴を削除（redo履歴を破棄）
        self.items.truncate(self.cursor);
        self.items.push(item);

        self.cursor = self.items.len();

        if self.cursor - self.base_len > self.max_undo {
            self.base_len += 1;
        }
    }

    pub fn current(&self) -> &[Point] {
        &self.items[..self.cursor]
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > self.base_len
    }

    pub fn undo(&mut self) {
        self.cursor -= 1;
    }

    pub fn can_redo(&self) -> bool {
        self.cursor < self.items.len()
    }

    pub fn redo(&mut self) {
        self.cursor += 1;
    }
}
