use crate::flow::Flow;
use rs_coroutine_core::Dispatcher;
use std::future::Future;

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

mod implementation;
