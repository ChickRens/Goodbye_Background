//! 編集セッション単位のロック表。
//!
//! `SegmentServiceImpl` は「編集セッションを取得 → clone して変更 → 丸ごと保存」という
//! read-modify-write を行う。同じセッションへのリクエストが同時に来ると後勝ちで片方の変更が
//! 消える（lost update）ので、操作全体をこのロックで囲んでセッションごとに直列化する。
//! 別セッション同士は並行に動ける。
//!
//! ロック本体は `TtlStore` に入れているので、他のストアと同じく GC で期限切れ分が削除される
//! （`DeleteExpiredEntriesUseCase` に登録すること）。

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::{
    maintenance::{
        deleting_expired_entries::{clock::clock::AppClock, repository::DeleteExpiredRepository},
        ttl_store::TtlStore,
    },
    session::id::SessionId,
};

pub struct EditingSessionLocks {
    locks: TtlStore<SessionId, Mutex<()>>,
}

impl EditingSessionLocks {
    pub fn new(clock: impl AppClock + 'static) -> Self {
        Self {
            locks: TtlStore::new(Arc::new(clock)),
        }
    }

    /// セッションのロックを返す（無ければ作る）。呼び出し側で `.lock()` して、
    /// ガードを操作の最後まで保持すること。
    pub fn lock_for(&self, session_id: SessionId) -> Arc<Mutex<()>> {
        self.locks.get_or_insert_with(session_id, || Mutex::new(()))
    }
}

impl DeleteExpiredRepository for EditingSessionLocks {
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.locks.delete_expired(now, ttl);
    }
}
