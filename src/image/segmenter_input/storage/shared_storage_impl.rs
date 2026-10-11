use std::sync::Arc;

use dashmap::mapref::one::RefMut;

use crate::{
    image::{
        image::id::ImageId,
        segmenter_input::{
            segmenter_input_image::SegmenterInputImage,
            storage::{
                error::SegmenterInputImageStorageError, storage::SegmenterInputImageStorage,
                storage_impl::SegmenterInputStorageInMemory,
            },
        },
    },
    maintenance::deleting_expired_entries::{
        clock::clock::AppClock,
        entry::{Entry, EntryGuard},
        repository::DeleteExpiredRepository,
    },
};

#[derive(Debug, Clone)]
pub struct SharedSegmenterInputStorage<Clock>
where
    Clock: AppClock,
{
    storage: Arc<SegmenterInputStorageInMemory<Clock>>,
}

impl<Clock> SegmenterInputImageStorage for SharedSegmenterInputStorage<Clock>
where
    Clock: AppClock,
{
    type Guard<'a>
        = EntryGuard<RefMut<'a, ImageId, Entry<SegmenterInputImage>>, SegmenterInputImage>
    where
        Self: 'a;

    fn save(&self, image: SegmenterInputImage) {
        self.storage.save(image);
    }

    fn get<'a>(
        &'a self,
        image_id: ImageId,
    ) -> Result<Self::Guard<'a>, SegmenterInputImageStorageError> {
        self.storage.get(image_id)
    }
}

impl<Clock> DeleteExpiredRepository for SharedSegmenterInputStorage<Clock>
where
    Clock: AppClock,
{
    fn delete_expired(&self, now: std::time::Instant, ttl: std::time::Duration) {
        self.storage.delete_expired(now, ttl);
    }
}

impl<Clock> SharedSegmenterInputStorage<Clock>
where
    Clock: AppClock,
{
    pub fn new(clock: Clock) -> Self {
        Self {
            storage: Arc::new(SegmenterInputStorageInMemory::new(clock)),
        }
    }
}
