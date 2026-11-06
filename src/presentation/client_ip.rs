//! 回数制限に使う利用者の IP の取り方（仕様 §4.6）。
//!
//! `X-Forwarded-For` の先頭（左端）は利用者が自由に書けるので使わない。信頼できるプロキシ（Cloud Run の
//! フロントエンドなど）は受け取った値の後ろに接続元を書き足すだけなので、右から数えて `trusted_hops` 番目を使う。

use std::net::{IpAddr, SocketAddr};

use axum::http::HeaderMap;

/// `trusted_hops` = 0 なら接続元（`ConnectInfo`）。N ≥ 1 なら `X-Forwarded-For` を右から数えて N 番目。
/// ヘッダーが無い・要素が足りない・IP として読めない場合は接続元を使う。
pub fn client_ip(headers: &HeaderMap, peer: SocketAddr, trusted_hops: usize) -> IpAddr {
    if trusted_hops == 0 {
        return peer.ip();
    }
    headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .nth(trusted_hops - 1)
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| peer.ip())
}

#[cfg(test)]
mod client_ip_test {
    use std::net::{IpAddr, SocketAddr};

    use axum::http::HeaderMap;

    use super::client_ip;

    fn xff(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", value.parse().unwrap());
        headers
    }

    fn peer(ip: &str) -> SocketAddr {
        SocketAddr::new(ip.parse().unwrap(), 12345)
    }

    fn ip(ip: &str) -> IpAddr {
        ip.parse().unwrap()
    }

    #[test]
    fn client_ip_uses_peer_when_hops_zero() {
        assert_eq!(
            client_ip(&xff("1.1.1.1"), peer("9.9.9.9"), 0),
            ip("9.9.9.9")
        );
    }

    #[test]
    fn client_ip_takes_nth_from_right() {
        assert_eq!(
            client_ip(&xff("6.6.6.6, 2.2.2.2"), peer("9.9.9.9"), 1),
            ip("2.2.2.2")
        );
        assert_eq!(
            client_ip(&xff("6.6.6.6, 2.2.2.2, 3.3.3.3"), peer("9.9.9.9"), 2),
            ip("2.2.2.2")
        );
        assert_eq!(
            client_ip(&xff("2.2.2.2"), peer("9.9.9.9"), 2),
            ip("9.9.9.9")
        );
    }

    #[test]
    fn client_ip_falls_back_to_peer_without_header_or_on_garbage() {
        assert_eq!(
            client_ip(&HeaderMap::new(), peer("9.9.9.9"), 1),
            ip("9.9.9.9")
        );
        assert_eq!(
            client_ip(&xff("1.1.1.1, not-an-ip"), peer("9.9.9.9"), 1),
            ip("9.9.9.9")
        );
    }
}
