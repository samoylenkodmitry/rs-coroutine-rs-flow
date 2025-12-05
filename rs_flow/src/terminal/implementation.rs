use super::*;
use std::collections::HashSet;
use std::hash::Hash;
use std::sync::Arc;
use tokio::sync::Mutex;

impl<T> FlowTerminal<T> for Flow<T>
where
    T: Send + 'static,
{
    async fn first(self) -> Result<T, FlowError> {
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

    async fn first_or_none(self) -> Option<T> {
        self.first().await.ok()
    }

    async fn single(self) -> Result<T, FlowError> {
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

    async fn single_or_none(self) -> Option<T> {
        self.single().await.ok()
    }

    async fn last_or_none(self) -> Option<T> {
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

    async fn to_vec(self) -> Vec<T> {
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

    async fn to_set(self) -> HashSet<T>
    where
        T: Eq + Hash,
    {
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

    async fn fold<R, F>(self, initial: R, f: F) -> R
    where
        R: Send + 'static,
        F: FnMut(R, T) -> R + Send + 'static,
    {
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

    async fn reduce<F>(self, f: F) -> Result<T, FlowError>
    where
        F: FnMut(T, T) -> T + Send + 'static,
    {
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

    async fn count(self) -> usize {
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

    async fn any<F>(self, predicate: F) -> bool
    where
        F: FnMut(&T) -> bool + Send + 'static,
    {
        let found = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let predicate = Arc::new(Mutex::new(predicate));
        let found_clone = Arc::clone(&found);
        let predicate_clone = Arc::clone(&predicate);

        self.collect(move |value| {
            let found = Arc::clone(&found_clone);
            let predicate = Arc::clone(&predicate_clone);
            async move {
                if !found.load(std::sync::atomic::Ordering::SeqCst)
                    && predicate.lock().await(&value)
                {
                    found.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }
        })
        .await;

        found.load(std::sync::atomic::Ordering::SeqCst)
    }

    async fn all<F>(self, predicate: F) -> bool
    where
        F: FnMut(&T) -> bool + Send + 'static,
    {
        let all_match = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let predicate = Arc::new(Mutex::new(predicate));
        let all_match_clone = Arc::clone(&all_match);
        let predicate_clone = Arc::clone(&predicate);

        self.collect(move |value| {
            let all_match = Arc::clone(&all_match_clone);
            let predicate = Arc::clone(&predicate_clone);
            async move {
                if all_match.load(std::sync::atomic::Ordering::SeqCst)
                    && !predicate.lock().await(&value)
                {
                    all_match.store(false, std::sync::atomic::Ordering::SeqCst);
                }
            }
        })
        .await;

        all_match.load(std::sync::atomic::Ordering::SeqCst)
    }

    async fn none<F>(self, predicate: F) -> bool
    where
        F: FnMut(&T) -> bool + Send + 'static,
    {
        !self.any(predicate).await
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
