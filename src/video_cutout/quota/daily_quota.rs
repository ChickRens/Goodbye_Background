//! 1 日の回数制限の trait。client_id と IP の両方で数え、どちらかが上限に達したら拒否する。

use std::net::{IpAddr, Ipv6Addr};

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
            ("ip", ip_quota_key(self.ip)),
        ]
    }
}

/// IP の回数のキー。IPv4 はアドレスそのもの、IPv6 は上位 64 ビット（/64）。
///
/// IPv6 では 1 台の端末（1 回線）に /64 が丸ごと割り当てられ、スマホはその中でアドレスを使い捨てる
/// （プライバシー拡張）。アドレス単位で数えると、アドレスが変わるたびに回数が戻ってしまう。
/// IPv4 射影アドレス（`::ffff:a.b.c.d`）は IPv4 として数える。
fn ip_quota_key(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => {
                let prefix = u128::from(v6) & !((1u128 << 64) - 1);
                format!("{}/64", Ipv6Addr::from(prefix))
            }
        },
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

#[cfg(test)]
mod daily_quota_test {
    use super::*;

    fn keys(ip: &str) -> QuotaKeys {
        QuotaKeys { client_id: Uuid::nil(), ip: ip.parse().unwrap() }
    }

    #[test]
    fn ipv4_is_counted_per_address() {
        assert_eq!(keys("203.0.113.7").rows()[1], ("ip", "203.0.113.7".to_string()));
    }

    #[test]
    fn ipv6_is_counted_per_64_prefix() {
        // スマホは同じ /64 の中でアドレスを使い捨てる（プライバシー拡張）。アドレス単位だと回数をすり抜けられる
        let a = keys("2001:db8:d2f6:abba:5e9:be22:aa6e:7b86").rows()[1].clone();
        let b = keys("2001:db8:d2f6:abba:1:2:3:4").rows()[1].clone();
        assert_eq!(a, ("ip", "2001:db8:d2f6:abba::/64".to_string()));
        assert_eq!(a, b);
        assert_ne!(a, keys("2001:db8:d2f6:abbb::1").rows()[1]);
    }

    #[test]
    fn ipv4_mapped_ipv6_is_counted_as_ipv4() {
        assert_eq!(keys("::ffff:203.0.113.7").rows()[1], ("ip", "203.0.113.7".to_string()));
    }
}
