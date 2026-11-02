//! 利用者識別子。フロントが `client-id` ヘッダーで送る UUID（ブラウザごとに1つ、localStorage に保存）。
//! ヘッダーが無い/不正な場合は `None` として記録される。

use uuid::Uuid;

pub struct UsageId {
    value: Uuid,
}

impl UsageId {
    pub fn new(value: Uuid) -> Self {
        Self { value }
    }

    pub fn value(&self) -> Uuid {
        self.value
    }
}
