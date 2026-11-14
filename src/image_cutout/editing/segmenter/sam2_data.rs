//! SAM2 の ONNX モデル間を受け渡すテンソルの型付きラッパー（中身は ndarray）。
//!
//! | 型 | 形状 | 生成元 → 使用先 |
//! |---|---|---|
//! | `ImageEmbeddings` | [1, 256, 64, 64] | image encoder → mask decoder |
//! | `HighResFeatureS0` | [1, 32, 256, 256] | image encoder → mask decoder |
//! | `HighResFeatureS1` | [1, 64, 128, 128] | image encoder → mask decoder |
//! | `SparseEmbeddings` | [1, N+1, 256] | prompt encoder → mask decoder（ポイント由来。N=点数、box 無しなのでパディング点が1つ付く） |
//! | `DenseEmbeddings` | [1, 256, 64, 64] | prompt encoder → mask decoder（前回マスク由来） |
//! | `Mask` | [1, 1, 256, 256] | mask decoder の出力。**ロジット**（sigmoid 前の値。0 が境界） |
//!
//! `SAM2StaticContext` = 画像特徴量3つ（f32 で合計約16MB）。セッション中不変。
//! `SAM2InferenceContext` = 直前のマスク（初回は None）。次の推論で prompt encoder に渡し、精度を上げる。

use ndarray::{Array3, Array4, ArrayBase, ArrayView3, ArrayView4, Dim, OwnedRepr};

#[derive(Debug, PartialEq, Clone, Default)]
pub struct ImageEmbeddings {
    value: Array4<f32>,
}

impl ImageEmbeddings {
    pub fn new(value: Array4<f32>) -> Self {
        Self { value }
    }

    pub fn view(&self) -> ArrayView4<f32> {
        self.value.view()
    }
}

#[derive(Debug, PartialEq, Clone, Default)]
pub struct HighResFeatureS0 {
    value: Array4<f32>,
}

impl HighResFeatureS0 {
    pub fn new(value: Array4<f32>) -> Self {
        Self { value }
    }

    pub fn view(&self) -> ArrayView4<f32> {
        self.value.view()
    }
}

#[derive(Debug, PartialEq, Clone, Default)]
pub struct HighResFeatureS1 {
    value: Array4<f32>,
}

impl HighResFeatureS1 {
    pub fn new(value: Array4<f32>) -> Self {
        Self { value }
    }

    pub fn view(&self) -> ArrayView4<f32> {
        self.value.view()
    }
}

#[derive(Debug, PartialEq, Clone, Default)]
pub struct SAM2StaticContext {
    image_embeddings: ImageEmbeddings,
    high_res_feature_s0: HighResFeatureS0,
    high_res_feature_s1: HighResFeatureS1,
}

impl SAM2StaticContext {
    pub fn new(
        image_embeddings: ImageEmbeddings,
        high_res_feature_s0: HighResFeatureS0,
        high_res_feature_s1: HighResFeatureS1,
    ) -> Self {
        Self {
            image_embeddings: image_embeddings,
            high_res_feature_s0: high_res_feature_s0,
            high_res_feature_s1: high_res_feature_s1,
        }
    }

    pub fn get_all_context_refs(&self) -> (&ImageEmbeddings, &HighResFeatureS0, &HighResFeatureS1) {
        (
            &self.image_embeddings,
            &self.high_res_feature_s0,
            &self.high_res_feature_s1,
        )
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct SparseEmbeddings {
    value: Array3<f32>,
}

impl SparseEmbeddings {
    pub fn new(value: Array3<f32>) -> Self {
        Self { value }
    }

    pub fn view(&self) -> ArrayView3<f32> {
        self.value.view()
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct DenseEmbeddings {
    value: Array4<f32>,
}

impl DenseEmbeddings {
    pub fn new(value: Array4<f32>) -> Self {
        Self { value }
    }

    pub fn view(&self) -> ArrayView4<f32> {
        self.value.view()
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct Mask {
    value: Array4<f32>,
}

impl Mask {
    pub fn new(value: Array4<f32>) -> Self {
        Self { value }
    }

    pub fn into_mask(self) -> ArrayBase<OwnedRepr<f32>, Dim<[usize; 4]>> {
        self.value
    }

    pub fn view(&self) -> ArrayView4<f32> {
        self.value.view()
    }
}

#[derive(Debug, PartialEq, Clone, Default)]
pub struct SAM2InferenceContext {
    masks: Option<Mask>,
}

impl SAM2InferenceContext {
    pub fn new(masks: Option<Mask>) -> Self {
        Self { masks }
    }

    pub fn get_all_context_refs(&self) -> &Option<Mask> {
        &self.masks
    }

    pub fn set_mask(&mut self, mask: Mask) {
        self.masks = Some(mask)
    }
}
