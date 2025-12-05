//! Terminal operators for Flow
//!
//! Terminal operators consume the flow and produce a final result.
//! They are all async functions that suspend until the flow completes.

use crate::flow::Flow;
use std::collections::HashSet;
use std::future::Future;
use std::hash::Hash;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Error types for terminal operators
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowError {
    /// Flow completed without emitting any values
    Empty,
    /// Flow emitted more than one value when exactly one was expected
    MoreThanOneElement,
}

impl std::fmt::Display for FlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlowError::Empty => write!(f, "Flow completed without emitting any values"),
            FlowError::MoreThanOneElement => {
                write!(f, "Flow emitted more than one value when exactly one was expected")
            }
        }
    }
}

impl std::error::Error for FlowError {}

/// Terminal operators for Flow
///
/// These operators consume the flow and produce a final result.
pub trait FlowTerminal<T>: Sized
where
    T: Send + 'static,
{
    /// Collect all values and return the first one.
    /// Returns `Err(FlowError::Empty)` if the flow completes without emitting.
    ///
    /// # Example
    /// ```ignore
    /// let first = flow.first().await?;
    /// ```
    fn first(self) -> impl Future<Output = Result<T, FlowError>> + Send;

    /// Collect all values and return the first one, or `None` if empty.
    ///
    /// # Example
    /// ```ignore
    /// let first = flow.first_or_none().await;
    /// ```
    fn first_or_none(self) -> impl Future<Output = Option<T>> + Send;

    /// Ensure the flow emits exactly one value and return it.
    /// Returns `Err(FlowError::Empty)` if empty, `Err(FlowError::MoreThanOneElement)` if more than one.
    ///
    /// # Example
    /// ```ignore
    /// let single = flow.single().await?;
    /// ```
    fn single(self) -> impl Future<Output = Result<T, FlowError>> + Send;

    /// Return the single value if exactly one, or `None` otherwise.
    ///
    /// # Example
    /// ```ignore
    /// let single = flow.single_or_none().await;
    /// ```
    fn single_or_none(self) -> impl Future<Output = Option<T>> + Send;

    /// Collect all values and return the last one, or `None` if empty.
    ///
    /// # Example
    /// ```ignore
    /// let last = flow.last_or_none().await;
    /// ```
    fn last_or_none(self) -> impl Future<Output = Option<T>> + Send;

    /// Collect all values into a Vec.
    ///
    /// # Example
    /// ```ignore
    /// let list = flow.to_vec().await;
    /// ```
    fn to_vec(self) -> impl Future<Output = Vec<T>> + Send;

    /// Collect all values into a HashSet.
    ///
    /// # Example
    /// ```ignore
    /// let set = flow.to_set().await;
    /// ```
    fn to_set(self) -> impl Future<Output = HashSet<T>> + Send
    where
        T: Eq + Hash;

    /// Accumulate values using an initial value and an accumulator function.
    ///
    /// # Example
    /// ```ignore
    /// let sum = flow.fold(0, |acc, x| acc + x).await;
    /// ```
    fn fold<R, F>(self, initial: R, f: F) -> impl Future<Output = R> + Send
    where
        R: Send + 'static,
        F: FnMut(R, T) -> R + Send + 'static;

    /// Accumulate values without an initial value.
    /// Returns `Err(FlowError::Empty)` if the flow is empty.
    ///
    /// # Example
    /// ```ignore
    /// let sum = flow.reduce(|acc, x| acc + x).await?;
    /// ```
    fn reduce<F>(self, f: F) -> impl Future<Output = Result<T, FlowError>> + Send
    where
        F: FnMut(T, T) -> T + Send + 'static;

    /// Count the number of emitted values.
    ///
    /// # Example
    /// ```ignore
    /// let count = flow.count().await;
    /// ```
    fn count(self) -> impl Future<Output = usize> + Send;

    /// Check if any value matches the predicate.
    ///
    /// # Example
    /// ```ignore
    /// let has_even = flow.any(|x| x % 2 == 0).await;
    /// ```
    fn any<F>(self, predicate: F) -> impl Future<Output = bool> + Send
    where
        F: FnMut(&T) -> bool + Send + 'static;

    /// Check if all values match the predicate.
    ///
    /// # Example
    /// ```ignore
    /// let all_positive = flow.all(|x| x > 0).await;
    /// ```
    fn all<F>(self, predicate: F) -> impl Future<Output = bool> + Send
    where
        F: FnMut(&T) -> bool + Send + 'static;

    /// Check if no values match the predicate.
    ///
    /// # Example
    /// ```ignore
    /// let no_negative = flow.none(|x| x < 0).await;
    /// ```
    fn none<F>(self, predicate: F) -> impl Future<Output = bool> + Send
    where
        F: FnMut(&T) -> bool + Send + 'static;
}

impl<T> FlowTerminal<T> for Flow<T>
where
    T: Send + 'static,
{
    fn first(self) -> impl Future<Output = Result<T, FlowError>> + Send {
        async move {
            let result = Arc::new(Mutex::new(None::<T>));
            let result_clone = Arc::clone(&result);
            let found = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let found_clone = Arc::clone(&found);

            self.collect(move |value| {
                let result = Arc::clone(&result_clone);
                let found = Arc::clone(&found_clone);
                async move {
                    if !found.swap(true, std::sync::atomic::Ordering::SeqCst) {
                        *result.lock().await = Some(value);
                    }
                }
            })
            .await;

            let value = result.lock().await.take();
            value.ok_or(FlowError::Empty)
        }
    }

    fn first_or_none(self) -> impl Future<Output = Option<T>> + Send {
        async move { self.first().await.ok() }
    }

    fn single(self) -> impl Future<Output = Result<T, FlowError>> + Send {
        async move {
            let result = Arc::new(Mutex::new(None::<T>));
            let result_clone = Arc::clone(&result);
            let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let count_clone = Arc::clone(&count);

            self.collect(move |value| {
                let result = Arc::clone(&result_clone);
                let count = Arc::clone(&count_clone);
                async move {
                    let prev = count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if prev == 0 {
                        *result.lock().await = Some(value);
                    }
                }
            })
            .await;

            let final_count = count.load(std::sync::atomic::Ordering::SeqCst);
            if final_count == 0 {
                Err(FlowError::Empty)
            } else if final_count > 1 {
                Err(FlowError::MoreThanOneElement)
            } else {
                let value = result.lock().await.take();
                value.ok_or(FlowError::Empty)
            }
        }
    }

    fn single_or_none(self) -> impl Future<Output = Option<T>> + Send {
        async move { self.single().await.ok() }
    }

    fn last_or_none(self) -> impl Future<Output = Option<T>> + Send {
        async move {
            let result = Arc::new(Mutex::new(None::<T>));
            let result_clone = Arc::clone(&result);

            self.collect(move |value| {
                let result = Arc::clone(&result_clone);
                async move {
                    *result.lock().await = Some(value);
                }
            })
            .await;

            let value = result.lock().await.take();
            value
        }
    }

    fn to_vec(self) -> impl Future<Output = Vec<T>> + Send {
        async move {
            let result = Arc::new(Mutex::new(Vec::new()));
            let result_clone = Arc::clone(&result);

            self.collect(move |value| {
                let result = Arc::clone(&result_clone);
                async move {
                    result.lock().await.push(value);
                }
            })
            .await;

            Arc::try_unwrap(result)
                .ok()
                .map(|m| m.into_inner())
                .unwrap_or_default()
        }
    }

    fn to_set(self) -> impl Future<Output = HashSet<T>> + Send
    where
        T: Eq + Hash,
    {
        async move {
            let result = Arc::new(Mutex::new(HashSet::new()));
            let result_clone = Arc::clone(&result);

            self.collect(move |value| {
                let result = Arc::clone(&result_clone);
                async move {
                    result.lock().await.insert(value);
                }
            })
            .await;

            Arc::try_unwrap(result)
                .ok()
                .map(|m| m.into_inner())
                .unwrap_or_default()
        }
    }

    fn fold<R, F>(self, initial: R, f: F) -> impl Future<Output = R> + Send
    where
        R: Send + 'static,
        F: FnMut(R, T) -> R + Send + 'static,
    {
        async move {
            let acc = Arc::new(Mutex::new(Some(initial)));
            let f = Arc::new(Mutex::new(f));
            let acc_clone = Arc::clone(&acc);
            let f_clone = Arc::clone(&f);

            self.collect(move |value| {
                let acc = Arc::clone(&acc_clone);
                let f = Arc::clone(&f_clone);
                async move {
                    let mut acc_guard = acc.lock().await;
                    let mut f_guard = f.lock().await;
                    if let Some(current) = acc_guard.take() {
                        *acc_guard = Some(f_guard(current, value));
                    }
                }
            })
            .await;

            let value = acc.lock().await.take();
            value.expect("fold accumulator missing")
        }
    }

    fn reduce<F>(self, f: F) -> impl Future<Output = Result<T, FlowError>> + Send
    where
        F: FnMut(T, T) -> T + Send + 'static,
    {
        async move {
            let acc = Arc::new(Mutex::new(None::<T>));
            let f = Arc::new(Mutex::new(f));
            let acc_clone = Arc::clone(&acc);
            let f_clone = Arc::clone(&f);

            self.collect(move |value| {
                let acc = Arc::clone(&acc_clone);
                let f = Arc::clone(&f_clone);
                async move {
                    let mut acc_guard = acc.lock().await;
                    let mut f_guard = f.lock().await;
                    *acc_guard = Some(match acc_guard.take() {
                        None => value,
                        Some(current) => f_guard(current, value),
                    });
                }
            })
            .await;

            let value = acc.lock().await.take();
            value.ok_or(FlowError::Empty)
        }
    }

    fn count(self) -> impl Future<Output = usize> + Send {
        async move {
            let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let count_clone = Arc::clone(&count);

            self.collect(move |_| {
                let count = Arc::clone(&count_clone);
                async move {
                    count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            })
            .await;

            count.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    fn any<F>(self, predicate: F) -> impl Future<Output = bool> + Send
    where
        F: FnMut(&T) -> bool + Send + 'static,
    {
        async move {
            let found = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let predicate = Arc::new(Mutex::new(predicate));
            let found_clone = Arc::clone(&found);
            let predicate_clone = Arc::clone(&predicate);

            self.collect(move |value| {
                let found = Arc::clone(&found_clone);
                let predicate = Arc::clone(&predicate_clone);
                async move {
                    if !found.load(std::sync::atomic::Ordering::SeqCst) {
                        if predicate.lock().await(&value) {
                            found.store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                    }
                }
            })
            .await;

            found.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    fn all<F>(self, predicate: F) -> impl Future<Output = bool> + Send
    where
        F: FnMut(&T) -> bool + Send + 'static,
    {
        async move {
            let all_match = Arc::new(std::sync::atomic::AtomicBool::new(true));
            let predicate = Arc::new(Mutex::new(predicate));
            let all_match_clone = Arc::clone(&all_match);
            let predicate_clone = Arc::clone(&predicate);

            self.collect(move |value| {
                let all_match = Arc::clone(&all_match_clone);
                let predicate = Arc::clone(&predicate_clone);
                async move {
                    if all_match.load(std::sync::atomic::Ordering::SeqCst) {
                        if !predicate.lock().await(&value) {
                            all_match.store(false, std::sync::atomic::Ordering::SeqCst);
                        }
                    }
                }
            })
            .await;

            all_match.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    fn none<F>(self, predicate: F) -> impl Future<Output = bool> + Send
    where
        F: FnMut(&T) -> bool + Send + 'static,
    {
        async move { !self.any(predicate).await }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::flow;

    #[tokio::test]
    async fn test_first() {
        let numbers = flow(|c| async move {
            c.emit(1).await;
            c.emit(2).await;
            c.emit(3).await;
        });

        let first = numbers.first().await.unwrap();
        assert_eq!(first, 1);
    }

    #[tokio::test]
    async fn test_first_empty() {
        let empty: Flow<i32> = flow(|_c| async move {});
        let result = empty.first().await;
        assert!(matches!(result, Err(FlowError::Empty)));
    }

    #[tokio::test]
    async fn test_single() {
        let single_flow = flow(|c| async move {
            c.emit(42).await;
        });

        let result = single_flow.single().await.unwrap();
        assert_eq!(result, 42);
    }

    #[tokio::test]
    async fn test_single_multiple() {
        let multiple = flow(|c| async move {
            c.emit(1).await;
            c.emit(2).await;
        });

        let result = multiple.single().await;
        assert!(matches!(result, Err(FlowError::MoreThanOneElement)));
    }

    #[tokio::test]
    async fn test_to_vec() {
        let numbers = flow(|c| async move {
            c.emit(1).await;
            c.emit(2).await;
            c.emit(3).await;
        });

        let vec = numbers.to_vec().await;
        assert_eq!(vec, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_fold() {
        let numbers = flow(|c| async move {
            for i in 1..=5 {
                c.emit(i).await;
            }
        });

        let sum = numbers.fold(0, |acc, x| acc + x).await;
        assert_eq!(sum, 15);
    }

    #[tokio::test]
    async fn test_reduce() {
        let numbers = flow(|c| async move {
            for i in 1..=5 {
                c.emit(i).await;
            }
        });

        let sum = numbers.reduce(|acc, x| acc + x).await.unwrap();
        assert_eq!(sum, 15);
    }

    #[tokio::test]
    async fn test_count() {
        let numbers = flow(|c| async move {
            for i in 1..=10 {
                c.emit(i).await;
            }
        });

        let count = numbers.count().await;
        assert_eq!(count, 10);
    }

    #[tokio::test]
    async fn test_any() {
        let numbers = flow(|c| async move {
            for i in 1..=5 {
                c.emit(i).await;
            }
        });

        let has_even = numbers.any(|x| *x % 2 == 0).await;
        assert!(has_even);
    }

    #[tokio::test]
    async fn test_all() {
        let numbers = flow(|c| async move {
            for i in [2, 4, 6, 8] {
                c.emit(i).await;
            }
        });

        let all_even = numbers.all(|x| *x % 2 == 0).await;
        assert!(all_even);
    }
}
