use std::{ops::Deref, sync::Arc, time::Instant};

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
        let guard = self.storage.get(image_id)?;
        Ok(guard.deref().clone())
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

#[derive(Clone)]
pub struct SharedPreviewService<PG, PS>
where
    PG: PreviewImageGenerator,
    PS: PreviewStorage,
{
    service: Arc<PreviewServiceImpl<PG, PS>>,
}

impl<PG, PS> PreviewService for SharedPreviewService<PG, PS>
where
    PG: PreviewImageGenerator,
    PS: PreviewStorage,
{
    fn generate_and_save_original(&self, image: &Image) -> f64 {
        self.service.generate_and_save_original(image)
    }

    fn generate_and_save_segmented(&self, image: &Image) {
        self.service.generate_and_save_segmented(image);
    }

    fn get(&self, image_id: ImageId) -> Result<PreviewImage, PreviewStorageError> {
        self.service.get(image_id)
    }
}

impl<PG, PS> SharedPreviewService<PG, PS>
where
    PG: PreviewImageGenerator,
    PS: PreviewStorage,
{
    pub fn new(preview_generator: PG, preview_storage: PS) -> Self {
        Self {
            service: Arc::new(PreviewServiceImpl::new(preview_generator, preview_storage)),
        }
    }
}
