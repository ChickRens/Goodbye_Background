//! 1 回の訪問。visit_records テーブルの 1 行に対応する。
//!
//! 値は誰でも送れるので、ここで整えてから DB に入れる:
//! - utm の各値は前後の空白を除いて最大 `MAX_UTM_CHARS` 文字（バイトではなく文字で切る）。空は `None`
//! - `utm_source`・`utm_medium` は集計で揺れないよう小文字にそろえる（campaign・content はそのまま）
//! - `utm_source` が空なら記録しない（`new` が `None`）
//! - `landing_path` は `/` で始まる `MAX_PATH_CHARS` 文字以内のものだけ。それ以外は `/`

use uuid::Uuid;

pub const MAX_UTM_CHARS: usize = 100;
pub const MAX_PATH_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisitEvent {
    client_id: Option<Uuid>,
    utm_source: String,
    utm_medium: Option<String>,
    utm_campaign: Option<String>,
    utm_content: Option<String>,
    landing_path: String,
}

impl VisitEvent {
    pub fn new(
        client_id: Option<Uuid>,
        utm_source: Option<&str>,
        utm_medium: Option<&str>,
        utm_campaign: Option<&str>,
        utm_content: Option<&str>,
        landing_path: Option<&str>,
    ) -> Option<Self> {
        let utm_source = clean(utm_source)?.to_lowercase();
        let landing_path = landing_path
            .filter(|p| p.starts_with('/') && p.chars().count() <= MAX_PATH_CHARS)
            .unwrap_or("/")
            .to_string();
        Some(Self {
            client_id,
            utm_source,
            utm_medium: clean(utm_medium).map(|v| v.to_lowercase()),
            utm_campaign: clean(utm_campaign),
            utm_content: clean(utm_content),
            landing_path,
        })
    }

    pub fn client_id(&self) -> Option<Uuid> {
        self.client_id
    }

    pub fn utm_source(&self) -> &str {
        &self.utm_source
    }

    pub fn utm_medium(&self) -> Option<&str> {
        self.utm_medium.as_deref()
    }

    pub fn utm_campaign(&self) -> Option<&str> {
        self.utm_campaign.as_deref()
    }

    pub fn utm_content(&self) -> Option<&str> {
        self.utm_content.as_deref()
    }

    pub fn landing_path(&self) -> &str {
        &self.landing_path
    }
}

/// 前後の空白を除いて最大 `MAX_UTM_CHARS` 文字にする。空なら `None`。
fn clean(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    (!value.is_empty()).then(|| value.chars().take(MAX_UTM_CHARS).collect())
}

#[cfg(test)]
mod visit_event_test {
    use uuid::Uuid;

    use super::VisitEvent;

    fn source_only(source: Option<&str>) -> Option<VisitEvent> {
        VisitEvent::new(None, source, None, None, None, Some("/"))
    }

    #[test]
    fn trims_and_lowercases_source_and_medium_only() {
        let id = Uuid::new_v4();
        let event = VisitEvent::new(
            Some(id),
            Some(" YouTube "),
            Some("LongForm"),
            Some(" Nov_Launch "),
            Some("Desc"),
            Some("/video.html"),
        )
        .unwrap();
        assert_eq!(event.client_id(), Some(id));
        assert_eq!(event.utm_source(), "youtube");
        assert_eq!(event.utm_medium(), Some("longform"));
        assert_eq!(event.utm_campaign(), Some("Nov_Launch"));
        assert_eq!(event.utm_content(), Some("Desc"));
        assert_eq!(event.landing_path(), "/video.html");
    }

    #[test]
    fn missing_or_blank_source_is_not_recorded() {
        assert_eq!(source_only(None), None);
        assert_eq!(source_only(Some("")), None);
        assert_eq!(source_only(Some("   ")), None);
    }

    #[test]
    fn blank_optional_values_become_none() {
        let event = VisitEvent::new(None, Some("youtube"), Some(" "), Some(""), None, None).unwrap();
        assert_eq!(event.utm_medium(), None);
        assert_eq!(event.utm_campaign(), None);
        assert_eq!(event.utm_content(), None);
    }

    #[test]
    fn long_values_are_cut_by_chars_not_bytes() {
        let ascii = "a".repeat(101);
        assert_eq!(source_only(Some(&ascii)).unwrap().utm_source(), "a".repeat(100));

        // 多バイトの文字の途中で切って panic しないこと
        let emoji = "🎬".repeat(101);
        let event = VisitEvent::new(None, Some("youtube"), None, Some(&emoji), None, None).unwrap();
        assert_eq!(event.utm_campaign(), Some("🎬".repeat(100).as_str()));
    }

    #[test]
    fn invalid_landing_path_falls_back_to_root() {
        let path = |p: Option<&str>| {
            VisitEvent::new(None, Some("youtube"), None, None, None, p)
                .unwrap()
                .landing_path()
                .to_string()
        };
        assert_eq!(path(Some("/app.html")), "/app.html");
        assert_eq!(path(Some("app.html")), "/");
        assert_eq!(path(Some(&format!("/{}", "a".repeat(200)))), "/");
        assert_eq!(path(Some(&format!("/{}", "a".repeat(199)))), format!("/{}", "a".repeat(199)));
        assert_eq!(path(None), "/");
    }
}
