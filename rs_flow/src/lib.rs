use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use futures::FutureExt;
use rs_coroutine_core::Dispatcher;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

pub struct FlowCollector<T> {
    on_emit: Mutex<Box<dyn FnMut(T) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send>>, 
}

impl<T> FlowCollector<T> {
    pub fn new(on_emit: impl FnMut(T) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + 'static) -> Self {
        Self {
            on_emit: Mutex::new(Box::new(on_emit)),
        }
    }

    pub async fn emit(&self, value: T) {
        let mut guard = self.on_emit.lock().await;
        (guard)(value).await
    }
}

pub struct Flow<T> {
    collect_fn: Arc<dyn Fn(Arc<FlowCollector<T>>) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>,
}

impl<T> Clone for Flow<T> {
    fn clone(&self) -> Self {
        Self {
            collect_fn: self.collect_fn.clone(),
        }
    }
}

impl<T: Send + 'static> Flow<T> {
    pub fn from_fn<F, Fut>(f: F) -> Self
    where
        F: Fn(Arc<FlowCollector<T>>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Self {
            collect_fn: Arc::new(move |collector| Box::pin(f(collector))),
        }
    }

    pub async fn collect<F, Fut>(&self, mut on_value: F)
    where
        F: FnMut(T) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let collector = Arc::new(FlowCollector::new(move |value| on_value(value).boxed()));
        (self.collect_fn)(collector).await;
    }

    pub fn map<U, F, Fut>(self, transform: F) -> Flow<U>
    where
        U: Send + 'static,
        F: Fn(T) -> Fut + Send + Sync + Clone + 'static,
        Fut: Future<Output = U> + Send + 'static,
    {
        Flow::from_fn(move |collector| {
            let upstream = self.clone();
            let transform = transform.clone();
            async move {
                upstream
                    .collect(move |value| {
                        let transform = transform.clone();
                        let collector = collector.clone();
                        async move {
                            let mapped = transform(value).await;
                            collector.emit(mapped).await;
                        }
                    })
                    .await;
            }
        })
    }

    pub fn filter<F, Fut>(self, predicate: F) -> Flow<T>
    where
        F: Fn(&T) -> Fut + Send + Sync + Clone + 'static,
        Fut: Future<Output = bool> + Send + 'static,
        T: Clone,
    {
        Flow::from_fn(move |collector| {
            let upstream = self.clone();
            let predicate = predicate.clone();
            async move {
                upstream
                    .collect(move |value: T| {
                        let predicate = predicate.clone();
                        let collector = collector.clone();
                        async move {
                            if predicate(&value).await {
                                collector.emit(value).await;
                            }
                        }
                    })
                    .await;
            }
        })
    }

    pub fn take(self, limit: usize) -> Flow<T>
    where
        T: Clone,
    {
        Flow::from_fn(move |collector| {
            let upstream = self.clone();
            async move {
                let mut remaining = limit;
                if remaining == 0 {
                    return;
                }
                upstream
                    .collect(move |value: T| {
                        let collector = collector.clone();
                        async move {
                            if remaining > 0 {
                                remaining -= 1;
                                collector.emit(value.clone()).await;
                            }
                        }
                    })
                    .await;
            }
        })
    }

    pub fn buffer(self, capacity: usize) -> Flow<T>
    where
        T: Clone,
    {
        Flow::from_fn(move |collector| {
            let upstream = self.clone();
            async move {
                let (tx, mut rx) = mpsc::channel(capacity);
                tokio::spawn(async move {
                    upstream
                        .collect(|value: T| {
                            let tx = tx.clone();
                            async move {
                                let _ = tx.send(value).await;
                            }
                        })
                        .await;
                });

                while let Some(value) = rx.recv().await {
                    collector.emit(value).await;
                }
            }
        })
    }

    pub fn flow_on(self, dispatcher: Dispatcher) -> Flow<T>
    where
        T: Clone,
    {
        Flow::from_fn(move |collector| {
            let upstream = self.clone();
            let dispatcher = dispatcher.clone();
            async move {
                let (tx, mut rx) = mpsc::channel(8);
                dispatcher.spawn(async move {
                    upstream
                        .collect(|value: T| {
                            let tx = tx.clone();
                            async move {
                                let _ = tx.send(value).await;
                            }
                        })
                        .await;
                });

                while let Some(value) = rx.recv().await {
                    collector.emit(value).await;
                }
            }
        })
    }

    pub fn flat_map_latest<U, F>(self, transform: F) -> Flow<U>
    where
        U: Send + 'static + Clone,
        F: Fn(T) -> Flow<U> + Send + Sync + Clone + 'static,
    {
        Flow::from_fn(move |collector| {
            let upstream = self.clone();
            let transform = transform.clone();
            async move {
                let (tx, mut rx) = mpsc::channel::<U>(8);
                tokio::spawn(async move {
                    let latest = Arc::new(Mutex::new(None::<JoinHandle<()>>));
                    upstream
                        .collect(move |value| {
                            let tx = tx.clone();
                            let transform = transform.clone();
                            let latest = latest.clone();
                            async move {
                                if let Some(handle) = latest.lock().await.take() {
                                    handle.abort();
                                }
                                let flow = transform(value);
                                let handle = tokio::spawn(async move {
                                    flow
                                        .collect(|inner| {
                                            let tx = tx.clone();
                                            async move {
                                                let _ = tx.send(inner).await;
                                            }
                                        })
                                        .await;
                                });
                                *latest.lock().await = Some(handle);
                            }
                        })
                        .await;
                });

                while let Some(value) = rx.recv().await {
                    collector.emit(value.clone()).await;
                }
            }
        })
    }
}

pub fn flow<T, F, Fut>(builder: F) -> Flow<T>
where
    T: Send + 'static,
    F: Fn(Arc<FlowCollector<T>>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    Flow::from_fn(builder)
}

#[macro_export]
macro_rules! flow_block {
    ($($body:tt)*) => {
        rs_flow::Flow::from_fn(move |emit| async move { $($body)* })
    };
}

pub mod suspending_ext {
    use rs_coroutine_core::suspending::Suspending;

    use super::{Flow, FlowCollector};
    use std::sync::Arc;

    pub trait SuspendingFlowExt<T> {
        fn as_flow(self) -> Flow<T>;
    }

    impl<T: Send + 'static> SuspendingFlowExt<T> for Suspending<T> {
        fn as_flow(self) -> Flow<T> {
            Flow::from_fn(move |collector: Arc<FlowCollector<T>>| {
                let susp = self.clone();
                async move {
                    let value = susp.call().await;
                    collector.emit(value).await;
                }
            })
        }
    }
}

pub mod hot {
    use super::{Flow, FlowCollector};
    use std::sync::Arc;
    use tokio::sync::{broadcast, watch};

    pub struct SharedFlow<T> {
        sender: broadcast::Sender<T>,
    }

    impl<T: Clone + Send + Sync + 'static> SharedFlow<T> {
        pub fn new(buffer: usize) -> Self {
            let (sender, _) = broadcast::channel(buffer);
            Self { sender }
        }

        pub async fn emit(&self, value: T) {
            let _ = self.sender.send(value);
        }

        pub fn as_flow(&self) -> Flow<T> {
            let sender = self.sender.clone();
            Flow::from_fn(move |collector: Arc<FlowCollector<T>>| {
                let mut rx = sender.subscribe();
                async move {
                    while let Ok(value) = rx.recv().await {
                        collector.emit(value).await;
                    }
                }
            })
        }
    }

    pub struct StateFlow<T> {
        sender: watch::Sender<T>,
    }

    impl<T: Clone + Send + Sync + 'static> StateFlow<T> {
        pub fn new(initial: T) -> Self {
            let (sender, _) = watch::channel(initial);
            Self { sender }
        }

        pub async fn emit(&self, value: T) {
            let _ = self.sender.send(value);
        }

        pub fn as_flow(&self) -> Flow<T> {
            let mut rx = self.sender.subscribe();
            Flow::from_fn(move |collector: Arc<FlowCollector<T>>| async move {
                collector.emit(rx.borrow().clone()).await;
                while rx.changed().await.is_ok() {
                    collector.emit(rx.borrow().clone()).await;
                }
            })
        }
    }
}
