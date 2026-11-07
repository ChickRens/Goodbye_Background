//! プレビューのマスクの出力。

#[derive(Debug)]
pub struct PreviewMaskOutput {
    /// グレースケール 8bit の alpha マスクの PNG（255 = 前景）。大きさは登録したフレームと同じ。
    pub png: Vec<u8>,
}
