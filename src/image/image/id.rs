use uuid::Uuid;

use crate::image::image::error::ImageIdError;

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub struct ImageId {
    id: Uuid,
}

impl ImageId {
    pub fn new() -> Self {
        Self { id: Uuid::new_v4() }
    }

    pub fn from_uuid(id: Uuid) -> Self {
        Self { id: id }
    }

    pub fn from_str(id: &str) -> Result<Self, ImageIdError> {
        let id_result = Uuid::parse_str(id);
        let id = match id_result {
            Ok(uuid) => uuid,
            Err(_error) => return Err(ImageIdError::InvalidImageId),
        };

        Ok(Self { id: id })
    }

    pub fn value(&self) -> &Uuid {
        &self.id
    }
}

#[cfg(test)]
mod image_id_tests {
    use uuid::Uuid;

    use crate::image::image::{error::ImageIdError, id::ImageId};

    #[test]
    fn test_normal_convert_from_uuid() {
        let uuid = Uuid::new_v4();
        let id = ImageId::from_uuid(uuid);

        assert_eq!(id.value(), &uuid)
    }

    #[test]
    fn test_normal_convert_from_str() {
        let uuid = Uuid::parse_str("1754ca13-617a-46be-bad2-a83eaedccb77");
        let id = ImageId::from_str("1754ca13-617a-46be-bad2-a83eaedccb77");

        assert_eq!(id.unwrap().value(), &uuid.unwrap())
    }

    #[test]
    fn test_invalid_str() {
        let id = ImageId::from_str("asga129470tgiaehdsg9g8sph");

        assert_eq!(id, Err(ImageIdError::InvalidImageId))
    }
}
