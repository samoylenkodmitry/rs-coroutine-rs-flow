use std::fmt;

/// Error type for task execution failures
#[derive(Debug)]
pub enum TaskError {
    /// Task was cancelled
    Cancelled,
    /// Task panicked (panic message)
    Panicked(String),
    /// Task was dropped/aborted before completion
    Aborted,
}

impl fmt::Display for TaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskError::Cancelled => write!(f, "Task was cancelled"),
            TaskError::Panicked(msg) => write!(f, "Task panicked: {}", msg),
            TaskError::Aborted => write!(f, "Task was aborted before completion"),
        }
    }
}

impl std::error::Error for TaskError {}

/// Error type specifically for cancellation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CancellationError;

impl fmt::Display for CancellationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Operation was cancelled")
    }
}

impl std::error::Error for CancellationError {}

impl From<CancellationError> for TaskError {
    fn from(_: CancellationError) -> Self {
        TaskError::Cancelled
    }
}
