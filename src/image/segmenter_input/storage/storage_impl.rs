use std::time::{Duration, Instant};

use dashmap::{DashMap, mapref::one::RefMut};

use crate::{
    application::{
        interface::{
            clock::AppClock,
            delete_expired_repository::DeleteExpiredRepository,
            segmenter_input_image_storage::{
                error::SegmenterInputImageStorageError, storage::SegmenterInputImageStorage,
            },
        },
        types::{
            entry::{Entry, EntryGuard},
            segmenter_input_image::SegmenterInputImage,
        },
    },
    domain::value_object::image_id::image_id::ImageId,
};

#[derive(Debug)]
pub struct SegmenterInputStorageInMemory<Clock>
where
    Clock: AppClock,
{
    images: DashMap<ImageId, Entry<SegmenterInputImage>>,
    clock: Clock,
}

impl<Clock> SegmenterInputImageStorage for SegmenterInputStorageInMemory<Clock>
where
    Clock: AppClock,
{
    type Guard<'a>
        = EntryGuard<RefMut<'a, ImageId, Entry<SegmenterInputImage>>, SegmenterInputImage>
    where
        Self: 'a;

    fn save(&self, image: SegmenterInputImage) {
        let id = image.image_id().clone();
        let entry = Entry::new(image, self.clock.now());
        self.images.insert(id, entry);
    }

    fn get<'a>(
        &'a self,
        image_id: ImageId,
    ) -> Result<Self::Guard<'a>, SegmenterInputImageStorageError> {
        self.images
            .get_mut(&image_id)
            .map(|entry| EntryGuard::new(entry, self.clock.now()))
            .ok_or(SegmenterInputImageStorageError::ImageNotFound)
    }
}

impl<Clock> DeleteExpiredRepository for SegmenterInputStorageInMemory<Clock>
where
    Clock: AppClock,
{
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.images.retain(|_, entry| !entry.is_expired(now, ttl));
    }
}

impl<Clock> SegmenterInputStorageInMemory<Clock>
where
    Clock: AppClock,
{
    pub fn new(clock: Clock) -> Self {
        Self {
            images: DashMap::new(),
            clock,
        }
    }
}

#[cfg(test)]
mod segmenter_input_image_repository_test {
    use std::time::{Duration, Instant};

    use crate::{
        application::{
            interface::{
                delete_expired_repository::DeleteExpiredRepository,
                segmenter_input_image_storage::{
                    error::SegmenterInputImageStorageError, storage::SegmenterInputImageStorage,
                },
            },
            types::segmenter_input_image::SegmenterInputImage,
        },
        domain::{
            entity::image::Image,
            value_object::{
                image_data::ImageData, image_id::image_id::ImageId,
                image_size::image_size::ImageSize,
            },
        },
        infrastructure::{
            clock::FakeClock,
            storage::segmenter_input_storage_in_memory::SegmenterInputStorageInMemory,
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
