//! 1 日の回数制限の trait。client_id と IP の両方で数え、どちらかが上限に達したら拒否する。

use std::net::IpAddr;

use async_trait::async_trait;
use uuid::Uuid;

use super::error::QuotaError;

/// 1 日あたりの上限回数。
pub const DAILY_LIMIT: i64 = 3;

/// 回数を数えるキー（client_id と IP アドレス）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaKeys {
    pub client_id: Uuid,
    pub ip: IpAddr,
}

impl QuotaKeys {
    /// DB に保存する（key_kind, key）の組。
    pub(super) fn rows(&self) -> [(&'static str, String); 2] {
        [
            ("client", self.client_id.to_string()),
            ("ip", self.ip.to_string()),
        ]
    }
}

/// 残り回数 = 上限 - max(client の回数, ip の回数)（0 未満にはしない）。
pub(super) fn remaining_from(client_count: i64, ip_count: i64) -> u8 {
    (DAILY_LIMIT - client_count.max(ip_count)).clamp(0, DAILY_LIMIT) as u8
}

#[async_trait]
pub trait DailyQuota: Send + Sync {
    /// `day` の残り回数（消費しない）。
    async fn remaining(&self, keys: &QuotaKeys, day: i64) -> Result<u8, QuotaError>;
    /// 1 回消費し、消費後の残り回数を返す。上限に達していれば `Exceeded`。
    async fn try_consume(&self, keys: &QuotaKeys, day: i64) -> Result<u8, QuotaError>;
    /// 1 回分を戻す（0 未満にはならない）。
    async fn refund(&self, keys: &QuotaKeys, day: i64) -> Result<(), QuotaError>;
}
