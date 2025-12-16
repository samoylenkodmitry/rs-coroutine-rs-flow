use std::fmt;

/// Error type for task execution failures
#[derive(Debug, Clone)]
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

/// Error returned when cancellation check is performed outside a scope
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotInScopeError;

impl std::fmt::Display for NotInScopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Operation requires being inside a CoroutineScope")
    }
}

impl std::error::Error for NotInScopeError {}

/// Extract a panic message from a JoinError
///
/// Attempts to downcast the panic payload to common string types.
/// Returns a generic message if the payload is not a string.
pub fn extract_panic_message(join_error: tokio::task::JoinError) -> String {
    let panic_payload = join_error.into_panic();

    if let Some(s) = panic_payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = panic_payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic with non-string payload".to_string()
    }
}
