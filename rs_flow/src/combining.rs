//! Combining operators for Flow
//!
//! These operators allow you to combine multiple flows into one.

use crate::flow::Flow;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

/// Combining operators for Flow
pub trait FlowCombining<T>: Sized
where
    T: Send + 'static,
{
    /// Combine this flow with another, emitting a new value whenever either flow emits.
    /// The transform function receives the latest values from both flows.
    ///
    /// # Example
    /// ```ignore
    /// let combined = flow1.combine(flow2, |a, b| a + b);
    /// ```
    fn combine<U, R, F>(self, other: Flow<U>, transform: F) -> Flow<R>
    where
        U: Send + Clone + 'static,
        R: Send + 'static,
        T: Clone,
        F: Fn(T, U) -> R + Send + Sync + 'static;

    /// Zip this flow with another, pairing values one-to-one.
    /// Completes when either flow completes.
    ///
    /// # Example
    /// ```ignore
    /// let zipped = flow1.zip(flow2, |a, b| (a, b));
    /// ```
    fn zip<U, R, F>(self, other: Flow<U>, transform: F) -> Flow<R>
    where
        U: Send + 'static,
        R: Send + 'static,
        F: Fn(T, U) -> R + Send + Sync + 'static;

    /// Combine this flow with another, emitting only the latest value from this flow
    /// when the other flow emits.
    ///
    /// # Example
    /// ```ignore
    /// let sampled = data_flow.sample(tick_flow);
    /// ```
    fn sample<U>(self, sampler: Flow<U>) -> Flow<T>
    where
        U: Send + 'static,
        T: Clone;

    /// Concatenate this flow with another, emitting all values from this flow
    /// first, then all values from the other flow.
    ///
    /// # Example
    /// ```ignore
    /// let concatenated = flow1.concat(flow2);
    /// ```
    fn concat(self, other: Flow<T>) -> Flow<T>;

    /// Start with the given values, then emit values from this flow.
    ///
    /// # Example
    /// ```ignore
    /// let flow = numbers.start_with(vec![0]);
    /// ```
    fn start_with<I>(self, values: I) -> Flow<T>
    where
        I: IntoIterator<Item = T> + Clone + Send + Sync + 'static,
        I::IntoIter: Send;
}

impl<T> FlowCombining<T> for Flow<T>
where
    T: Send + 'static,
{
    fn combine<U, R, F>(self, other: Flow<U>, transform: F) -> Flow<R>
    where
        U: Send + Clone + 'static,
        R: Send + 'static,
        T: Clone,
        F: Fn(T, U) -> R + Send + Sync + 'static,
    {
        let transform = Arc::new(transform);

        Flow::new(move |collector| {
            let upstream1 = self.clone();
            let upstream2 = other.clone();
            let transform = Arc::clone(&transform);

            async move {
                let latest1: Arc<Mutex<Option<T>>> = Arc::new(Mutex::new(None));
                let latest2: Arc<Mutex<Option<U>>> = Arc::new(Mutex::new(None));

                let (tx, mut rx) = mpsc::channel::<(Option<T>, Option<U>)>(16);

                // Spawn task to collect from first flow
                let tx1 = tx.clone();
                let latest1_clone = Arc::clone(&latest1);
                let latest2_clone = Arc::clone(&latest2);
                let task1 = tokio::spawn(async move {
                    upstream1
                        .collect(move |value| {
                            let tx = tx1.clone();
                            let latest1 = Arc::clone(&latest1_clone);
                            let latest2 = Arc::clone(&latest2_clone);
                            async move {
                                *latest1.lock().await = Some(value.clone());
                                let l2 = latest2.lock().await.clone();
                                let _ = tx.send((Some(value), l2)).await;
                            }
                        })
                        .await;
                });

                // Spawn task to collect from second flow
                let tx2 = tx.clone();
                let latest1_clone = Arc::clone(&latest1);
                let latest2_clone = Arc::clone(&latest2);
                let task2 = tokio::spawn(async move {
                    upstream2
                        .collect(move |value| {
                            let tx = tx2.clone();
                            let latest1 = Arc::clone(&latest1_clone);
                            let latest2 = Arc::clone(&latest2_clone);
                            async move {
                                *latest2.lock().await = Some(value.clone());
                                let l1 = latest1.lock().await.clone();
                                let _ = tx.send((l1, Some(value))).await;
                            }
                        })
                        .await;
                });

                // Drop our sender so rx will close when tasks complete
                drop(tx);

                // Emit combined values
                while let Some((v1, v2)) = rx.recv().await {
                    if let (Some(a), Some(b)) = (v1, v2) {
                        collector.emit(transform(a, b)).await;
                    }
                }

                let _ = task1.await;
                let _ = task2.await;
            }
        })
    }

    fn zip<U, R, F>(self, other: Flow<U>, transform: F) -> Flow<R>
    where
        U: Send + 'static,
        R: Send + 'static,
        F: Fn(T, U) -> R + Send + Sync + 'static,
    {
        let transform = Arc::new(transform);

        Flow::new(move |collector| {
            let upstream1 = self.clone();
            let upstream2 = other.clone();
            let transform = Arc::clone(&transform);

            async move {
                let (tx1, mut rx1) = mpsc::channel::<T>(16);
                let (tx2, mut rx2) = mpsc::channel::<U>(16);

                // Spawn task to collect from first flow
                let task1 = tokio::spawn(async move {
                    upstream1
                        .collect(move |value| {
                            let tx = tx1.clone();
                            async move {
                                let _ = tx.send(value).await;
                            }
                        })
                        .await;
                });

                // Spawn task to collect from second flow
                let task2 = tokio::spawn(async move {
                    upstream2
                        .collect(move |value| {
                            let tx = tx2.clone();
                            async move {
                                let _ = tx.send(value).await;
                            }
                        })
                        .await;
                });

                // Zip values
                while let (Some(v1), Some(v2)) = (rx1.recv().await, rx2.recv().await) {
                    collector.emit(transform(v1, v2)).await;
                }

                let _ = task1.await;
                let _ = task2.await;
            }
        })
    }

    fn sample<U>(self, sampler: Flow<U>) -> Flow<T>
    where
        U: Send + 'static,
        T: Clone,
    {
        Flow::new(move |collector| {
            let upstream = self.clone();
            let sampler = sampler.clone();

            async move {
                let latest: Arc<Mutex<Option<T>>> = Arc::new(Mutex::new(None));
                let (tx, mut rx) = mpsc::channel::<()>(16);

                // Spawn task to collect from data flow
                let latest_clone = Arc::clone(&latest);
                let task1 = tokio::spawn(async move {
                    upstream
                        .collect(move |value| {
                            let latest = Arc::clone(&latest_clone);
                            async move {
                                *latest.lock().await = Some(value);
                            }
                        })
                        .await;
                });

                // Spawn task to collect from sampler flow
                let task2 = tokio::spawn(async move {
                    sampler
                        .collect(move |_| {
                            let tx = tx.clone();
                            async move {
                                let _ = tx.send(()).await;
                            }
                        })
                        .await;
                });

                // Emit sampled values
                while let Some(()) = rx.recv().await {
                    if let Some(value) = latest.lock().await.clone() {
                        collector.emit(value).await;
                    }
                }

                let _ = task1.await;
                let _ = task2.await;
            }
        })
    }

    fn concat(self, other: Flow<T>) -> Flow<T> {
        Flow::new(move |collector| {
            let first = self.clone();
            let second = other.clone();

            async move {
                // Collect from first flow
                let collector_clone = collector.clone();
                first
                    .collect(move |value| {
                        let collector = collector_clone.clone();
                        async move {
                            collector.emit(value).await;
                        }
                    })
                    .await;

                // Then collect from second flow
                second
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

    fn start_with<I>(self, values: I) -> Flow<T>
    where
        I: IntoIterator<Item = T> + Clone + Send + Sync + 'static,
        I::IntoIter: Send,
    {
        Flow::new(move |collector| {
            let upstream = self.clone();
            let values = values.clone();

            async move {
                // Emit initial values
                for value in values {
                    collector.emit(value).await;
                }

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
}

/// Merge multiple flows into a single flow.
/// Values are emitted as they arrive from any source flow.
///
/// # Example
/// ```ignore
/// let merged = merge(vec![flow1, flow2, flow3]);
/// ```
pub fn merge<T>(flows: Vec<Flow<T>>) -> Flow<T>
where
    T: Send + 'static,
{
    Flow::new(move |collector| {
        let flows = flows.clone();

        async move {
            let (tx, mut rx) = mpsc::channel::<T>(16);

            // Spawn a task for each flow
            let tasks: Vec<_> = flows
                .into_iter()
                .map(|flow| {
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        flow.collect(move |value| {
                            let tx = tx.clone();
                            async move {
                                let _ = tx.send(value).await;
                            }
                        })
                        .await;
                    })
                })
                .collect();

            // Drop our sender so rx will close when all tasks complete
            drop(tx);

            // Emit merged values
            while let Some(value) = rx.recv().await {
                collector.emit(value).await;
            }

            // Wait for all tasks
            for task in tasks {
                let _ = task.await;
            }
        }
    })
}

/// Macro to merge multiple flows
///
/// # Example
/// ```ignore
/// let merged = merge!(flow1, flow2, flow3);
/// ```
#[macro_export]
macro_rules! merge {
    ($($flow:expr),+ $(,)?) => {
        $crate::combining::merge(vec![$($flow),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::flow;
    use crate::terminal::FlowTerminal;
    use std::time::Duration;

    #[tokio::test]
    async fn test_zip() {
        let flow1 = flow(|c| async move {
            c.emit(1).await;
            c.emit(2).await;
            c.emit(3).await;
        });

        let flow2 = flow(|c| async move {
            c.emit("a").await;
            c.emit("b").await;
            c.emit("c").await;
        });

        let zipped = flow1.zip(flow2, |a, b| format!("{}{}", a, b));
        let result = zipped.to_vec().await;

        assert_eq!(result, vec!["1a", "2b", "3c"]);
    }

    #[tokio::test]
    async fn test_concat() {
        let flow1 = flow(|c| async move {
            c.emit(1).await;
            c.emit(2).await;
        });

        let flow2 = flow(|c| async move {
            c.emit(3).await;
            c.emit(4).await;
        });

        let concatenated = flow1.concat(flow2);
        let result = concatenated.to_vec().await;

        assert_eq!(result, vec![1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn test_start_with() {
        let flow = flow(|c| async move {
            c.emit(3).await;
            c.emit(4).await;
        })
        .start_with(vec![1, 2]);

        let result = flow.to_vec().await;
        assert_eq!(result, vec![1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn test_merge() {
        let flow1 = flow(|c| async move {
            c.emit(1).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
            c.emit(3).await;
        });

        let flow2 = flow(|c| async move {
            tokio::time::sleep(Duration::from_millis(25)).await;
            c.emit(2).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
            c.emit(4).await;
        });

        let merged = merge(vec![flow1, flow2]);
        let result = merged.to_vec().await;

        // Values arrive in time order (roughly)
        assert_eq!(result.len(), 4);
        assert!(result.contains(&1));
        assert!(result.contains(&2));
        assert!(result.contains(&3));
        assert!(result.contains(&4));
    }
}
