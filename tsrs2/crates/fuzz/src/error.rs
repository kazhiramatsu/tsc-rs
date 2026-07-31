use std::error::Error;
use std::fmt;

pub type FoundationResult<T> = Result<T, FoundationError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FoundationError {
    message: String,
}

impl FoundationError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for FoundationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for FoundationError {}
