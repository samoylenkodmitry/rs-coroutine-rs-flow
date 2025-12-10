#![deny(warnings)]

pub mod executor;
pub mod job;
pub mod scope;
pub mod suspending;

#[cfg(test)]
pub mod test_utils;

// Also make test_utils available for integration tests
#[cfg(not(test))]
pub mod test_utils;

pub use executor::{Dispatcher, Dispatchers, Executor, TokioExecutor};
pub use job::{CancelToken, JobHandle};
pub use scope::{
    check_cancellation, get_current_scope, with_current_scope, yield_now, CancellationError,
    CoroutineScope, Deferred, CURRENT_SCOPE,
};
pub use suspending::Suspending;
