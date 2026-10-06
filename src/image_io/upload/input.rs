pub struct UploadInput {
    pub image: Vec<u8>,
}

impl UploadInput {
    pub fn new(image: Vec<u8>) -> Self {
        Self { image: image }
    }

    pub fn into_image_data(self) -> Vec<u8> {
        self.image
    }
}
