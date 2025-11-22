use crate::flow::Flow;
use rs_coroutine_core::Dispatcher;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Extension methods for Flow
pub trait FlowExt<T>: Sized
where
    T: Send + 'static,
{
    /// Map each value in the flow
    fn map<U, F, Fut>(self, f: F) -> Flow<U>
    where
        U: Send + 'static,
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = U> + Send + 'static;

    /// Filter values in the flow
    fn filter<F, Fut>(self, predicate: F) -> Flow<T>
    where
        F: Fn(&T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static,
        T: Clone;

    /// Take only the first n values
    fn take(self, count: usize) -> Flow<T>;

    /// Buffer emissions
    fn buffer(self, capacity: usize) -> Flow<T>;

    /// Switch to a different dispatcher for upstream collection
    fn flow_on(self, dispatcher: Dispatcher) -> Flow<T>;

    /// Flat map to the latest flow, cancelling previous
    fn flat_map_latest<U, F, Fut>(self, f: F) -> Flow<U>
    where
        U: Send + 'static,
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Flow<U>> + Send + 'static;
}

impl<T> FlowExt<T> for Flow<T>
where
    T: Send + 'static,
{
    fn map<U, F, Fut>(self, f: F) -> Flow<U>
    where
        U: Send + 'static,
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = U> + Send + 'static,
    {
        let f = Arc::new(f);
        Flow::new(move |collector| {
            let upstream = self.clone();
            let f = Arc::clone(&f);
            async move {
                upstream
                    .collect(move |value| {
                        let f = Arc::clone(&f);
                        let collector = collector.clone();
                        async move {
                            let mapped = f(value).await;
                            collector.emit(mapped).await;
                        }
                    })
                    .await;
            }
        })
    }

    fn filter<F, Fut>(self, predicate: F) -> Flow<T>
    where
        F: Fn(&T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static,
        T: Clone,
    {
        let predicate = Arc::new(predicate);
        Flow::new(move |collector| {
            let upstream = self.clone();
            let predicate = Arc::clone(&predicate);
            async move {
                upstream
                    .collect(move |value| {
                        let predicate = Arc::clone(&predicate);
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

    fn take(self, count: usize) -> Flow<T> {
        Flow::new(move |collector| {
            let upstream = self.clone();
            async move {
                let emitted = Arc::new(AtomicBool::new(false));
                let mut remaining = count;

                upstream
                    .collect(move |value| {
                        let collector = collector.clone();
                        let emitted = Arc::clone(&emitted);
                        async move {
                            if remaining > 0 && !emitted.load(Ordering::SeqCst) {
                                remaining -= 1;
                                collector.emit(value).await;
                                if remaining == 0 {
                                    emitted.store(true, Ordering::SeqCst);
                                }
                            }
                        }
                    })
                    .await;
            }
        })
    }

    fn buffer(self, capacity: usize) -> Flow<T> {
        Flow::new(move |collector| {
            let upstream = self.clone();
            async move {
                let (tx, mut rx) = mpsc::channel(capacity);

                let producer = tokio::spawn(async move {
                    upstream
                        .collect(move |value| {
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

                let _ = producer.await;
            }
        })
    }

    fn flow_on(self, dispatcher: Dispatcher) -> Flow<T> {
        Flow::new(move |collector| {
            let upstream = self.clone();
            let dispatcher = dispatcher.clone();
            async move {
                let (tx, mut rx) = mpsc::channel(16);

                let producer_dispatcher = dispatcher.clone();
                producer_dispatcher.spawn(async move {
                    upstream
                        .collect(move |value| {
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

    fn flat_map_latest<U, F, Fut>(self, f: F) -> Flow<U>
    where
        U: Send + 'static,
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Flow<U>> + Send + 'static,
    {
        let f = Arc::new(f);
        Flow::new(move |collector| {
            let upstream = self.clone();
            let f = Arc::clone(&f);
            async move {
                let (tx, mut rx) = mpsc::channel::<Flow<U>>(1);

                let producer = tokio::spawn({
                    let f = Arc::clone(&f);
                    async move {
                        upstream
                            .collect(move |value| {
                                let f = Arc::clone(&f);
                                let tx = tx.clone();
                                async move {
                                    let flow = f(value).await;
                                    let _ = tx.send(flow).await;
                                }
                            })
                            .await;
                    }
                });

                while let Some(inner_flow) = rx.recv().await {
                    let collector = collector.clone();
                    inner_flow
                        .collect(move |value| {
                            let collector = collector.clone();
                            async move {
                                collector.emit(value).await;
                            }
                        })
                        .await;
                }

                let _ = producer.await;
            }
        })
    }
}
