use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    image::{
        image::id::ImageId,
        preview::{
            preview::PreviewImage,
            storage::{error::PreviewStorageError, storage::PreviewStorage},
        },
    },
    maintenance::deleting_expired_entries::{
        clock::clock::AppClock, repository::DeleteExpiredRepository,
    },
    maintenance::ttl_store::TtlStore,
};

#[derive(Debug)]
pub struct PreviewStorageInMemory {
    original_storage: PreviewOriginalStorage,
    segmented_storage: PreviewSegmentedStorage,
}

impl PreviewStorage for PreviewStorageInMemory {
    fn save_as_original(&self, image: PreviewImage) {
        self.original_storage.save(image);
    }

    fn save_as_segmented(&self, image: PreviewImage) {
        self.segmented_storage.save(image);
    }

    fn get(&self, image_id: ImageId) -> Result<Arc<PreviewImage>, PreviewStorageError> {
        let is_original = self.original_storage.contains(image_id);
        let is_segmented = self.segmented_storage.contains(image_id);
        match (is_original, is_segmented) {
            (false, false) => Err(PreviewStorageError::ImageNotFound),
            (false, true) => self
                .segmented_storage
                .get(image_id)
                .ok_or(PreviewStorageError::ImageNotFound),
            (true, false) => self
                .original_storage
                .get(image_id)
                .ok_or(PreviewStorageError::ImageNotFound),
            (true, true) => Err(PreviewStorageError::AmbiguousId),
        }
    }
}

impl PreviewStorageInMemory {
    pub fn new(clock: impl AppClock + Clone + 'static) -> Self {
        Self {
            original_storage: PreviewOriginalStorage {
                image: TtlStore::new(Arc::new(clock.clone())),
            },
            segmented_storage: PreviewSegmentedStorage {
                image: TtlStore::new(Arc::new(clock)),
            },
        }
    }
}

impl DeleteExpiredRepository for PreviewStorageInMemory {
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.original_storage.delete_expired(now, ttl);
        self.segmented_storage.delete_expired(now, ttl);
    }
}

#[derive(Debug)]
struct PreviewOriginalStorage {
    image: TtlStore<ImageId, PreviewImage>,
}

impl PreviewOriginalStorage {
    fn save(&self, original: PreviewImage) {
        let image_id = original.image_id();
        self.image.insert(image_id, original);
    }

    fn get(&self, image_id: ImageId) -> Option<Arc<PreviewImage>> {
        self.image.get(image_id)
    }

    fn contains(&self, image_id: ImageId) -> bool {
        self.image.contains(&image_id)
    }
}

impl DeleteExpiredRepository for PreviewOriginalStorage {
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.image.delete_expired(now, ttl);
    }
}

#[derive(Debug)]
struct PreviewSegmentedStorage {
    image: TtlStore<ImageId, PreviewImage>,
}

impl PreviewSegmentedStorage {
    pub fn save(&self, segmented: PreviewImage) {
        let image_id = segmented.image_id();
        self.image.insert(image_id, segmented);
    }

    fn get(&self, image_id: ImageId) -> Option<Arc<PreviewImage>> {
        self.image.get(image_id)
    }

    fn contains(&self, image_id: ImageId) -> bool {
        self.image.contains(&image_id)
    }
}

impl DeleteExpiredRepository for PreviewSegmentedStorage {
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.image.delete_expired(now, ttl);
    }
}

#[cfg(test)]
mod preview_storage_in_memory_test {
    use std::time::{Duration, Instant};

    use crate::image::preview::storage::storage::PreviewStorage;
    use crate::{
        image::{
            image::{id::ImageId, image::Image, raw_data::ImageData, size::ImageSize},
            preview::{
                preview::PreviewImage,
                storage::{error::PreviewStorageError, storage_impl::PreviewStorageInMemory},
            },
        },
        maintenance::deleting_expired_entries::{
            clock::clock_impl::FakeClock, repository::DeleteExpiredRepository,
        },
    };

    fn make_preview_image() -> PreviewImage {
        let size = ImageSize::new(4, 4).unwrap();
        let image = Image::new(
            ImageData::new(vec![
                255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
            ]),
            ImageId::new(),
            size,
        );
        PreviewImage::new(image)
    }

    #[test]
    fn save_original_and_get_returns_the_saved_image() {
        let now = Instant::now();
        let repo = PreviewStorageInMemory::new(FakeClock::new(now));
        let image = make_preview_image();
        let image_id = image.image_id();

        repo.save_as_original(image.clone());

        let fetched = repo.get(image_id).unwrap();
        assert_eq!(fetched.image_id(), image.image_id());
        assert_eq!(
            fetched.as_ref().clone().into_data().2,
            image.clone().into_data().2
        );
    }

    #[test]
    fn save_segmented_and_get_returns_the_saved_image() {
        let now = Instant::now();
        let repo = PreviewStorageInMemory::new(FakeClock::new(now));
        let image = make_preview_image();
        let image_id = image.image_id();

        repo.save_as_segmented(image.clone());

        let fetched = repo.get(image_id).unwrap();
        assert_eq!(fetched.image_id(), image.image_id());
        assert_eq!(
            fetched.as_ref().clone().into_data().2,
            image.clone().into_data().2
        );
    }

    #[test]
    fn get_missing_image_returns_not_found() {
        let repo = PreviewStorageInMemory::new(FakeClock::new(Instant::now()));

        let result = repo.get(ImageId::new());

        assert!(matches!(result, Err(PreviewStorageError::ImageNotFound)));
    }

    #[test]
    fn same_id_in_original_and_segmented_is_ambiguous() {
        let now = Instant::now();
        let repo = PreviewStorageInMemory::new(FakeClock::new(now));
        let image = make_preview_image();
        let image_id = image.image_id();

        repo.save_as_original(image.clone());
        repo.save_as_segmented(image);

        let result = repo.get(image_id);

        assert!(matches!(result, Err(PreviewStorageError::AmbiguousId)));
    }

    #[test]
    fn delete_expired_removes_old_images() {
        let base = Instant::now();
        let repo = PreviewStorageInMemory::new(FakeClock::new(base));
        let image = make_preview_image();
        let image_id = image.image_id();

        repo.save_as_original(image);
        repo.delete_expired(base + Duration::from_secs(30), Duration::from_secs(10));

        assert!(repo.get(image_id).is_err());
    }
}
