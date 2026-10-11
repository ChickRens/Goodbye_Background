use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::{DashMap, mapref::one::RefMut};

use crate::{application::{interface::{clock::AppClock, delete_expired_repository::DeleteExpiredRepository}, types::entry::{Entry, EntryGuard}}, image::id::ImageId, preview::{preview::PreviewImage, storage::{error::PreviewStorageError, storage::PreviewStorage}}};

#[derive(Debug)]
pub struct PreviewStorageInMemory<Clock>
where
    Clock: AppClock,
{
    original_storage: PreviewOriginalStorage<Clock>,
    segmented_storage: PreviewSegmentedStorage<Clock>,
}

impl<Clock> PreviewStorage for PreviewStorageInMemory<Clock>
where
    Clock: AppClock,
{
    type Guard<'a>
        = PreviewGuard<'a>
    where
        Self: 'a;

    fn save_as_original(&self, image: PreviewImage) {
        self.original_storage.save(image);
    }

    fn save_as_segmented(&self, image: PreviewImage) {
        self.segmented_storage.save(image);
    }

    fn get<'a>(&'a self, image_id: ImageId) -> Result<Self::Guard<'a>, PreviewStorageError> {
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

impl<Clock> PreviewStorageInMemory<Clock>
where
    Clock: AppClock + Clone,
{
    pub fn new(clock: Clock) -> Self {
        Self {
            original_storage: PreviewOriginalStorage {
                image: DashMap::new(),
                clock: clock.clone(),
            },
            segmented_storage: PreviewSegmentedStorage {
                image: DashMap::new(),
                clock: clock.clone(),
            },
        }
    }
}

impl<Clock> DeleteExpiredRepository for PreviewStorageInMemory<Clock>
where
    Clock: AppClock,
{
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.original_storage.delete_expired(now, ttl);
        self.segmented_storage.delete_expired(now, ttl);
    }
}

pub enum PreviewGuard<'a> {
    Original(EntryGuard<RefMut<'a, ImageId, Entry<PreviewImage>>, PreviewImage>),
    Segmented(EntryGuard<RefMut<'a, ImageId, Entry<PreviewImage>>, PreviewImage>),
}

impl<'a> Deref for PreviewGuard<'a> {
    type Target = PreviewImage;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Original(guard) => guard.deref(),
            Self::Segmented(guard) => guard.deref(),
        }
    }
}

impl<'a> DerefMut for PreviewGuard<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Original(guard) => guard.deref_mut(),
            Self::Segmented(guard) => guard.deref_mut(),
        }
    }
}

#[derive(Debug)]
struct PreviewOriginalStorage<Clock>
where
    Clock: AppClock,
{
    image: DashMap<ImageId, Entry<PreviewImage>>,
    clock: Clock,
}

impl<Clock> PreviewOriginalStorage<Clock>
where
    Clock: AppClock,
{
    fn save(&self, original: PreviewImage) {
        let image_id = original.image_id();
        let entry = Entry::new(original, self.clock.now());
        self.image.insert(image_id, entry);
    }

    fn get(&self, image_id: ImageId) -> Option<PreviewGuard<'_>> {
        self.image
            .get_mut(&image_id)
            .map(|entry| EntryGuard::new(entry, self.clock.now()))
            .map(|guard| PreviewGuard::Original(guard))
    }

    fn contains(&self, image_id: ImageId) -> bool {
        self.image.contains_key(&image_id)
    }

    fn new(clock: Clock) -> Self {
        Self {
            image: DashMap::new(),
            clock,
        }
    }
}

impl<Clock> DeleteExpiredRepository for PreviewOriginalStorage<Clock>
where
    Clock: AppClock,
{
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.image.retain(|_, entry| !entry.is_expired(now, ttl));
    }
}

#[derive(Debug)]
struct PreviewSegmentedStorage<Clock>
where
    Clock: AppClock,
{
    image: DashMap<ImageId, Entry<PreviewImage>>,
    clock: Clock,
}

impl<Clock> PreviewSegmentedStorage<Clock>
where
    Clock: AppClock,
{
    pub fn save(&self, segmented: PreviewImage) {
        let image_id = segmented.image_id();
        let entry = Entry::new(segmented, self.clock.now());
        self.image.insert(image_id, entry);
    }

    fn get<'a>(&'a self, image_id: ImageId) -> Option<PreviewGuard<'a>> {
        self.image
            .get_mut(&image_id)
            .map(|entry| EntryGuard::new(entry, self.clock.now()))
            .map(|guard| PreviewGuard::Segmented(guard))
    }

    fn contains(&self, image_id: ImageId) -> bool {
        self.image.contains_key(&image_id)
    }
}

impl<Clock> DeleteExpiredRepository for PreviewSegmentedStorage<Clock>
where
    Clock: AppClock,
{
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.image.retain(|_, entry| !entry.is_expired(now, ttl));
    }
}

#[derive(Debug, Clone)]
pub struct SharedPreviewStorage<Clock>
where
    Clock: AppClock,
{
    storage: Arc<PreviewStorageInMemory<Clock>>,
}

impl<Clock> PreviewStorage for SharedPreviewStorage<Clock>
where
    Clock: AppClock,
{
    type Guard<'a>
        = PreviewGuard<'a>
    where
        Self: 'a;

    fn save_as_original(&self, original: PreviewImage) {
        self.storage.save_as_original(original);
    }

    fn save_as_segmented(&self, segmented: PreviewImage) {
        self.storage.save_as_segmented(segmented);
    }

    fn get<'a>(&'a self, image_id: ImageId) -> Result<Self::Guard<'a>, PreviewStorageError> {
        self.storage.get(image_id)
    }
}

impl<Clock> DeleteExpiredRepository for SharedPreviewStorage<Clock>
where
    Clock: AppClock,
{
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.storage.delete_expired(now, ttl);
    }
}

impl<Clock> SharedPreviewStorage<Clock>
where
    Clock: AppClock + Clone,
{
    pub fn new(clock: Clock) -> Self {
        Self {
            storage: Arc::new(PreviewStorageInMemory::new(clock)),
        }
    }
}

#[cfg(test)]
mod preview_storage_in_memory_test {
    use std::time::{Duration, Instant};

use crate::{image::{id::ImageId, image::Image, raw_data::ImageData, size::ImageSize}, infrastructure::clock::FakeClock, preview::{preview::PreviewImage, storage::{error::PreviewStorageError, storage_impl::PreviewStorageInMemory}}};

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
        assert_eq!(fetched.clone().into_data().2, image.clone().into_data().2);
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
        assert_eq!(fetched.clone().into_data().2, image.clone().into_data().2);
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
