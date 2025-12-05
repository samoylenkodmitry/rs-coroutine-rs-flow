//! Terminal operators for Flow
//!
//! Terminal operators consume the flow and produce a final result.
//! They are all async functions that suspend until the flow completes.

use crate::flow::Flow;
use std::collections::HashSet;
use std::hash::Hash;

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
                write!(
                    f,
                    "Flow emitted more than one value when exactly one was expected"
                )
            }
        }
    }
}

impl std::error::Error for FlowError {}

/// Terminal operators for Flow
///
/// These operators consume the flow and produce a final result.
#[allow(async_fn_in_trait)]
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
    async fn first(self) -> Result<T, FlowError>;

    /// Collect all values and return the first one, or `None` if empty.
    ///
    /// # Example
    /// ```ignore
    /// let first = flow.first_or_none().await;
    /// ```
    async fn first_or_none(self) -> Option<T>;

    /// Ensure the flow emits exactly one value and return it.
    /// Returns `Err(FlowError::Empty)` if empty, `Err(FlowError::MoreThanOneElement)` if more than one.
    ///
    /// # Example
    /// ```ignore
    /// let single = flow.single().await?;
    /// ```
    async fn single(self) -> Result<T, FlowError>;

    /// Return the single value if exactly one, or `None` otherwise.
    ///
    /// # Example
    /// ```ignore
    /// let single = flow.single_or_none().await;
    /// ```
    async fn single_or_none(self) -> Option<T>;

    /// Collect all values and return the last one, or `None` if empty.
    ///
    /// # Example
    /// ```ignore
    /// let last = flow.last_or_none().await;
    /// ```
    async fn last_or_none(self) -> Option<T>;

    /// Collect all values into a Vec.
    ///
    /// # Example
    /// ```ignore
    /// let list = flow.to_vec().await;
    /// ```
    async fn to_vec(self) -> Vec<T>;

    /// Collect all values into a HashSet.
    ///
    /// # Example
    /// ```ignore
    /// let set = flow.to_set().await;
    /// ```
    async fn to_set(self) -> HashSet<T>
    where
        T: Eq + Hash;

    /// Accumulate values using an initial value and an accumulator function.
    ///
    /// # Example
    /// ```ignore
    /// let sum = flow.fold(0, |acc, x| acc + x).await;
    /// ```
    async fn fold<R, F>(self, initial: R, f: F) -> R
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
    async fn reduce<F>(self, f: F) -> Result<T, FlowError>
    where
        F: FnMut(T, T) -> T + Send + 'static;

    /// Count the number of emitted values.
    ///
    /// # Example
    /// ```ignore
    /// let count = flow.count().await;
    /// ```
    async fn count(self) -> usize;

    /// Check if any value matches the predicate.
    ///
    /// # Example
    /// ```ignore
    /// let has_even = flow.any(|x| x % 2 == 0).await;
    /// ```
    async fn any<F>(self, predicate: F) -> bool
    where
        F: FnMut(&T) -> bool + Send + 'static;

    /// Check if all values match the predicate.
    ///
    /// # Example
    /// ```ignore
    /// let all_positive = flow.all(|x| x > 0).await;
    /// ```
    async fn all<F>(self, predicate: F) -> bool
    where
        F: FnMut(&T) -> bool + Send + 'static;

    /// Check if no values match the predicate.
    ///
    /// # Example
    /// ```ignore
    /// let no_negative = flow.none(|x| x < 0).await;
    /// ```
    async fn none<F>(self, predicate: F) -> bool
    where
        F: FnMut(&T) -> bool + Send + 'static;
}

mod implementation;
