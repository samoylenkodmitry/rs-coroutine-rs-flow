use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Type-erased join handle that can be awaited to detect panics
///
/// **Note:** This is explicitly a Tokio `JoinHandle`. This library is **tokio-only**
/// and does not abstract over other async runtimes. The JoinHandle is required for
/// proper panic detection using `JoinError::is_panic()`.
pub type BoxedJoinHandle = tokio::task::JoinHandle<()>;

/// Minimal executor trait for spawning futures
///
/// **Note:** This trait is **tokio-specific** and cannot be implemented for other runtimes.
/// The return type `BoxedJoinHandle` is `tokio::task::JoinHandle<()>`, which is required
/// for proper panic detection. This library makes no attempt to abstract over async runtimes.
///
/// The trait exists primarily for:
/// - Testing/mocking purposes
/// - Potential future runtime configuration (e.g., different tokio runtime flavors)
///
/// But it is **not** a generic executor abstraction.
pub trait Executor: Send + Sync + 'static {
    fn spawn(&self, fut: Pin<Box<dyn Future<Output = ()> + Send + 'static>>) -> BoxedJoinHandle;
}

/// Dispatcher wraps an Executor and provides a cloneable interface
#[derive(Clone)]
pub struct Dispatcher {
    inner: Arc<dyn Executor>,
}

impl Dispatcher {
    /// Create a new Dispatcher from an Executor
    pub fn new(inner: Arc<dyn Executor>) -> Self {
        Self { inner }
    }

    /// Spawn a future on this dispatcher
    /// Returns a JoinHandle that can be used to detect panics
    pub fn spawn(&self, fut: impl Future<Output = ()> + Send + 'static) -> BoxedJoinHandle {
        self.inner.spawn(Box::pin(fut))
    }
}

/// Default Tokio executor implementation
pub struct TokioExecutor;

impl Executor for TokioExecutor {
    fn spawn(&self, fut: Pin<Box<dyn Future<Output = ()> + Send + 'static>>) -> BoxedJoinHandle {
        tokio::spawn(fut)
    }
}

/// Registry of standard dispatchers
pub struct Dispatchers;

impl Dispatchers {
    /// Main/Default dispatcher using Tokio runtime
    pub fn main() -> Dispatcher {
        Dispatcher::new(Arc::new(TokioExecutor))
    }

    /// IO dispatcher for blocking IO operations
    pub fn io() -> Dispatcher {
        Dispatcher::new(Arc::new(TokioExecutor))
    }
}

impl Default for Dispatcher {
    fn default() -> Self {
        Dispatchers::main()
    }
}
