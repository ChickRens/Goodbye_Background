//! ワーカーへの依頼（JSON）。キー名は `contracts/video_cutout/worker_job_request.json` と一致させる（仕様 §6.2）。

use serde::Serialize;

use crate::video_cutout::job::point::{NormalizedPoint, PointLabel};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkerJobRequest {
    pub job_id: String,
    /// 入力動画の署名付き GET URL。
    pub input_url: String,
    /// 出力動画の署名付き PUT URL。
    pub output_url: String,
    pub range_ms: WorkerRange,
    /// 1 つ目の prompt の点（古いワーカー向け。古いワーカーは `prompts` を知らずに無視し、これだけ使う）。
    /// 新しいワーカーは `prompts` を使う。両方のデプロイが済んだら送るのをやめる（仕様 §3.2）
    pub points: Vec<WorkerPoint>,
    /// 点を打ったフレーム（1 つ目は開始位置）。
    pub prompts: Vec<WorkerPrompt>,
    pub background_rgb: [u8; 3],
    pub watermark: bool,
    pub max_input_bytes: u64,
    pub callback_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct WorkerRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkerPrompt {
    /// 元の動画の時刻（`range_ms` と同じ基準）。
    pub time_ms: u32,
    pub points: Vec<WorkerPoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct WorkerPoint {
    pub x: f32,
    pub y: f32,
    pub label: WorkerPointLabel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkerPointLabel {
    Include,
    Exclude,
}

impl From<&NormalizedPoint> for WorkerPoint {
    fn from(p: &NormalizedPoint) -> Self {
        let label = match p.label() {
            PointLabel::Include => WorkerPointLabel::Include,
            PointLabel::Exclude => WorkerPointLabel::Exclude,
        };
        Self { x: p.x(), y: p.y(), label }
    }
}

#[cfg(test)]
pub fn sample_request() -> WorkerJobRequest {
    WorkerJobRequest {
        job_id: "6f1c2b9e-0d4a-4c1e-9a57-3f2e8b1d7c60".to_string(),
        input_url: "https://example.com/in".to_string(),
        output_url: "https://example.com/out".to_string(),
        range_ms: WorkerRange { start: 1200, end: 9200 },
        points: vec![WorkerPoint { x: 0.51, y: 0.44, label: WorkerPointLabel::Include }],
        prompts: vec![WorkerPrompt {
            time_ms: 1200,
            points: vec![WorkerPoint { x: 0.51, y: 0.44, label: WorkerPointLabel::Include }],
        }],
        background_rgb: [0, 255, 0],
        watermark: true,
        max_input_bytes: 209_715_200,
        callback_url: "https://example.com/internal/video-jobs/x/events".to_string(),
    }
}

#[cfg(test)]
mod worker_request_test {
    use std::collections::BTreeSet;

    use serde_json::Value;

    use super::*;

    /// オブジェクトのキーを "a.b" の形で再帰的に集める（配列は要素の中まで見る）。
    fn keys_recursive(v: &Value) -> BTreeSet<String> {
        fn walk(v: &Value, prefix: &str, out: &mut BTreeSet<String>) {
            match v {
                Value::Object(m) => {
                    for (k, child) in m {
                        let path = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                        out.insert(path.clone());
                        walk(child, &path, out);
                    }
                }
                Value::Array(items) => {
                    for item in items {
                        walk(item, &format!("{prefix}[]"), out);
                    }
                }
                _ => {}
            }
        }
        let mut out = BTreeSet::new();
        walk(v, "", &mut out);
        out
    }

    #[test]
    fn worker_request_matches_contract_keys() {
        let example: Value = serde_json::from_str(include_str!(
            "../../../contracts/video_cutout/worker_job_request.json"
        ))
        .unwrap();
        let ours = serde_json::to_value(sample_request()).unwrap();
        assert_eq!(keys_recursive(&ours), keys_recursive(&example));
    }

    #[test]
    fn labels_and_background_serialize_like_contract() {
        let ours = serde_json::to_value(sample_request()).unwrap();
        assert_eq!(ours["points"][0]["label"], "include");
        assert_eq!(ours["background_rgb"], serde_json::json!([0, 255, 0]));
        let exclude = WorkerPoint::from(
            &NormalizedPoint::new(0.1, 0.2, PointLabel::Exclude).unwrap(),
        );
        assert_eq!(serde_json::to_value(exclude).unwrap()["label"], "exclude");
    }
}
