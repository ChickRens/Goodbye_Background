use std::{ops::Deref, sync::Arc, time::Instant};

use crate::{
    application::{
        interface::{
            completed_image_generator::CompletedImageGenerator,
            completed_image_repository::{
                error::CompletedImageRepositoryError, repository::CompletedImageRepository,
            },
        },
        types::completed_image::CompletedImage,
    },
    domain::{entity::image::Image, value_object::image_id::image_id::ImageId},
};

pub trait CompletedService {
    fn generate_and_save(&self, image: &Image);
    fn get(&self, image_id: ImageId) -> Result<CompletedImage, CompletedImageRepositoryError>;
}

#[derive(Debug)]
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

    fn get(&self, image_id: ImageId) -> Result<CompletedImage, CompletedImageRepositoryError> {
        let guard = self.repository.get(image_id)?;
        Ok(guard.deref().clone())
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

#[derive(Debug, Clone)]
pub struct SharedCompletedService<CG, CR>
where
    CG: CompletedImageGenerator,
    CR: CompletedImageRepository,
{
    service: Arc<CompletedServiceImpl<CG, CR>>,
}

impl<CG, CR> CompletedService for SharedCompletedService<CG, CR>
where
    CG: CompletedImageGenerator,
    CR: CompletedImageRepository,
{
    fn generate_and_save(&self, image: &Image) {
        self.service.generate_and_save(image);
    }

    fn get(&self, image_id: ImageId) -> Result<CompletedImage, CompletedImageRepositoryError> {
        self.service.get(image_id)
    }
}

impl<CG, CR> SharedCompletedService<CG, CR>
where
    CG: CompletedImageGenerator,
    CR: CompletedImageRepository,
{
    pub fn new(completed_image_generator: CG, completed_image_repository: CR) -> Self {
        Self {
            service: Arc::new(CompletedServiceImpl::new(
                completed_image_generator,
                completed_image_repository,
            )),
        }
    }
}
