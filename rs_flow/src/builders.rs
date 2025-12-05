//! Flow builders for creating flows from various sources
//!
//! This module provides convenient ways to create flows from iterators,
//! fixed values, channels, and other sources.

use crate::flow::Flow;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Trait for converting types into Flow
pub trait IntoFlow<T> {
    /// Convert this type into a Flow
    fn into_flow(self) -> Flow<T>;
}

/// Convert an iterator into a Flow
impl<I, T> IntoFlow<T> for I
where
    I: IntoIterator<Item = T> + Clone + Send + Sync + 'static,
    I::IntoIter: Send,
    T: Send + 'static,
{
    fn into_flow(self) -> Flow<T> {
        Flow::new(move |collector| {
            let iter = self.clone().into_iter();
            async move {
                for item in iter {
                    collector.emit(item).await;
                }
            }
        })
    }
}

/// Create a flow that emits no values
pub fn empty_flow<T: Send + 'static>() -> Flow<T> {
    Flow::new(|_collector| async move {})
}

/// Create a flow from a single value
pub fn flow_of_one<T: Send + Clone + Sync + 'static>(value: T) -> Flow<T> {
    Flow::new(move |collector| {
        let value = value.clone();
        async move {
            collector.emit(value).await;
        }
    })
}

/// Create a flow from multiple values
pub fn flow_of<T, I>(values: I) -> Flow<T>
where
    I: IntoIterator<Item = T> + Clone + Send + Sync + 'static,
    I::IntoIter: Send,
    T: Send + 'static,
{
    values.into_flow()
}

/// Create a flow from a range
pub fn flow_range<T>(range: std::ops::Range<T>) -> Flow<T>
where
    T: Send + 'static,
    std::ops::Range<T>: Iterator<Item = T> + Clone + Send + Sync,
{
    range.into_flow()
}

/// Create a flow from a range (inclusive)
pub fn flow_range_inclusive<T>(range: std::ops::RangeInclusive<T>) -> Flow<T>
where
    T: Send + 'static,
    std::ops::RangeInclusive<T>: Iterator<Item = T> + Clone + Send + Sync,
{
    range.into_flow()
}

/// Create a flow that emits values from an async generator function.
/// This is like Kotlin's `channelFlow` - it allows concurrent emission.
///
/// # Example
/// ```ignore
/// let flow = channel_flow(|tx| async move {
///     tx.send(1).await;
///     tx.send(2).await;
/// });
/// ```
pub fn channel_flow<T, F, Fut>(builder: F) -> Flow<T>
where
    T: Send + 'static,
    F: Fn(mpsc::Sender<T>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let builder = Arc::new(builder);
    Flow::new(move |collector| {
        let builder = Arc::clone(&builder);
        async move {
            let (tx, mut rx) = mpsc::channel(16);

            let fut = builder(tx);
            let producer = tokio::spawn(fut);

            while let Some(value) = rx.recv().await {
                collector.emit(value).await;
            }

            let _ = producer.await;
        }
    })
}

/// Create a flow that generates values on demand.
/// The generator function is called for each value.
///
/// # Example
/// ```ignore
/// let flow = generate_flow(|| {
///     static COUNTER: AtomicUsize = AtomicUsize::new(0);
///     Some(COUNTER.fetch_add(1, Ordering::SeqCst))
/// });
/// ```
pub fn generate_flow<T, F>(generator: F) -> Flow<T>
where
    T: Send + 'static,
    F: Fn() -> Option<T> + Send + Sync + 'static,
{
    let generator = Arc::new(generator);
    Flow::new(move |collector| {
        let generator = Arc::clone(&generator);
        async move {
            while let Some(value) = generator() {
                collector.emit(value).await;
            }
        }
    })
}

/// Create a flow that repeats a value indefinitely.
/// Use with `take()` to limit the number of emissions.
///
/// # Example
/// ```ignore
/// let flow = repeat_flow(42).take(5); // Emits 42 five times
/// ```
pub fn repeat_flow<T: Clone + Send + Sync + 'static>(value: T) -> Flow<T> {
    Flow::new(move |collector| {
        let value = value.clone();
        async move {
            loop {
                collector.emit(value.clone()).await;
            }
        }
    })
}

/// Create a flow that emits values at a fixed interval.
///
/// # Example
/// ```ignore
/// let flow = interval_flow(Duration::from_secs(1)).take(5);
/// ```
pub fn interval_flow(period: std::time::Duration) -> Flow<u64> {
    Flow::new(move |collector| async move {
        let mut counter = 0u64;
        let mut interval = tokio::time::interval(period);

        loop {
            interval.tick().await;
            collector.emit(counter).await;
            counter += 1;
        }
    })
}

/// Macro to create a flow from a list of values (like Kotlin's flowOf)
///
/// # Example
/// ```ignore
/// let flow = flow_of!(1, 2, 3, 4, 5);
/// ```
#[macro_export]
macro_rules! flow_of {
    () => {
        $crate::builders::empty_flow()
    };
    ($($value:expr),+ $(,)?) => {{
        $crate::builders::flow_of([$($value),+])
    }};
}

/// Macro to create a flow from a range
///
/// # Example
/// ```ignore
/// let flow = flow_range!(1..=10);
/// ```
#[macro_export]
macro_rules! flow_range {
    ($range:expr) => {
        $crate::builders::IntoFlow::into_flow($range)
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::FlowTerminal;

    #[tokio::test]
    async fn test_into_flow_vec() {
        let flow = vec![1, 2, 3].into_flow();
        let result = flow.to_vec().await;
        assert_eq!(result, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_into_flow_range() {
        let flow = (1..=5).into_flow();
        let result = flow.to_vec().await;
        assert_eq!(result, vec![1, 2, 3, 4, 5]);
    }

    #[tokio::test]
    async fn test_flow_of() {
        let flow = flow_of([1, 2, 3]);
        let result = flow.to_vec().await;
        assert_eq!(result, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_empty_flow() {
        let flow: Flow<i32> = empty_flow();
        let result = flow.to_vec().await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_channel_flow() {
        let flow = channel_flow(|tx| async move {
            tx.send(1).await.ok();
            tx.send(2).await.ok();
            tx.send(3).await.ok();
        });

        let result = flow.to_vec().await;
        assert_eq!(result, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_flow_of_macro() {
        let flow = flow_of!(1, 2, 3);
        let result = flow.to_vec().await;
        assert_eq!(result, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_repeat_flow() {
        use crate::operators::FlowExt;

        let flow = repeat_flow(42).take(3);
        let result = flow.to_vec().await;
        assert_eq!(result, vec![42, 42, 42]);
    }
}
