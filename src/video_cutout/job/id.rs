//! 動画ジョブ ID（UUID v4）。

use uuid::Uuid;

use crate::video_cutout::job::error::VideoJobError;

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub struct VideoJobId(Uuid);

impl VideoJobId {
    pub fn new_random() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_str(id: &str) -> Result<Self, VideoJobError> {
        Uuid::parse_str(id)
            .map(Self)
            .map_err(|_| VideoJobError::InvalidId)
    }

    pub fn value(&self) -> &Uuid {
        &self.0
    }
}

#[cfg(test)]
mod video_job_id_test {
    use super::*;

    #[test]
    fn parses_valid_uuid() {
        let id = VideoJobId::from_str("1754ca13-617a-46be-bad2-a83eaedccb77").unwrap();
        assert_eq!(id.value().to_string(), "1754ca13-617a-46be-bad2-a83eaedccb77");
    }

    #[test]
    fn rejects_invalid_str() {
        assert_eq!(VideoJobId::from_str("nope"), Err(VideoJobError::InvalidId));
    }

    #[test]
    fn random_ids_differ() {
        assert_ne!(VideoJobId::new_random(), VideoJobId::new_random());
    }
}
