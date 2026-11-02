pub trait Code {
    fn code(&self) -> &str;
}

pub trait Cause {
    fn cause(&self) -> Option<&str>;
}

pub enum ErrorType {
    InvalidInput,
    Unauthorized,
    Conflict,
    NotFound,
    Internal,
}

pub trait ErrorTypeProvider {
    fn error_type(&self) -> ErrorType;
}
