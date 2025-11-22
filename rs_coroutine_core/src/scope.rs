use crate::executor::Dispatcher;
use crate::job::{CancelToken, JobHandle};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::oneshot;

tokio::task_local! {
    pub static CURRENT_SCOPE: Arc<CoroutineScope>;
}

/// A coroutine scope manages the lifecycle of coroutines
#[derive(Clone)]
pub struct CoroutineScope {
    pub dispatcher: Dispatcher,
    pub job: JobHandle,
    pub cancel_token: CancelToken,
}

impl CoroutineScope {
    /// Create a new CoroutineScope
    pub fn new(dispatcher: Dispatcher) -> Self {
        Self {
            dispatcher,
            job: JobHandle::new(),
            cancel_token: CancelToken::new(),
        }
    }

    /// Launch a new coroutine in this scope
    pub fn launch<F>(&self, fut: F) -> JobHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let scope = Arc::new(self.clone());
        let dispatcher = self.dispatcher.clone();
        let job = self.job.new_child();
        let cancel_token = self.cancel_token.clone();

        let job_clone = job.clone();
        dispatcher.spawn(async move {
            CURRENT_SCOPE
                .scope(scope.clone(), async move {
                    if !cancel_token.is_cancelled() {
                        fut.await;
                    }
                    job_clone.complete();
                })
                .await;
        });

        job
    }

    /// Switch to a different dispatcher for the given future
    pub async fn with_dispatcher<F, T>(&self, dispatcher: Dispatcher, fut: F) -> T
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let child_scope = Arc::new(CoroutineScope {
            dispatcher: dispatcher.clone(),
            job: self.job.new_child(),
            cancel_token: self.cancel_token.child(),
        });

        dispatcher.spawn(async move {
            let res = CURRENT_SCOPE
                .scope(child_scope, async move { fut.await })
                .await;
            let _ = tx.send(res);
        });

        rx.await.expect("dispatcher dropped future")
    }

    /// Async task that returns a Deferred
    pub fn async_task<F, T>(&self, dispatcher: Dispatcher, fut: F) -> Deferred<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let child_scope = Arc::new(CoroutineScope {
            dispatcher: dispatcher.clone(),
            job: self.job.new_child(),
            cancel_token: self.cancel_token.child(),
        });
        let job = child_scope.job.clone();

        dispatcher.spawn(async move {
            let res = CURRENT_SCOPE
                .scope(child_scope, async move { fut.await })
                .await;
            let _ = tx.send(res);
        });

        Deferred { rx, job }
    }

    /// Cancel this scope
    pub fn cancel(&self) {
        self.cancel_token.cancel();
        self.job.cancel();
    }

    /// Check if this scope is cancelled
    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }
}

/// A deferred value that can be awaited
pub struct Deferred<T> {
    rx: oneshot::Receiver<T>,
    job: JobHandle,
}

impl<T> Deferred<T> {
    /// Await the deferred value
    pub async fn await_result(self) -> T {
        self.rx.await.expect("task dropped")
    }

    /// Get the job handle
    pub fn job(&self) -> &JobHandle {
        &self.job
    }
}

/// Helper to access the current scope
pub fn with_current_scope<F, Fut, T>(f: F) -> impl Future<Output = T>
where
    F: FnOnce(&CoroutineScope) -> Fut,
    Fut: Future<Output = T>,
{
    async move {
        CURRENT_SCOPE.with(|scope| f(scope)).await
    }
}

/// Helper to get a reference to the current scope (for macros)
pub fn get_current_scope() -> Arc<CoroutineScope> {
    CURRENT_SCOPE.with(|scope| Arc::clone(scope))
}
