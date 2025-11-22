use crate::flow::Flow;
use tokio::sync::{broadcast, watch};

/// A hot flow that multicasts values to all collectors
#[derive(Clone)]
pub struct SharedFlow<T>
where
    T: Clone + Send + 'static,
{
    tx: broadcast::Sender<T>,
}

impl<T> SharedFlow<T>
where
    T: Clone + Send + 'static,
{
    /// Create a new SharedFlow with the given capacity
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Emit a value to all subscribers
    pub fn emit(&self, value: T) {
        let _ = self.tx.send(value);
    }

    /// Convert to a cold Flow
    pub fn as_flow(&self) -> Flow<T> {
        let rx = self.tx.subscribe();
        Flow::new(move |collector| {
            let mut rx = rx.resubscribe();
            async move {
                while let Ok(value) = rx.recv().await {
                    collector.emit(value).await;
                }
            }
        })
    }

    /// Get the number of subscribers
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

/// A hot flow that holds and emits the latest state
#[derive(Clone)]
pub struct StateFlow<T>
where
    T: Clone + Send + Sync + 'static,
{
    tx: watch::Sender<T>,
}

impl<T> StateFlow<T>
where
    T: Clone + Send + Sync + 'static,
{
    /// Create a new StateFlow with an initial value
    pub fn new(initial: T) -> Self {
        let (tx, _) = watch::channel(initial);
        Self { tx }
    }

    /// Update the state
    pub fn emit(&self, value: T) {
        let _ = self.tx.send(value);
    }

    /// Set the state (alias for emit)
    pub fn set(&self, value: T) {
        self.emit(value);
    }

    /// Get the current state
    pub fn get(&self) -> T {
        self.tx.borrow().clone()
    }

    /// Get a reference to the current state
    pub fn borrow(&self) -> watch::Ref<'_, T> {
        self.tx.borrow()
    }

    /// Convert to a cold Flow that emits the current value and all updates
    pub fn as_flow(&self) -> Flow<T> {
        let rx = self.tx.subscribe();
        Flow::new(move |collector| {
            let mut rx = rx.clone();
            async move {
                // Emit the current value first
                let current = rx.borrow_and_update().clone();
                collector.emit(current).await;

                // Then emit all subsequent updates
                while rx.changed().await.is_ok() {
                    let value = rx.borrow_and_update().clone();
                    collector.emit(value).await;
                }
            }
        })
    }

    /// Get the number of subscribers
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}
