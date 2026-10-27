use std::sync::Arc;

use crate::{
    image::completed::completed_service::CompletedService,
    image_io::get_completed::{
        error::GetCompletedUseCaseError, input::GetCompletedInput, output::GetCompletedOutput,
    },
};

pub struct GetCompletedUseCase {
    completed_service: Arc<dyn CompletedService>,
}

impl GetCompletedUseCase {
    pub fn new(completed_service: Arc<dyn CompletedService>) -> Self {
        Self { completed_service }
    }

    pub fn execute(
        &self,
        input: GetCompletedInput,
    ) -> Result<GetCompletedOutput, GetCompletedUseCaseError> {
        let image_id = input.image_id();

        let completed = self.completed_service.get(image_id)?;

        let image = completed.into_image();
        let (data, _, size) = image.into_data();

        let output = GetCompletedOutput::new(data.into_image(), size.clone());

        Ok(output)
    }
}
