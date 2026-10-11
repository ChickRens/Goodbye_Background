#[derive(Clone)]
pub enum UsageStatus {
    Success,
    Failed(String),
}
