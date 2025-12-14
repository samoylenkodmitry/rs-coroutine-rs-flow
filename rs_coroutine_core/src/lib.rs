#![forbid(unsafe_code)]
#![deny(warnings)]

pub mod error;
pub mod executor;
pub mod job;
pub mod scope;
pub mod suspending;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

pub use error::{CancellationError, NotInScopeError, TaskError};
pub use executor::{Dispatcher, Dispatchers, Executor, TokioExecutor};
pub use job::{CancelToken, JobHandle};
pub use scope::{
    check_cancellation, check_cancellation_lenient, get_current_scope, require_scope,
    with_current_scope, yield_now, CoroutineScope, Deferred, CURRENT_SCOPE,
};
pub use suspending::Suspending;
