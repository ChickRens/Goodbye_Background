use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    image::{
        completed::{
            completed_image::CompletedImage,
            storage::{error::CompletedImageStorageError, storage::CompletedImageRepository},
        },
        image::id::ImageId,
    },
    maintenance::deleting_expired_entries::{
        clock::clock::AppClock, repository::DeleteExpiredRepository,
    },
    maintenance::ttl_store::TtlStore,
};

#[derive(Debug)]
pub struct CompletedRepositoryInMemory {
    image: TtlStore<ImageId, CompletedImage>,
}

impl CompletedImageRepository for CompletedRepositoryInMemory {
    fn save(&self, completed_image: CompletedImage) {
        let image_id = completed_image.image_id();
        self.image.insert(image_id, completed_image);
    }

    fn get(&self, image_id: ImageId) -> Result<Arc<CompletedImage>, CompletedImageStorageError> {
        self.image
            .get(image_id)
            .ok_or(CompletedImageStorageError::ImageNotFound)
    }
}

impl DeleteExpiredRepository for CompletedRepositoryInMemory {
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.image.delete_expired(now, ttl);
    }
}

impl CompletedRepositoryInMemory {
    pub fn new(clock: impl AppClock + 'static) -> Self {
        Self {
            image: TtlStore::new(Arc::new(clock)),
        }
    }
}

#[cfg(test)]
mod completed_image_repository_test {
    use std::time::{Duration, Instant};

    use crate::{
        image::{
            completed::{
                completed_image::CompletedImage,
                storage::{
                    error::CompletedImageStorageError, storage::CompletedImageRepository,
                    storage_impl::CompletedRepositoryInMemory,
                },
            },
            image::{id::ImageId, image::Image, raw_data::ImageData, size::ImageSize},
        },
        maintenance::deleting_expired_entries::{
            clock::clock_impl::FakeClock, repository::DeleteExpiredRepository,
        },
    };

    fn make_image() -> CompletedImage {
        let size = ImageSize::new(4, 4).unwrap();
        let image = Image::new(
            ImageData::new(vec![
                255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
            ]),
            ImageId::new(),
            size,
        );
        CompletedImage::new(image)
    }

    #[test]
    fn save_and_get_returns_the_saved_image() {
        let now = Instant::now();
        let repo = CompletedRepositoryInMemory::new(FakeClock::new(now));
        let image = make_image();
        let image_id = image.image_id();

        repo.save(image.clone());

        let fetched = repo.get(image_id).unwrap();
        assert_eq!(fetched.image_id(), image.image_id());
        assert_eq!(
            fetched.as_ref().clone().into_image().image_size(),
            image.clone().into_image().image_size()
        );
    }

    #[test]
    fn get_missing_image_returns_not_found() {
        let repo = CompletedRepositoryInMemory::new(FakeClock::new(Instant::now()));

        let result = repo.get(ImageId::new());

        assert!(matches!(
            result,
            Err(CompletedImageStorageError::ImageNotFound)
        ));
    }

    #[test]
    fn delete_expired_removes_old_images() {
        let base = Instant::now();
        let repo = CompletedRepositoryInMemory::new(FakeClock::new(base));
        let image = make_image();
        let image_id = image.image_id();

        repo.save(image);
        repo.delete_expired(base + Duration::from_secs(30), Duration::from_secs(10));

        assert!(repo.get(image_id).is_err());
    }
}
