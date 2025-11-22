use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use once_cell::sync::Lazy;
use tokio::runtime::{self, Runtime};

pub trait Executor: Send + Sync + 'static {
    fn spawn(&self, fut: Pin<Box<dyn Future<Output = ()> + Send>>);
}

#[derive(Clone)]
pub struct Dispatcher {
    inner: Arc<dyn Executor>,
}

impl Dispatcher {
    pub fn new(inner: Arc<dyn Executor>) -> Self {
        Self { inner }
    }

    pub fn spawn(&self, fut: impl Future<Output = ()> + Send + 'static) {
        self.inner.spawn(Box::pin(fut))
    }
}

struct TokioExecutor {
    runtime: Runtime,
}

impl TokioExecutor {
    fn new() -> Self {
        let runtime = runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build runtime");
        Self { runtime }
    }
}

impl Executor for TokioExecutor {
    fn spawn(&self, fut: Pin<Box<dyn Future<Output = ()> + Send>>) {
        let _ = self.runtime.spawn(fut);
    }
}

pub struct Dispatchers;

impl Dispatchers {
    fn default_runtime_dispatcher() -> Dispatcher {
        static DEFAULT: Lazy<Dispatcher> = Lazy::new(|| {
            let exec = Arc::new(TokioExecutor::new());
            Dispatcher::new(exec)
        });

        DEFAULT.clone()
    }

    pub fn main() -> Dispatcher {
        Self::default_runtime_dispatcher()
    }

    pub fn io() -> Dispatcher {
        Self::default_runtime_dispatcher()
    }

    pub fn new_custom(executor: Arc<dyn Executor>) -> Dispatcher {
        Dispatcher::new(executor)
    }
}
