use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::{DashMap, mapref::one::RefMut};

use crate::{
    application::{
        interface::{
            clock::AppClock,
            completed_image_repository::{
                error::CompletedImageRepositoryError, repository::CompletedImageRepository,
            },
            delete_expired_repository::DeleteExpiredRepository,
        },
        types::{
            completed_image::CompletedImage,
            entry::{Entry, EntryGuard},
        },
    },
    domain::value_object::image_id::image_id::ImageId,
};

#[derive(Debug)]
pub struct CompletedRepositoryInMemory<Clock>
where
    Clock: AppClock,
{
    image: DashMap<ImageId, Entry<CompletedImage>>,
    clock: Clock,
}

impl<Clock> CompletedImageRepository for CompletedRepositoryInMemory<Clock>
where
    Clock: AppClock,
{
    type Guard<'a>
        = EntryGuard<RefMut<'a, ImageId, Entry<CompletedImage>>, CompletedImage>
    where
        Self: 'a;

    fn save(&self, completed_image: CompletedImage) {
        let image_id = completed_image.image_id();

        let entry = Entry::new(completed_image, self.clock.now());
        self.image.insert(image_id, entry);
    }

    fn get<'a>(
        &'a self,
        image_id: ImageId,
    ) -> Result<Self::Guard<'a>, CompletedImageRepositoryError> {
        self.image
            .get_mut(&image_id)
            .map(|entry| EntryGuard::new(entry, self.clock.now()))
            .ok_or(CompletedImageRepositoryError::ImageNotFound)
    }
}

impl<Clock> DeleteExpiredRepository for CompletedRepositoryInMemory<Clock>
where
    Clock: AppClock,
{
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.image.retain(|_, image| !image.is_expired(now, ttl));
    }
}

impl<Clock> CompletedRepositoryInMemory<Clock>
where
    Clock: AppClock,
{
    pub fn new(clock: Clock) -> Self {
        Self {
            image: DashMap::new(),
            clock,
        }
    }
}

#[cfg(test)]
mod completed_image_repository_test {
    use std::time::{Duration, Instant};

    use crate::{
        application::{
            interface::{
                completed_image_repository::{
                    error::CompletedImageRepositoryError, repository::CompletedImageRepository,
                },
                delete_expired_repository::DeleteExpiredRepository,
            },
            types::completed_image::CompletedImage,
        },
        domain::{
            entity::image::Image,
            value_object::{
                image_data::ImageData, image_id::image_id::ImageId,
                image_size::image_size::ImageSize,
            },
        },
        infrastructure::{
            clock::FakeClock, repository::completed_repository::CompletedRepositoryInMemory,
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
            fetched.clone().into_image().image_size(),
            image.clone().into_image().image_size()
        );
    }

    #[test]
    fn get_missing_image_returns_not_found() {
        let repo = CompletedRepositoryInMemory::new(FakeClock::new(Instant::now()));

        let result = repo.get(ImageId::new());

        assert!(matches!(
            result,
            Err(CompletedImageRepositoryError::ImageNotFound)
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

#[derive(Debug, Clone)]
pub struct SharedCompletedImageRepository<Clock>
where
    Clock: AppClock,
{
    repository: Arc<CompletedRepositoryInMemory<Clock>>,
}

impl<Clock> CompletedImageRepository for SharedCompletedImageRepository<Clock>
where
    Clock: AppClock,
{
    type Guard<'a>
        = EntryGuard<RefMut<'a, ImageId, Entry<CompletedImage>>, CompletedImage>
    where
        Self: 'a;

    fn get<'a>(
        &'a self,
        image_id: ImageId,
    ) -> Result<Self::Guard<'a>, CompletedImageRepositoryError> {
        self.repository.get(image_id)
    }

    fn save(&self, completed_image: CompletedImage) {
        self.repository.save(completed_image);
    }
}

impl<Clock> DeleteExpiredRepository for SharedCompletedImageRepository<Clock>
where
    Clock: AppClock,
{
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.repository.delete_expired(now, ttl);
    }
}

impl<Clock> SharedCompletedImageRepository<Clock>
where
    Clock: AppClock,
{
    pub fn new(clock: Clock) -> Self {
        Self {
            repository: Arc::new(CompletedRepositoryInMemory::new(clock)),
        }
    }
}
