use std::time::Instant;

use crate::image::{
    image::{id::ImageId, image::Image},
    preview::{
        generator::generator::PreviewImageGenerator,
        preview::PreviewImage,
        storage::{error::PreviewStorageError, storage::PreviewStorage},
    },
};

pub trait PreviewService {
    fn generate_and_save_original(&self, image: &Image) -> f64;
    fn generate_and_save_segmented(&self, image: &Image);
    fn get(&self, image_id: ImageId) -> Result<PreviewImage, PreviewStorageError>;
}

#[derive(Clone)]
pub struct PreviewServiceImpl<PG, PS>
where
    PG: PreviewImageGenerator,
    PS: PreviewStorage,
{
    generator: PG,
    storage: PS,
}

impl<PG, PS> PreviewService for PreviewServiceImpl<PG, PS>
where
    PG: PreviewImageGenerator,
    PS: PreviewStorage,
{
    fn generate_and_save_original(&self, image: &Image) -> f64 {
        let start = Instant::now();
        let (preview, scale) = self.generator.generate(image);
        let end = start.elapsed();
        println!("Preview Generate: {:?}", end);

        self.storage.save_as_original(preview);
        scale
    }

    fn generate_and_save_segmented(&self, image: &Image) {
        let start = Instant::now();
        let (preview, _) = self.generator.generate(image);
        let end = start.elapsed();
        println!("Preview Generate: {:?}", end);

        self.storage.save_as_segmented(preview);
    }

    fn get(&self, image_id: ImageId) -> Result<PreviewImage, PreviewStorageError> {
        let preview = self.storage.get(image_id)?;
        Ok(preview.as_ref().clone())
    }
}

impl<PG, PS> PreviewServiceImpl<PG, PS>
where
    PG: PreviewImageGenerator,
    PS: PreviewStorage,
{
    pub fn new(preview_generator: PG, preview_storage: PS) -> Self {
        Self {
            generator: preview_generator,
            storage: preview_storage,
        }
    }
}
