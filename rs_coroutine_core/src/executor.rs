use std::future::Future;
use std::pin::Pin;

/// Minimal executor trait that can spawn futures
pub trait Executor: Send + Sync + 'static {
    fn spawn(&self, fut: Pin<Box<dyn Future<Output = ()> + Send + 'static>>);
}

/// Tokio executor implementation
pub struct TokioExecutor;

impl Executor for TokioExecutor {
    fn spawn(&self, fut: Pin<Box<dyn Future<Output = ()> + Send + 'static>>) {
        tokio::spawn(fut);
    }
}
