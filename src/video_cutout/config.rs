//! 動画切り抜きの設定（環境変数。仕様 §4.7）。
//!
//! `TRUSTED_PROXY_HOPS`・`LOG_CLIENT_IP_DEBUG`（どちらも任意）以外が 1 つでも欠けて（空文字も欠けている扱い）いたら `None` を返し、
//! 動画のルートは 503 `VIDEO_CUTOUT_DISABLED` になる（画像機能はそのまま動く）。

pub struct VideoCutoutConfig {
    pub r2_account_id: String,
    pub r2_bucket: String,
    pub r2_access_key_id: String,
    pub r2_secret_access_key: String,
    pub modal_endpoint_url: String,
    pub modal_key: String,
    pub modal_secret: String,
    /// ワーカーからの通知の HMAC 鍵（Modal 側の Secret と同じ値）。
    pub worker_callback_secret: String,
    /// ワーカーが通知を送る先のオリジン（例: `https://example.com`）。
    pub public_base_url: String,
    /// `X-Forwarded-For` を右から何番目まで信頼するか（`presentation::client_ip`）。既定 0 = 接続元を使う。
    pub trusted_proxy_hops: usize,
    /// 一時的な確認用。`LOG_CLIENT_IP_DEBUG` がちょうど `"1"` のときだけ、`POST /video-jobs` で
    /// IP の取り方を 1 行ログに出す（`presentation::client_ip::client_ip_debug_line`）。任意（無くても動画は有効）。
    /// `TRUSTED_PROXY_HOPS` の確認が終わったら外す。
    pub log_client_ip_debug: bool,
}

impl VideoCutoutConfig {
    pub fn from_env() -> Option<Self> {
        Self::from_vars(|name| std::env::var(name).ok())
    }

    /// `var(name)` で値を引く。テストで環境変数を触らずに済むように分けてある。
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Option<Self> {
        // 前後の空白・改行は落とす（Secret Manager などに改行付きで入れても署名が合うように）
        let required = |name: &str| {
            var(name)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let trusted_proxy_hops = match var("TRUSTED_PROXY_HOPS").filter(|v| !v.trim().is_empty()) {
            None => 0,
            Some(v) => match v.trim().parse() {
                Ok(n) => n,
                Err(_) => {
                    // 黙って 0 にすると Cloud Run で全員が同じ IP になるので、動画ごと無効にする
                    eprintln!(
                        "TRUSTED_PROXY_HOPS が数値ではないため、動画切り抜きを無効にします: {v:?}"
                    );
                    return None;
                }
            },
        };
        Some(Self {
            r2_account_id: required("R2_ACCOUNT_ID")?,
            r2_bucket: required("R2_BUCKET")?,
            r2_access_key_id: required("R2_ACCESS_KEY_ID")?,
            r2_secret_access_key: required("R2_SECRET_ACCESS_KEY")?,
            modal_endpoint_url: required("MODAL_ENDPOINT_URL")?,
            modal_key: required("MODAL_KEY")?,
            modal_secret: required("MODAL_SECRET")?,
            worker_callback_secret: required("WORKER_CALLBACK_SECRET")?,
            public_base_url: required("PUBLIC_BASE_URL")?,
            trusted_proxy_hops,
            log_client_ip_debug: var("LOG_CLIENT_IP_DEBUG").as_deref() == Some("1"),
        })
    }
}

#[cfg(test)]
mod config_test {
    use std::collections::HashMap;

    use super::VideoCutoutConfig;

    fn all_vars() -> HashMap<&'static str, String> {
        [
            "R2_ACCOUNT_ID",
            "R2_BUCKET",
            "R2_ACCESS_KEY_ID",
            "R2_SECRET_ACCESS_KEY",
            "MODAL_ENDPOINT_URL",
            "MODAL_KEY",
            "MODAL_SECRET",
            "WORKER_CALLBACK_SECRET",
            "PUBLIC_BASE_URL",
        ]
        .into_iter()
        .map(|k| (k, format!("value-of-{k}")))
        .collect()
    }

    fn vars_without(name: &str) -> impl Fn(&str) -> Option<String> {
        let mut vars = all_vars();
        vars.remove(name);
        move |k| vars.get(k).cloned()
    }

    #[test]
    fn config_is_none_when_any_var_missing() {
        assert!(VideoCutoutConfig::from_vars(vars_without("R2_BUCKET")).is_none());
        assert!(VideoCutoutConfig::from_vars(vars_without("PUBLIC_BASE_URL")).is_none());
    }

    #[test]
    fn config_treats_empty_value_as_missing() {
        let mut vars = all_vars();
        vars.insert("MODAL_KEY", String::new());
        assert!(VideoCutoutConfig::from_vars(move |k| vars.get(k).cloned()).is_none());
    }

    #[test]
    fn config_reads_all_vars_and_defaults_hops_to_zero() {
        let config = VideoCutoutConfig::from_vars(vars_without("TRUSTED_PROXY_HOPS")).unwrap();
        assert_eq!(config.r2_bucket, "value-of-R2_BUCKET");
        assert_eq!(
            config.worker_callback_secret,
            "value-of-WORKER_CALLBACK_SECRET"
        );
        assert_eq!(config.public_base_url, "value-of-PUBLIC_BASE_URL");
        assert_eq!(config.trusted_proxy_hops, 0);
    }

    #[test]
    fn config_reads_trusted_proxy_hops() {
        let mut vars = all_vars();
        vars.insert("TRUSTED_PROXY_HOPS", "1".to_string());
        let config = VideoCutoutConfig::from_vars(move |k| vars.get(k).cloned()).unwrap();
        assert_eq!(config.trusted_proxy_hops, 1);
    }

    #[test]
    fn config_is_none_when_hops_is_not_a_number() {
        let mut vars = all_vars();
        vars.insert("TRUSTED_PROXY_HOPS", "one".to_string());
        assert!(VideoCutoutConfig::from_vars(move |k| vars.get(k).cloned()).is_none());
    }

    #[test]
    fn config_enables_client_ip_debug_only_with_exactly_one() {
        for (value, expected) in [
            (None, false),
            (Some("1"), true),
            (Some("true"), false),
            (Some(" 1"), false),
            (Some("0"), false),
        ] {
            let mut vars = all_vars();
            if let Some(v) = value {
                vars.insert("LOG_CLIENT_IP_DEBUG", v.to_string());
            }
            let config = VideoCutoutConfig::from_vars(move |k| vars.get(k).cloned()).unwrap();
            assert_eq!(config.log_client_ip_debug, expected, "{value:?}");
        }
    }

    #[test]
    fn config_trims_whitespace_and_newlines_from_values() {
        let mut vars = all_vars();
        vars.insert("WORKER_CALLBACK_SECRET", "secret-value\n".to_string());
        vars.insert("R2_SECRET_ACCESS_KEY", " r2-secret\r\n".to_string());
        let config = VideoCutoutConfig::from_vars(move |k| vars.get(k).cloned()).unwrap();
        assert_eq!(config.worker_callback_secret, "secret-value");
        assert_eq!(config.r2_secret_access_key, "r2-secret");
    }
}
