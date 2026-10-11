use ndarray::{Array4, ArrayView4, Axis};
use rayon::{
    iter::{IndexedParallelIterator, ParallelIterator},
    slice::ParallelSliceMut,
};

pub struct SAM2MaskResizer;

impl SAM2MaskResizer {
    pub fn resize_mask(mask: &ArrayView4<f32>, target_h: usize, target_w: usize) -> Array4<f32> {
        let (batch, channel, src_h, src_w) = mask.dim();

        debug_assert_eq!(batch, 1);
        debug_assert_eq!(channel, 1);

        let mask = mask.index_axis(Axis(0), 0);
        let mask = mask.index_axis(Axis(0), 0);

        let input = mask
            .as_slice()
            .expect("Resizer requires contiguous ArrayView4");

        let mut output = Array4::<f32>::zeros((1, 1, target_h, target_w));
        let mut output_view = output.index_axis_mut(Axis(0), 0);
        let mut output_view = output_view.index_axis_mut(Axis(0), 0);

        let output_slice = output_view
            .as_slice_mut()
            .expect("Resizer requires contiguous ArrayView4");

        output_slice
            .par_chunks_exact_mut(target_w)
            .enumerate()
            .for_each(|(y, row)| {
                let src_y = (y as f32 + 0.5) * src_h as f32 / target_h as f32 - 0.5;
                let src_y = src_y.clamp(0.0, (src_h - 1) as f32);

                let upper_y = src_y.floor() as usize;
                let lower_y = (upper_y + 1).min(src_h - 1);
                let dy = src_y - upper_y as f32;

                let upper_row = upper_y * src_w;
                let lower_row = lower_y * src_w;

                for x in 0..target_w {
                    let src_x = (x as f32 + 0.5) * src_w as f32 / target_w as f32 - 0.5;
                    let src_x = src_x.clamp(0.0, (src_w - 1) as f32);

                    let left_x = src_x.floor() as usize;
                    let right_x = (left_x + 1).min(src_w - 1);

                    let dx = src_x - left_x as f32;

                    let upper_left = input[upper_row + left_x];
                    let upper_right = input[upper_row + right_x];
                    let lower_left = input[lower_row + left_x];
                    let lower_right = input[lower_row + right_x];

                    row[x] = (1.0 - dx) * (1.0 - dy) * upper_left
                        + dx * (1.0 - dy) * upper_right
                        + (1.0 - dx) * dy * lower_left
                        + dx * dy * lower_right;
                }
            });

        output
    }
}

#[cfg(test)]
mod resizer_test {
    use crate::infrastructure::segmenter::mask_resizer::SAM2MaskResizer;
    use ndarray::Array4;

    const EPSILON: f32 = 1e-5;
    #[test]
    fn test_resize() {
        let mut mask = Array4::<f32>::zeros((1, 1, 2, 2));

        mask[[0, 0, 0, 0]] = 0.0;
        mask[[0, 0, 0, 1]] = 1.1;
        mask[[0, 0, 1, 0]] = 5.01;
        mask[[0, 0, 1, 1]] = 10.002;

        let output = SAM2MaskResizer::resize_mask(&mask.view(), 2, 2);

        for y in 0..2 {
            for x in 0..2 {
                assert!((mask[[0, 0, y, x]] - output[[0, 0, y, x]]).abs() < EPSILON)
            }
        }
    }

    #[test]
    fn test_resized_mask_do_not_contain_invalid_value() {
        let mut mask = Array4::<f32>::zeros((1, 1, 2, 2));

        mask[[0, 0, 0, 0]] = 0.0;
        mask[[0, 0, 0, 1]] = 1.0;
        mask[[0, 0, 1, 0]] = 0.5;
        mask[[0, 0, 1, 1]] = 1.0;

        let output = SAM2MaskResizer::resize_mask(&mask.view(), 100, 100);

        for y in 0..100 {
            for x in 0..100 {
                assert!((0.0..=1.0).contains(&output[[0, 0, y, x]]))
            }
        }
    }

    #[test]
    fn test_constant_mask_remains_constant() {
        let mut mask = Array4::<f32>::zeros((1, 1, 2, 2));

        mask[[0, 0, 0, 0]] = 5.0;
        mask[[0, 0, 0, 1]] = 5.0;
        mask[[0, 0, 1, 0]] = 5.0;
        mask[[0, 0, 1, 1]] = 5.0;

        let output = SAM2MaskResizer::resize_mask(&mask.view(), 300, 400);

        for y in 0..300 {
            for x in 0..400 {
                assert!(((output[[0, 0, y, x]] - 5.0).abs()) < EPSILON)
            }
        }
    }

    #[test]
    fn test_resized_mask_is_expected_shape() {
        let mut mask = Array4::<f32>::zeros((1, 1, 2, 2));

        mask[[0, 0, 0, 0]] = 5.0;
        mask[[0, 0, 0, 1]] = 5.0;
        mask[[0, 0, 1, 0]] = 5.0;
        mask[[0, 0, 1, 1]] = 5.0;

        let output = SAM2MaskResizer::resize_mask(&mask.view(), 3000, 500);

        assert_eq!(output.dim(), (1, 1, 3000, 500))
    }
}
