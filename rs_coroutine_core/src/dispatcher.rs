use std::future::Future;
use std::sync::Arc;
use crate::executor::{Executor, TokioExecutor};

/// Dispatcher encapsulates an executor
#[derive(Clone)]
pub struct Dispatcher {
    inner: Arc<dyn Executor>,
}

impl Dispatcher {
    pub fn new(inner: Arc<dyn Executor>) -> Self {
        Self { inner }
    }

    pub fn spawn(&self, fut: impl Future<Output = ()> + Send + 'static) {
        self.inner.spawn(Box::pin(fut));
    }
}

/// Registry of common dispatchers
pub struct Dispatchers;

impl Dispatchers {
    /// Main dispatcher (uses default Tokio runtime)
    pub fn main() -> Dispatcher {
        Dispatcher::new(Arc::new(TokioExecutor))
    }

    /// IO dispatcher (uses default Tokio runtime)
    pub fn io() -> Dispatcher {
        Dispatcher::new(Arc::new(TokioExecutor))
    }

    /// Default dispatcher
    pub fn default() -> Dispatcher {
        Dispatcher::new(Arc::new(TokioExecutor))
    }
}
