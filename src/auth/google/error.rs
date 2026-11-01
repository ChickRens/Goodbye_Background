use crate::{
    common::traits::ErrorType::{Internal, InvalidInput},
    leaf_detail_error, leaf_error,
};

leaf_error!(
    pub enum GoogleLoginConfigError {
        NotConfigured => ("GOOGLE_LOGIN_NOT_CONFIGURED", Internal),
    }
);

leaf_detail_error!(
    pub enum GoogleAuthError {
        Request => ("GOOGLE_AUTH_REQUEST_FAILED", Internal),
        InvalidResponse => ("GOOGLE_AUTH_INVALID_RESPONSE", Internal),
        EmailNotVerified => ("GOOGLE_EMAIL_NOT_VERIFIED", InvalidInput),
    }
);
