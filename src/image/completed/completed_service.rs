use std::time::Instant;

use crate::image::{
    completed::{
        completed_image::CompletedImage,
        generator::generator_impl::CompletedImageGenerator,
        storage::{error::CompletedImageStorageError, storage::CompletedImageRepository},
    },
    image::{id::ImageId, image::Image},
};

pub trait CompletedService {
    fn generate_and_save(&self, image: &Image);
    fn get(&self, image_id: ImageId) -> Result<CompletedImage, CompletedImageStorageError>;
}

#[derive(Debug, Clone)]
pub struct CompletedServiceImpl<CG, CR>
where
    CG: CompletedImageGenerator,
    CR: CompletedImageRepository,
{
    generator: CG,
    repository: CR,
}

impl<CG, CR> CompletedService for CompletedServiceImpl<CG, CR>
where
    CG: CompletedImageGenerator,
    CR: CompletedImageRepository,
{
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

impl<CG, CR> CompletedServiceImpl<CG, CR>
where
    CG: CompletedImageGenerator,
    CR: CompletedImageRepository,
{
    pub fn new(completed_image_generator: CG, completed_image_repository: CR) -> Self {
        Self {
            generator: completed_image_generator,
            repository: completed_image_repository,
        }
    }
}
