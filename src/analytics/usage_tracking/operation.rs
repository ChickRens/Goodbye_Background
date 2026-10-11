pub enum UsageOperation {
    Upload,
    Segment,
    Undo,
    Redo,
    Save,
    GetCompleted,
    GetPreview,
    Unknown(String),
}

impl UsageOperation {
    pub fn new(operation: &str) -> Self {
        match operation {
            "upload" => Self::Upload,
            "segment" => Self::Segment,
            "save" => Self::Save,
            "undo" => Self::Undo,
            "redo" => Self::Redo,
            "get-completed" => Self::GetCompleted,
            "get-preview" => Self::GetPreview,
            unknown => Self::Unknown(unknown.to_string()),
        }
    }
}
