use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    image::{
        image::id::ImageId,
        original::{
            original_image::OriginalImage,
            original_repository::{
                error::OriginalImageRepositoryError, repository::OriginalImageRepository,
            },
        },
    },
    maintenance::{
        deleting_expired_entries::{clock::clock::AppClock, repository::DeleteExpiredRepository},
        ttl_store::TtlStore,
    },
};

pub(crate) struct OriginalImageRepositoryInMemory {
    store: TtlStore<ImageId, OriginalImage>,
}

impl OriginalImageRepositoryInMemory {
    pub(crate) fn new(clock: Arc<dyn AppClock>) -> Self {
        Self {
            store: TtlStore::new(clock),
        }
    }
}

impl OriginalImageRepository for OriginalImageRepositoryInMemory {
    fn save(&self, image: OriginalImage) {
        let id = image.image_id();
        self.store.insert(id, image);
    }

    fn get(&self, image_id: &ImageId) -> Result<Arc<OriginalImage>, OriginalImageRepositoryError> {
        self.store
            .get(*image_id)
            .ok_or(OriginalImageRepositoryError::ImageNotFound)
    }
}

impl DeleteExpiredRepository for OriginalImageRepositoryInMemory {
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.store.delete_expired(now, ttl);
    }
}

#[cfg(test)]
mod image_repository_test {
    use std::time::{Duration, Instant};

    use crate::{
        image::original::original_repository::repository::OriginalImageRepository,
        image::{
            image::{id::ImageId, image::Image, raw_data::ImageData, size::ImageSize},
            original::{
                original_image::OriginalImage,
                original_repository::{
                    error::OriginalImageRepositoryError,
                    repository_impl::OriginalImageRepositoryInMemory,
                },
            },
        },
        maintenance::deleting_expired_entries::{
            clock::clock_impl::FakeClock, repository::DeleteExpiredRepository,
        },
    };

    fn make_image() -> OriginalImage {
        let size = ImageSize::new(4, 4).unwrap();
        let image = Image::new(
            ImageData::new(vec![
                255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
            ]),
            ImageId::new(),
            size,
        );
        OriginalImage::new(image)
    }

    #[test]
    fn save_and_get_returns_the_saved_image() {
        let now = Instant::now();
        let repo = OriginalImageRepositoryInMemory::new(std::sync::Arc::new(FakeClock::new(now)));
        let image = make_image();
        let image_id = image.image_id();

        repo.save(image.clone());

        let fetched = repo.get(&image_id).unwrap();
        assert_eq!(fetched.image_id(), image.image_id());
        assert_eq!(fetched.image().image_size(), image.image().image_size());
    }

    #[test]
    fn get_missing_image_returns_not_found() {
        let repo = OriginalImageRepositoryInMemory::new(std::sync::Arc::new(FakeClock::new(
            Instant::now(),
        )));

        let result = repo.get(&ImageId::new());

        assert!(matches!(
            result,
            Err(OriginalImageRepositoryError::ImageNotFound)
        ));
    }

    #[test]
    fn delete_expired_removes_old_images() {
        let base = Instant::now();
        let repo = OriginalImageRepositoryInMemory::new(std::sync::Arc::new(FakeClock::new(base)));
        let image = make_image();
        let image_id = image.image_id();

        repo.save(image);
        repo.delete_expired(base + Duration::from_secs(30), Duration::from_secs(10));

        assert!(repo.get(&image_id).is_err());
    }
}
