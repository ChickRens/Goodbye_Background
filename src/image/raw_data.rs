#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ImageData {
    image: Vec<u8>,
}

impl ImageData {
    pub fn new(image: Vec<u8>) -> Self {
        Self { image }
    }

    pub fn image(&self) -> &Vec<u8> {
        &self.image
    }

    pub fn into_image(self) -> Vec<u8> {
        self.image
    }
}
