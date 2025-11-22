use std::future::Future;
use std::sync::Arc;

use tokio::sync::oneshot;
use tokio::task_local;

use crate::{Dispatcher, JobHandle};

task_local! {
    static CURRENT_SCOPE: Arc<CoroutineScope>;
}

#[derive(Clone)]
pub struct CoroutineScope {
    pub dispatcher: Dispatcher,
    pub job: JobHandle,
}

impl CoroutineScope {
    pub fn new(dispatcher: Dispatcher) -> Self {
        Self {
            dispatcher,
            job: JobHandle::new_root(),
        }
    }

    pub fn launch<F>(&self, fut: F) -> JobHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let scope = Arc::new(self.clone());
        let dispatcher = self.dispatcher.clone();
        let job = self.job.new_child();
        let job_for_task = job.clone();

        dispatcher.spawn(async move {
            CURRENT_SCOPE
                .scope(scope.clone(), async {
                    if !job_for_task.is_cancelled() {
                        fut.await;
                    }
                })
                .await;
        });

        job
    }

    pub fn async_task<F, Fut, T>(&self, dispatcher: Dispatcher, fut: F) -> impl Future<Output = T>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let parent_job = self.job.new_child();

        dispatcher.spawn(async move {
            if parent_job.is_cancelled() {
                return;
            }
            let result = fut().await;
            let _ = tx.send(result);
        });

        async move { rx.await.expect("task cancelled") }
    }

    pub async fn with_dispatcher<F, Fut, T>(&self, dispatcher: Dispatcher, fut: F) -> T
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        self.async_task(dispatcher, fut).await
    }
}

pub async fn with_current_scope<F, Fut, T>(f: F) -> T
where
    F: FnOnce(&CoroutineScope) -> Fut,
    Fut: Future<Output = T>,
{
    CURRENT_SCOPE.with(|scope| f(scope)).await
}
