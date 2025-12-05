//! Lifecycle operators for Flow
//!
//! These operators allow you to hook into flow lifecycle events
//! like start, completion, and errors.

use crate::flow::{Flow, FlowCollector};
use std::future::Future;
use std::sync::Arc;

/// Lifecycle operators for Flow
pub trait FlowLifecycle<T>: Sized
where
    T: Send + 'static,
{
    /// Execute an action when flow collection starts, before any values are emitted.
    /// The action can emit values.
    ///
    /// # Example
    /// ```ignore
    /// flow.on_start(|collector| async move {
    ///     collector.emit(Loading).await;
    /// })
    /// ```
    fn on_start<F, Fut>(self, action: F) -> Flow<T>
    where
        F: FnOnce(FlowCollector<T>) -> Fut + Send + Sync + 'static + Clone,
        Fut: Future<Output = ()> + Send + 'static;

    /// Execute an action when flow completes (successfully or with error).
    /// The action receives `None` on success, or `Some(error)` on failure.
    /// The action can emit values.
    ///
    /// # Example
    /// ```ignore
    /// flow.on_completion(|collector, error| async move {
    ///     if error.is_none() {
    ///         collector.emit(Done).await;
    ///     }
    /// })
    /// ```
    fn on_completion<F, Fut>(self, action: F) -> Flow<T>
    where
        F: FnOnce(FlowCollector<T>, Option<Box<dyn std::error::Error + Send + Sync>>) -> Fut
            + Send
            + Sync
            + 'static
            + Clone,
        Fut: Future<Output = ()> + Send + 'static;

    /// Execute an action if the flow completes without emitting any values.
    /// The action can emit default values.
    ///
    /// # Example
    /// ```ignore
    /// flow.on_empty(|collector| async move {
    ///     collector.emit(default_value).await;
    /// })
    /// ```
    fn on_empty<F, Fut>(self, action: F) -> Flow<T>
    where
        F: FnOnce(FlowCollector<T>) -> Fut + Send + Sync + 'static + Clone,
        Fut: Future<Output = ()> + Send + 'static;

    /// Catch and handle errors from upstream.
    /// The handler can emit recovery values or re-throw.
    ///
    /// Note: In Rust, we use Result<T, E> instead of exceptions.
    /// This operator catches panics in the upstream flow.
    ///
    /// # Example
    /// ```ignore
    /// flow.catch_panic(|collector, panic_info| async move {
    ///     collector.emit(default_value).await;
    /// })
    /// ```
    fn catch_panic<F, Fut>(self, handler: F) -> Flow<T>
    where
        F: FnOnce(FlowCollector<T>, String) -> Fut + Send + Sync + 'static + Clone,
        Fut: Future<Output = ()> + Send + 'static;

    /// Retry the flow on failure up to `max_retries` times.
    ///
    /// # Example
    /// ```ignore
    /// flow.retry(3) // Retry up to 3 times
    /// ```
    fn retry(self, max_retries: usize) -> Flow<T>
    where
        Self: Clone;

    /// Timeout if no values are emitted within the specified duration.
    /// Returns a flow that completes with an error if timeout occurs.
    ///
    /// # Example
    /// ```ignore
    /// flow.timeout(Duration::from_secs(5))
    /// ```
    fn with_timeout(self, duration: std::time::Duration) -> Flow<T>;
}

impl<T> FlowLifecycle<T> for Flow<T>
where
    T: Send + 'static,
{
    fn on_start<F, Fut>(self, action: F) -> Flow<T>
    where
        F: FnOnce(FlowCollector<T>) -> Fut + Send + Sync + 'static + Clone,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Flow::new(move |collector| {
            let upstream = self.clone();
            let action = action.clone();
            async move {
                // Execute start action first
                action(collector.clone()).await;

                // Then collect from upstream
                upstream
                    .collect(move |value| {
                        let collector = collector.clone();
                        async move {
                            collector.emit(value).await;
                        }
                    })
                    .await;
            }
        })
    }

    fn on_completion<F, Fut>(self, action: F) -> Flow<T>
    where
        F: FnOnce(FlowCollector<T>, Option<Box<dyn std::error::Error + Send + Sync>>) -> Fut
            + Send
            + Sync
            + 'static
            + Clone,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Flow::new(move |collector| {
            let upstream = self.clone();
            let action = action.clone();
            async move {
                let collector_clone = collector.clone();

                // Collect from upstream (catching panics would require more infrastructure)
                upstream
                    .collect(move |value| {
                        let collector = collector_clone.clone();
                        async move {
                            collector.emit(value).await;
                        }
                    })
                    .await;

                // Execute completion action (no error in normal case)
                action(collector, None).await;
            }
        })
    }

    fn on_empty<F, Fut>(self, action: F) -> Flow<T>
    where
        F: FnOnce(FlowCollector<T>) -> Fut + Send + Sync + 'static + Clone,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Flow::new(move |collector| {
            let upstream = self.clone();
            let action = action.clone();
            async move {
                let emitted = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let emitted_clone = Arc::clone(&emitted);
                let collector_clone = collector.clone();

                upstream
                    .collect(move |value| {
                        let collector = collector_clone.clone();
                        let emitted = Arc::clone(&emitted_clone);
                        async move {
                            emitted.store(true, std::sync::atomic::Ordering::SeqCst);
                            collector.emit(value).await;
                        }
                    })
                    .await;

                // If nothing was emitted, execute the action
                if !emitted.load(std::sync::atomic::Ordering::SeqCst) {
                    action(collector).await;
                }
            }
        })
    }

    fn catch_panic<F, Fut>(self, handler: F) -> Flow<T>
    where
        F: FnOnce(FlowCollector<T>, String) -> Fut + Send + Sync + 'static + Clone,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Flow::new(move |collector| {
            let upstream = self.clone();
            let handler = handler.clone();
            async move {
                let collector_clone = collector.clone();

                // Use catch_unwind for panic handling
                let result = std::panic::AssertUnwindSafe(async {
                    upstream
                        .collect(move |value| {
                            let collector = collector_clone.clone();
                            async move {
                                collector.emit(value).await;
                            }
                        })
                        .await;
                });

                match futures::FutureExt::catch_unwind(result).await {
                    Ok(()) => {}
                    Err(panic) => {
                        let panic_msg = if let Some(s) = panic.downcast_ref::<&str>() {
                            s.to_string()
                        } else if let Some(s) = panic.downcast_ref::<String>() {
                            s.clone()
                        } else {
                            "Unknown panic".to_string()
                        };
                        handler(collector, panic_msg).await;
                    }
                }
            }
        })
    }

    fn retry(self, max_retries: usize) -> Flow<T>
    where
        Self: Clone,
    {
        Flow::new(move |collector| {
            let upstream = self.clone();
            async move {
                let mut attempts = 0;

                loop {
                    let collector_clone = collector.clone();
                    let upstream_clone = upstream.clone();

                    let result = std::panic::AssertUnwindSafe(async {
                        upstream_clone
                            .collect(move |value| {
                                let collector = collector_clone.clone();
                                async move {
                                    collector.emit(value).await;
                                }
                            })
                            .await;
                    });

                    match futures::FutureExt::catch_unwind(result).await {
                        Ok(()) => break, // Success
                        Err(_) if attempts < max_retries => {
                            attempts += 1;
                            continue; // Retry
                        }
                        Err(panic) => {
                            std::panic::resume_unwind(panic); // Re-throw
                        }
                    }
                }
            }
        })
    }

    fn with_timeout(self, duration: std::time::Duration) -> Flow<T> {
        Flow::new(move |collector| {
            let upstream = self.clone();
            async move {
                let collector_clone = collector.clone();

                tokio::select! {
                    _ = async {
                        upstream
                            .collect(move |value| {
                                let collector = collector_clone.clone();
                                async move {
                                    collector.emit(value).await;
                                }
                            })
                            .await;
                    } => {}
                    _ = tokio::time::sleep(duration) => {
                        // Timeout - flow completes without emitting more
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::flow;
    use crate::terminal::FlowTerminal;
    use std::time::Duration;

    #[tokio::test]
    async fn test_on_start() {
        let flow = flow(|c| async move {
            c.emit(2).await;
            c.emit(3).await;
        })
        .on_start(|c| async move {
            c.emit(1).await; // Emit 1 before others
        });

        let result = flow.to_vec().await;
        assert_eq!(result, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_on_completion() {
        let flow = flow(|c| async move {
            c.emit(1).await;
            c.emit(2).await;
        })
        .on_completion(|c, _error| async move {
            c.emit(3).await; // Emit 3 at the end
        });

        let result = flow.to_vec().await;
        assert_eq!(result, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_on_empty() {
        let empty: Flow<i32> = flow(|_c| async move {});
        let flow = empty.on_empty(|c| async move {
            c.emit(42).await; // Emit default value
        });

        let result = flow.to_vec().await;
        assert_eq!(result, vec![42]);
    }

    #[tokio::test]
    async fn test_on_empty_not_triggered() {
        let flow = flow(|c| async move {
            c.emit(1).await;
        })
        .on_empty(|c| async move {
            c.emit(42).await; // Should not be called
        });

        let result = flow.to_vec().await;
        assert_eq!(result, vec![1]);
    }

    #[tokio::test]
    async fn test_with_timeout() {
        let flow = flow(|c| async move {
            c.emit(1).await;
            tokio::time::sleep(Duration::from_millis(100)).await;
            c.emit(2).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
            c.emit(3).await; // Should not be emitted
        })
        .with_timeout(Duration::from_millis(250));

        let result = flow.to_vec().await;
        assert_eq!(result, vec![1, 2]);
    }
}
