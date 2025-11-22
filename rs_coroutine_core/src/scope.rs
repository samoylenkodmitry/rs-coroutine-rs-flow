use std::future::Future;
use std::sync::Arc;
use tokio::sync::oneshot;
use crate::{Dispatcher, JobHandle, Deferred, CURRENT_SCOPE};

/// CoroutineScope manages the lifecycle and context of coroutines
#[derive(Clone)]
pub struct CoroutineScope {
    pub dispatcher: Dispatcher,
    pub job: JobHandle,
}

impl CoroutineScope {
    pub fn new(dispatcher: Dispatcher) -> Self {
        Self {
            dispatcher,
            job: JobHandle::new(),
        }
    }

    /// Launch a new coroutine
    pub fn launch<F>(&self, fut: F) -> JobHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let scope = Arc::new(self.clone());
        let dispatcher = self.dispatcher.clone();
        let job = self.job.new_child();
        let job_clone = job.clone();

        dispatcher.spawn(async move {
            CURRENT_SCOPE
                .scope(scope.clone(), async move {
                    if !job_clone.is_cancelled() {
                        fut.await;
                    }
                })
                .await;
        });

        job
    }

    /// Switch to a different dispatcher (like Kotlin's withContext)
    pub async fn with_dispatcher<F, T>(&self, dispatcher: Dispatcher, fut: F) -> T
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let child_scope = Arc::new(CoroutineScope {
            dispatcher: dispatcher.clone(),
            job: self.job.new_child(),
        });

        dispatcher.spawn(async move {
            let res = CURRENT_SCOPE
                .scope(child_scope, async move { fut.await })
                .await;
            let _ = tx.send(res);
        });

        rx.await.expect("dispatcher dropped")
    }

    /// Start a parallel task (like Kotlin's async)
    pub fn async_task<F, T>(&self, dispatcher: Dispatcher, fut: F) -> Deferred<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let child_scope = Arc::new(CoroutineScope {
            dispatcher: dispatcher.clone(),
            job: self.job.new_child(),
        });
        let job = child_scope.job.clone();

        dispatcher.spawn(async move {
            let res = CURRENT_SCOPE
                .scope(child_scope, async move { fut.await })
                .await;
            let _ = tx.send(res);
        });

        Deferred::new(rx, job)
    }
}
