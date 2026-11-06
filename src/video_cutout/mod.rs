//! 動画切り抜き機能。ジョブのドメインの値・時計・回数制限・外部へのポート（R2・ワーカー）と、ユースケースを持つ。

pub mod clock;
pub mod clock_impl;
pub mod create_job;
pub mod job;
pub mod quota;
pub mod signature;
pub mod start_job;
pub mod storage;
#[cfg(test)]
pub mod testing;
pub mod worker;
