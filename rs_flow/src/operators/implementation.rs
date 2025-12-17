use super::*;
use std::future::Future;
use std::ops::ControlFlow::{Break, Continue};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Default buffer size for converting Flows to streams
///
/// This controls the channel buffer size used when materializing a Flow into a stream.
/// A larger buffer allows more values to be buffered in memory, which can improve
/// throughput at the cost of memory usage.
///
/// 16 is a reasonable default that balances memory usage with performance for most use cases.
const DEFAULT_STREAM_BUFFER_SIZE: usize = 16;

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
                            collector.emit(mapped).await
                        }
                    })
                    .await
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
                                collector.emit(value).await
                            } else {
                                Continue(())
                            }
                        }
                    })
                    .await
            }
        })
    }

    fn take(self, count: usize) -> Flow<T> {
        use crate::internal_utils::spawn_in_scope;

        Flow::new(move |collector| {
            let upstream = self.clone();
            async move {
                if count == 0 {
                    return Continue(());
                }

                let (tx, mut rx) = mpsc::channel::<T>(1);
                let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let done_clone = Arc::clone(&done);

                // Spawn upstream collection in current scope if available
                let producer = spawn_in_scope(async move {
                    let _ = upstream
                        .collect(move |value| {
                            let tx = tx.clone();
                            let done = Arc::clone(&done_clone);
                            async move {
                                if done.load(Ordering::SeqCst) {
                                    // Stop sending - downstream is done
                                    return Break(());
                                }
                                // This will block if receiver is full, providing backpressure
                                if tx.send(value).await.is_err() {
                                    return Break(());
                                }
                                Continue(())
                            }
                        })
                        .await;
                })
                .into_cancel_on_drop();

                // Receive exactly `count` values then stop
                let mut received = 0;
                while received < count {
                    if let Some(value) = rx.recv().await {
                        match collector.emit(value).await {
                            Continue(()) => received += 1,
                            Break(()) => break,
                        }
                    } else {
                        break; // Upstream completed
                    }
                }

                // Signal upstream to stop and cancel producer task
                done.store(true, Ordering::SeqCst);
                drop(rx);
                drop(producer); // Explicitly cancel the producer task
                Continue(())
            }
        })
    }

    fn buffer(self, capacity: usize) -> Flow<T> {
        use crate::internal_utils::spawn_in_scope;
        use std::sync::atomic::{AtomicBool, Ordering};

        Flow::new(move |collector| {
            let upstream = self.clone();
            async move {
                let (tx, mut rx) = mpsc::channel(capacity);
                let stopped = Arc::new(AtomicBool::new(false));

                // Spawn upstream collection in current scope if available
                let stopped_clone = Arc::clone(&stopped);
                let producer = spawn_in_scope(async move {
                    let _ = upstream
                        .collect(move |value| {
                            let tx = tx.clone();
                            let stopped = Arc::clone(&stopped_clone);
                            async move {
                                // CRITICAL: Stop immediately if receiver dropped
                                // Without this check, we busy-loop burning CPU
                                if stopped.load(Ordering::Relaxed) {
                                    return Break(());
                                }

                                // Try to send - if it fails, receiver is dropped, stop collecting
                                if tx.send(value).await.is_err() {
                                    stopped.store(true, Ordering::Relaxed);
                                    return Break(());
                                }
                                Continue(())
                            }
                        })
                        .await;
                })
                .into_cancel_on_drop();

                while let Some(value) = rx.recv().await {
                    match collector.emit(value).await {
                        Continue(()) => {},
                        Break(()) => break,
                    }
                }

                // Producer completes naturally or is cancelled when guard drops
                drop(producer);
                Continue(())
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
                    let _ = upstream
                        .collect(move |value| {
                            let tx = tx.clone();
                            async move {
                                if tx.send(value).await.is_err() {
                                    Break(())
                                } else {
                                    Continue(())
                                }
                            }
                        })
                        .await;
                });

                while let Some(value) = rx.recv().await {
                    match collector.emit(value).await {
                        Continue(()) => {},
                        Break(()) => break,
                    }
                }
                Continue(())
            }
        })
    }

    fn flat_map_latest<U, F, Fut>(self, f: F) -> Flow<U>
    where
        U: Send + 'static,
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Flow<U>> + Send + 'static,
    {
        use crate::internal_utils::spawn_in_scope;
        use futures::StreamExt;

        let f = Arc::new(f);
        Flow::new(move |collector| {
            let upstream = self.clone();
            let f = Arc::clone(&f);
            async move {
                let (tx_inner, mut rx) = mpsc::channel::<Flow<U>>(1);

                // Spawn producer task to transform upstream values to flows
                // Move tx ownership entirely into spawned task to ensure proper cleanup
                let producer = {
                    let tx = tx_inner;
                    spawn_in_scope({
                        let f = Arc::clone(&f);
                        async move {
                            let _ = upstream
                                .collect(move |value| {
                                    let f = Arc::clone(&f);
                                    let tx = tx.clone();
                                    async move {
                                        let flow = f(value).await;
                                        if tx.send(flow).await.is_err() {
                                            Break(())
                                        } else {
                                            Continue(())
                                        }
                                    }
                                })
                                .await;
                            // tx (owned by callback closure) drops here, closing the channel
                        }
                    })
                    .into_cancel_on_drop()
                };

                // Single consumer loop using select! to switch between flows
                //
                // BORROW CHECKER NOTE: The pattern `value = async { match current_stream.as_mut() ... }`
                // is required for select! to work correctly. Alternative patterns like:
                //   - `Some(v) = current_stream.as_mut().and_then(|s| s.next())` don't work
                //   - Direct polling without async block fails borrow checking
                // The async block ensures the mutable borrow is properly scoped within the select! arm.
                let mut current_stream: Option<crate::FlowStream<U>> = None;

                loop {
                    tokio::select! {
                        biased;

                        // New flow arrived - switch to it, or producer finished (None)
                        new_flow_opt = rx.recv() => {
                            match new_flow_opt {
                                Some(new_flow) => {
                                    // Drop old stream (cooperative cancellation via scope if available)
                                    current_stream = Some(new_flow.to_stream(DEFAULT_STREAM_BUFFER_SIZE));
                                }
                                None => {
                                    // Producer is done, no more flows coming
                                    // Finish current stream if any, then exit
                                    if let Some(stream) = current_stream.as_mut() {
                                        while let Some(value) = stream.next().await {
                                            match collector.emit(value).await {
                                                Continue(()) => {},
                                                Break(()) => break,
                                            }
                                        }
                                    }
                                    break;
                                }
                            }
                        }

                        // Next element from current flow (if exists)
                        // Using async block to satisfy borrow checker requirements in select!
                        // When None, pending().await disables this branch automatically
                        value = async {
                            match current_stream.as_mut() {
                                Some(stream) => stream.next().await,
                                None => std::future::pending().await,
                            }
                        } => {
                            if let Some(v) = value {
                                match collector.emit(v).await {
                                    Continue(()) => {},
                                    Break(()) => break,
                                }
                            } else {
                                // Current inner flow completed, clear it and wait for next
                                current_stream = None;
                            }
                        }
                    }
                }

                // Producer guard will cancel on drop
                drop(producer);
                Continue(())
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
                            collector.emit(mapped).await
                        }
                    })
                    .await
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
                                collector.emit(value).await
                            } else {
                                Continue(())
                            }
                        }
                    })
                    .await
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
                                        collector.emit(inner_value).await
                                    }
                                })
                                .await
                        }
                    })
                    .await
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
                            collector.emit(value).await
                        }
                    })
                    .await
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
                            collector.emit(value).await
                        }
                    })
                    .await
            }
        })
    }

    fn drop_first(self, count: usize) -> Flow<T> {
        Flow::new(move |collector| {
            let upstream = self.clone();
            async move {
                let dropped = Arc::new(AtomicUsize::new(0));
                upstream
                    .collect(move |value| {
                        let collector = collector.clone();
                        let dropped = Arc::clone(&dropped);
                        async move {
                            let current = dropped.fetch_add(1, Ordering::SeqCst);
                            if current >= count {
                                collector.emit(value).await
                            } else {
                                Continue(())
                            }
                        }
                    })
                    .await
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
                                    collector.emit(value).await
                                } else {
                                    Continue(())
                                }
                            } else {
                                collector.emit(value).await
                            }
                        }
                    })
                    .await
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
                                collector.emit(value).await
                            } else {
                                done.store(true, Ordering::SeqCst);
                                Break(())
                            }
                        }
                    })
                    .await
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
                                collector.emit(value).await
                            } else {
                                Continue(())
                            }
                        }
                    })
                    .await
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
                                collector.emit(value).await
                            } else {
                                Continue(())
                            }
                        }
                    })
                    .await
            }
        })
    }
}
