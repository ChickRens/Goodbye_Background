use crate::application::{
    interface::image_loader::error::LoadingError, types::loaded_image::LoadedImage,
};

pub trait ImageLoader {
    fn load(&self, data: Vec<u8>) -> Result<LoadedImage, LoadingError>;
}
