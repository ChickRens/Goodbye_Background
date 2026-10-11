use crate::domain::entity::image::Image;

#[derive(Debug, PartialEq, Eq)]
pub struct LoadedImage {
    image: Image,
}

impl LoadedImage {
    pub fn new(image: Image) -> Self {
        Self { image }
    }

    pub fn into_image(self) -> Image {
        self.image
    }
}
