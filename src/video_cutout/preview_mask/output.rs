//! プレビューのマスクの出力。

#[derive(Debug)]
pub struct PreviewMaskOutput {
    /// グレースケール 8bit の alpha マスクの PNG（255 = 前景）。大きさは登録したフレームと同じ。
    pub png: Vec<u8>,
    /// 1 フレーム目に対象が写っているか（`object_score_logits > 0`）。
    pub object_present: bool,
}
