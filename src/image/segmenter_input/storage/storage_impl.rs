use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    image::{
        image::id::ImageId,
        segmenter_input::{
            segmenter_input_image::SegmenterInputImage,
            storage::{
                error::SegmenterInputImageStorageError, storage::SegmenterInputImageStorage,
            },
        },
    },
    maintenance::deleting_expired_entries::{
        clock::clock::AppClock, repository::DeleteExpiredRepository,
    },
    maintenance::ttl_store::TtlStore,
};

#[derive(Debug)]
pub struct SegmenterInputStorageInMemory {
    images: TtlStore<ImageId, SegmenterInputImage>,
}

impl SegmenterInputImageStorage for SegmenterInputStorageInMemory {
    fn save(&self, image: SegmenterInputImage) {
        let id = image.image_id().clone();
        self.images.insert(id, image);
    }

    fn get(
        &self,
        image_id: ImageId,
    ) -> Result<Arc<SegmenterInputImage>, SegmenterInputImageStorageError> {
        self.images
            .get(image_id)
            .ok_or(SegmenterInputImageStorageError::ImageNotFound)
    }
}

impl DeleteExpiredRepository for SegmenterInputStorageInMemory {
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.images.delete_expired(now, ttl);
    }
}

impl SegmenterInputStorageInMemory {
    pub fn new(clock: impl AppClock + 'static) -> Self {
        Self {
            images: TtlStore::new(Arc::new(clock)),
        }
    }
}

#[cfg(test)]
mod segmenter_input_image_repository_test {
    use std::time::{Duration, Instant};

    use crate::{
        image::{
            image::{id::ImageId, image::Image, raw_data::ImageData, size::ImageSize},
            segmenter_input::{
                segmenter_input_image::SegmenterInputImage,
                storage::{
                    error::SegmenterInputImageStorageError, storage::SegmenterInputImageStorage,
                    storage_impl::SegmenterInputStorageInMemory,
                },
            },
        },
        maintenance::deleting_expired_entries::{
            clock::clock_impl::FakeClock, repository::DeleteExpiredRepository,
        },
    };

    fn make_image() -> SegmenterInputImage {
        let size = ImageSize::new(4, 4).unwrap();
        let image = Image::new(
            ImageData::new(vec![
                255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
            ]),
            ImageId::new(),
            size,
        );
        SegmenterInputImage::new(image)
    }

    #[test]
    fn save_and_get_returns_the_saved_image() {
        let now = Instant::now();
        let repo = SegmenterInputStorageInMemory::new(FakeClock::new(now));
        let image = make_image();
        let image_id = image.image_id();

        repo.save(image.clone());

        let fetched = repo.get(*image_id).unwrap();
        assert_eq!(fetched.image_id(), image.image_id());
        assert_eq!(fetched.image().image_size(), image.image().image_size());
    }

    #[test]
    fn get_missing_image_returns_not_found() {
        let repo = SegmenterInputStorageInMemory::new(FakeClock::new(Instant::now()));

        let result = repo.get(ImageId::new());

        assert!(matches!(
            result,
            Err(SegmenterInputImageStorageError::ImageNotFound)
        ));
    }

    #[test]
    fn delete_expired_removes_old_images() {
        let base = Instant::now();
        let repo = SegmenterInputStorageInMemory::new(FakeClock::new(base));
        let image = make_image();
        repo.save(image.clone());
        let image_id = image.image_id();

        repo.delete_expired(base + Duration::from_secs(30), Duration::from_secs(10));

        assert!(repo.get(*image_id).is_err());
    }
}
