use std::{sync::Arc, time::Instant};

use crate::image::{
    completed::{
        completed_image::CompletedImage,
        generator::generator_impl::CompletedImageGenerator,
        storage::{error::CompletedImageStorageError, storage::CompletedImageRepository},
    },
    image::{id::ImageId, image::Image},
};

pub trait CompletedService: Send + Sync {
    fn generate_and_save(&self, image: &Image);
    fn get(&self, image_id: ImageId) -> Result<CompletedImage, CompletedImageStorageError>;
}

#[derive(Clone)]
pub struct CompletedServiceImpl {
    generator: Arc<dyn CompletedImageGenerator>,
    repository: Arc<dyn CompletedImageRepository>,
}

impl CompletedService for CompletedServiceImpl {
    fn generate_and_save(&self, image: &Image) {
        let start = Instant::now();
        let completed_image = self.generator.generate(image);
        let end = start.elapsed();
        println!("CompletedImage generate: {:?}", end);
        self.repository.save(completed_image);
    }

    fn get(&self, image_id: ImageId) -> Result<CompletedImage, CompletedImageStorageError> {
        let completed = self.repository.get(image_id)?;
        Ok(completed.as_ref().clone())
    }
}

impl CompletedServiceImpl {
    pub fn new(
        completed_image_generator: Arc<dyn CompletedImageGenerator>,
        completed_image_repository: Arc<dyn CompletedImageRepository>,
    ) -> Self {
        Self {
            generator: completed_image_generator,
            repository: completed_image_repository,
        }
    }
}
