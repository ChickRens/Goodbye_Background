//! SAM2 の mask decoder が出すロジットを 0〜1 の確率に変換する（sigmoid）。
//!
//! 変換するのは「元画像サイズにリサイズした後・平滑化/適用の前」。
//! SAM2 公式も低解像度ロジットを補間してから閾値処理するので、それに合わせている。
//!
//! 注意: `SAM2InferenceContext` に保存する「前回マスク」はロジットのまま残すこと
//! （prompt encoder への入力はロジットを想定しているため）。ここで変換するのは表示・保存用の画像だけ。

use ndarray::Array4;
use rayon::{iter::ParallelIterator, slice::ParallelSliceMut};

pub struct SAM2MaskActivation;

impl SAM2MaskActivation {
    /// ロジット → 確率（1 / (1 + e^-x)）を要素ごとに適用する。0 → 0.5、正 → 1 に、負 → 0 に近づく。
    pub fn sigmoid(mut mask: Array4<f32>) -> Array4<f32> {
        mask.as_slice_mut()
            .expect("Activation requires contiguous Array4")
            .par_chunks_mut(4096)
            .for_each(|chunk| {
                for value in chunk {
                    *value = 1.0 / (1.0 + (-*value).exp());
                }
            });
        mask
    }
}

#[cfg(test)]
mod mask_activation_test {
    use ndarray::Array4;

    use super::SAM2MaskActivation;

    #[test]
    fn sigmoid_maps_logits_to_probabilities() {
        let logits = Array4::from_shape_vec((1, 1, 1, 4), vec![0.0, 20.0, -20.0, 2.0]).unwrap();

        let probabilities = SAM2MaskActivation::sigmoid(logits);

        assert!((probabilities[[0, 0, 0, 0]] - 0.5).abs() < 1e-6);
        assert!((probabilities[[0, 0, 0, 1]] - 1.0).abs() < 1e-6);
        assert!(probabilities[[0, 0, 0, 2]].abs() < 1e-6);
        assert!((probabilities[[0, 0, 0, 3]] - 0.880_797).abs() < 1e-5);
    }
}
