use crate::image_io::upload::image_loader::{error::LoadingError, loaded_image::LoadedImage};

pub trait ImageLoader: Send + Sync {
    fn load(&self, data: Vec<u8>) -> Result<LoadedImage, LoadingError>;
}
