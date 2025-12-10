#![deny(warnings)]

pub mod executor;
pub mod job;
pub mod scope;
pub mod suspending;

pub use executor::{Dispatcher, Dispatchers, Executor, TokioExecutor};
pub use job::{CancelToken, JobHandle};
pub use scope::{
    check_cancellation, get_current_scope, with_current_scope, yield_now, CancellationError,
    CoroutineScope, Deferred, CURRENT_SCOPE,
};
pub use suspending::Suspending;
