//! undo/redo 可能なポイント履歴。
//!
//! 仕組み（テキストエディタの undo と同じ「カーソル + 配列」方式）:
//! ```text
//! items  = [p0, p1, p2, p3]
//! cursor = 3            → current() は [p0, p1, p2]（p3 は redo で戻せる）
//! base_len = 0          → cursor をここより小さくできない（undo の下限）
//! ```
//! - `add`  : cursor 以降を捨てて（redo 履歴破棄）末尾に追加
//! - `undo` : cursor -= 1 / `redo`: cursor += 1
//! - undo できる回数が `max_undo` を超えたら `base_len` を進めて古い点を「確定」させる
//!   （古い点は配列から消さず、SAM2 への入力には含まれ続ける。undo で戻れなくなるだけ）
//!
//! `InferenceContextHistory` と常にセットで操作される（`CommonEditingSession` 参照）。
//! 2つの履歴は初期状態の扱いが違う（こちらは空配列、向こうは初期要素1つ）ので、
//! cursor の意味が1ずれている点に注意。
//!
//! 注意: `undo`/`redo` は `can_undo`/`can_redo` をチェックしない。呼ぶ前に必ず確認すること
//! （チェック無しで `undo` すると usize のアンダーフローで panic する）。

use crate::point::point::Point;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct PointHistory {
    items: Vec<Point>,
    cursor: usize,
    max_undo: usize,

    // 確定されたitemsのlen（これより前には undo で戻れない）
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

    /// 現在有効なポイント列（SAM2 に渡す全ポイント）。
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
