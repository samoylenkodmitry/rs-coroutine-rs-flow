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
    /// Map each value in the flow (async)
    fn map<U, F, Fut>(self, f: F) -> Flow<U>
    where
        U: Send + 'static,
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = U> + Send + 'static;

    /// Map each value in the flow (sync - Kotlin-like)
    ///
    /// # Example
    /// ```ignore
    /// flow.map_sync(|x| x * 2)
    /// ```
    fn map_sync<U, F>(self, f: F) -> Flow<U>
    where
        U: Send + 'static,
        F: Fn(T) -> U + Send + Sync + 'static;

    /// Filter values in the flow (async)
    fn filter<F, Fut>(self, predicate: F) -> Flow<T>
    where
        F: Fn(&T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static,
        T: Clone;

    /// Filter values in the flow (sync - Kotlin-like)
    ///
    /// # Example
    /// ```ignore
    /// flow.filter_sync(|x| x % 2 == 0)
    /// ```
    fn filter_sync<F>(self, predicate: F) -> Flow<T>
    where
        F: Fn(&T) -> bool + Send + Sync + 'static,
        T: Clone;

    /// Take only the first n values
    fn take(self, count: usize) -> Flow<T>;

    /// Buffer emissions
    fn buffer(self, capacity: usize) -> Flow<T>;

    /// Switch to a different dispatcher for upstream collection
    fn flow_on(self, dispatcher: Dispatcher) -> Flow<T>;

    /// Flat map to the latest flow, cancelling previous (async)
    fn flat_map_latest<U, F, Fut>(self, f: F) -> Flow<U>
    where
        U: Send + 'static,
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Flow<U>> + Send + 'static;

    /// Flat map to the latest flow, cancelling previous (sync)
    fn flat_map_latest_sync<U, F>(self, f: F) -> Flow<U>
    where
        U: Send + 'static,
        F: Fn(T) -> Flow<U> + Send + Sync + 'static;

    /// Transform each value and flatten (async)
    fn flat_map<U, F, Fut>(self, f: F) -> Flow<U>
    where
        U: Send + 'static,
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Flow<U>> + Send + 'static;

    /// Transform each value and flatten (sync)
    fn flat_map_sync<U, F>(self, f: F) -> Flow<U>
    where
        U: Send + 'static,
        F: Fn(T) -> Flow<U> + Send + Sync + 'static;

    /// Perform a side effect for each value (sync - Kotlin's onEach)
    fn on_each<F>(self, f: F) -> Flow<T>
    where
        F: Fn(&T) + Send + Sync + 'static,
        T: Clone;

    /// Perform a side effect for each value (async)
    fn on_each_async<F, Fut>(self, f: F) -> Flow<T>
    where
        F: Fn(&T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
        T: Clone;

    /// Skip the first n values (Kotlin's drop)
    fn drop_first(self, count: usize) -> Flow<T>;

    /// Skip values while predicate is true (sync)
    fn drop_while<F>(self, predicate: F) -> Flow<T>
    where
        F: Fn(&T) -> bool + Send + Sync + 'static,
        T: Clone;

    /// Take values while predicate is true (sync)
    fn take_while<F>(self, predicate: F) -> Flow<T>
    where
        F: Fn(&T) -> bool + Send + Sync + 'static,
        T: Clone;

    /// Only emit distinct consecutive values
    fn distinct_until_changed(self) -> Flow<T>
    where
        T: Clone + PartialEq;

    /// Only emit distinct consecutive values by key
    fn distinct_until_changed_by<K, F>(self, key_selector: F) -> Flow<T>
    where
        K: PartialEq + Send + 'static,
        F: Fn(&T) -> K + Send + Sync + 'static,
        T: Clone;
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
                let emitted = Arc::new(std::sync::atomic::AtomicUsize::new(0));

                upstream
                    .collect(move |value| {
                        let collector = collector.clone();
                        let emitted = Arc::clone(&emitted);
                        async move {
                            // Atomically increment and check if we should emit
                            let prev_emitted = emitted.fetch_add(1, Ordering::SeqCst);
                            if prev_emitted < count {
                                collector.emit(value).await;
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

    fn map_sync<U, F>(self, f: F) -> Flow<U>
    where
        U: Send + 'static,
        F: Fn(T) -> U + Send + Sync + 'static,
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
                            let mapped = f(value);
                            collector.emit(mapped).await;
                        }
                    })
                    .await;
            }
        })
    }

    fn filter_sync<F>(self, predicate: F) -> Flow<T>
    where
        F: Fn(&T) -> bool + Send + Sync + 'static,
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
                            if predicate(&value) {
                                collector.emit(value).await;
                            }
                        }
                    })
                    .await;
            }
        })
    }

    fn flat_map_latest_sync<U, F>(self, f: F) -> Flow<U>
    where
        U: Send + 'static,
        F: Fn(T) -> Flow<U> + Send + Sync + 'static,
    {
        let f = Arc::new(f);
        self.flat_map_latest(move |value| {
            let f = Arc::clone(&f);
            async move { f(value) }
        })
    }

    fn flat_map<U, F, Fut>(self, f: F) -> Flow<U>
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
                upstream
                    .collect(move |value| {
                        let f = Arc::clone(&f);
                        let collector = collector.clone();
                        async move {
                            let inner_flow = f(value).await;
                            inner_flow
                                .collect(move |inner_value| {
                                    let collector = collector.clone();
                                    async move {
                                        collector.emit(inner_value).await;
                                    }
                                })
                                .await;
                        }
                    })
                    .await;
            }
        })
    }

    fn flat_map_sync<U, F>(self, f: F) -> Flow<U>
    where
        U: Send + 'static,
        F: Fn(T) -> Flow<U> + Send + Sync + 'static,
    {
        let f = Arc::new(f);
        self.flat_map(move |value| {
            let f = Arc::clone(&f);
            async move { f(value) }
        })
    }

    fn on_each<F>(self, f: F) -> Flow<T>
    where
        F: Fn(&T) + Send + Sync + 'static,
        T: Clone,
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
                            f(&value);
                            collector.emit(value).await;
                        }
                    })
                    .await;
            }
        })
    }

    fn on_each_async<F, Fut>(self, f: F) -> Flow<T>
    where
        F: Fn(&T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
        T: Clone,
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
                            f(&value).await;
                            collector.emit(value).await;
                        }
                    })
                    .await;
            }
        })
    }

    fn drop_first(self, count: usize) -> Flow<T> {
        Flow::new(move |collector| {
            let upstream = self.clone();
            async move {
                let dropped = Arc::new(std::sync::atomic::AtomicUsize::new(0));
                upstream
                    .collect(move |value| {
                        let collector = collector.clone();
                        let dropped = Arc::clone(&dropped);
                        async move {
                            let current = dropped.fetch_add(1, Ordering::SeqCst);
                            if current >= count {
                                collector.emit(value).await;
                            }
                        }
                    })
                    .await;
            }
        })
    }

    fn drop_while<F>(self, predicate: F) -> Flow<T>
    where
        F: Fn(&T) -> bool + Send + Sync + 'static,
        T: Clone,
    {
        let predicate = Arc::new(predicate);
        Flow::new(move |collector| {
            let upstream = self.clone();
            let predicate = Arc::clone(&predicate);
            async move {
                let dropping = Arc::new(AtomicBool::new(true));
                upstream
                    .collect(move |value| {
                        let predicate = Arc::clone(&predicate);
                        let collector = collector.clone();
                        let dropping = Arc::clone(&dropping);
                        async move {
                            if dropping.load(Ordering::SeqCst) {
                                if !predicate(&value) {
                                    dropping.store(false, Ordering::SeqCst);
                                    collector.emit(value).await;
                                }
                            } else {
                                collector.emit(value).await;
                            }
                        }
                    })
                    .await;
            }
        })
    }

    fn take_while<F>(self, predicate: F) -> Flow<T>
    where
        F: Fn(&T) -> bool + Send + Sync + 'static,
        T: Clone,
    {
        let predicate = Arc::new(predicate);
        Flow::new(move |collector| {
            let upstream = self.clone();
            let predicate = Arc::clone(&predicate);
            async move {
                let done = Arc::new(AtomicBool::new(false));
                upstream
                    .collect(move |value| {
                        let predicate = Arc::clone(&predicate);
                        let collector = collector.clone();
                        let done = Arc::clone(&done);
                        async move {
                            if !done.load(Ordering::SeqCst) && predicate(&value) {
                                collector.emit(value).await;
                            } else {
                                done.store(true, Ordering::SeqCst);
                            }
                        }
                    })
                    .await;
            }
        })
    }

    fn distinct_until_changed(self) -> Flow<T>
    where
        T: Clone + PartialEq,
    {
        Flow::new(move |collector| {
            let upstream = self.clone();
            async move {
                let last = Arc::new(tokio::sync::Mutex::new(None::<T>));
                upstream
                    .collect(move |value| {
                        let collector = collector.clone();
                        let last = Arc::clone(&last);
                        async move {
                            let mut guard = last.lock().await;
                            let should_emit = match &*guard {
                                None => true,
                                Some(prev) => prev != &value,
                            };
                            if should_emit {
                                *guard = Some(value.clone());
                                drop(guard);
                                collector.emit(value).await;
                            }
                        }
                    })
                    .await;
            }
        })
    }

    fn distinct_until_changed_by<K, F>(self, key_selector: F) -> Flow<T>
    where
        K: PartialEq + Send + 'static,
        F: Fn(&T) -> K + Send + Sync + 'static,
        T: Clone,
    {
        let key_selector = Arc::new(key_selector);
        Flow::new(move |collector| {
            let upstream = self.clone();
            let key_selector = Arc::clone(&key_selector);
            async move {
                let last_key = Arc::new(tokio::sync::Mutex::new(None::<K>));
                upstream
                    .collect(move |value| {
                        let collector = collector.clone();
                        let last_key = Arc::clone(&last_key);
                        let key_selector = Arc::clone(&key_selector);
                        async move {
                            let key = key_selector(&value);
                            let mut guard = last_key.lock().await;
                            let should_emit = match &*guard {
                                None => true,
                                Some(prev_key) => prev_key != &key,
                            };
                            if should_emit {
                                *guard = Some(key);
                                drop(guard);
                                collector.emit(value).await;
                            }
                        }
                    })
                    .await;
            }
        })
    }
}
