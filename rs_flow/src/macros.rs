/// Kotlin-like macros for rs-coroutine/rs-flow
///
/// This module provides ergonomic macros that eliminate boilerplate
/// and provide a Kotlin-like developer experience.
/// Creates a Flow with implicit collector access via emit!()
///
/// # Example
/// ```ignore
/// use coroflow::flow;
///
/// // Kotlin-like syntax
/// let numbers = flow! {
///     for i in 1..=5 {
///         emit!(i);
///     }
/// };
/// ```
#[macro_export]
macro_rules! flow {
    { $($body:tt)* } => {{
        $crate::flow::flow(|__collector__| async move {
            // Make collector available for emit! macro
            #[allow(unused_macros)]
            macro_rules! emit {
                ($value:expr) => {
                    __collector__.emit($value).await
                };
            }
            $($body)*
        })
    }};
}

/// Emit a value in a flow! block
///
/// This is a standalone version that works with an explicit collector.
/// Inside a flow! block, a local emit! macro is automatically available.
#[macro_export]
macro_rules! emit_to {
    ($collector:expr, $value:expr) => {
        $collector.emit($value).await
    };
}

/// Collect values from a flow with simplified syntax
///
/// # Examples
/// ```ignore
/// // Simple collection
/// collect!(my_flow, |x| {
///     println!("Got: {}", x);
/// });
/// ```
#[macro_export]
macro_rules! collect {
    // Sync body - auto-wrap in async
    ($flow:expr, |$x:ident| $body:expr) => {
        $flow.collect(|$x| async move { $body }).await
    };
    // Explicit async body
    ($flow:expr, |$x:ident| async $body:block) => {
        $flow.collect(|$x| async move $body).await
    };
    // Async move body
    ($flow:expr, |$x:ident| async move $body:block) => {
        $flow.collect(|$x| async move $body).await
    };
}

/// Launch a coroutine on the current scope
///
/// # Examples
/// ```ignore
/// // Launch on current scope
/// launch! { println!("Hello from coroutine!"); }
///
/// // Launch on specific scope
/// launch!(scope => { work().await; });
/// ```
#[macro_export]
macro_rules! launch {
    // Launch on current scope (must be in scope context)
    { $($body:tt)* } => {{
        $crate::scope::get_current_scope().launch(async move {
            $($body)*
        })
    }};
    // Launch on explicit scope
    ($scope:expr => { $($body:tt)* }) => {{
        $scope.launch(async move {
            $($body)*
        })
    }};
    // Launch on explicit scope with async block
    ($scope:expr => async { $($body:tt)* }) => {{
        $scope.launch(async move {
            $($body)*
        })
    }};
}

/// Switch context to a different dispatcher
///
/// # Example
/// ```ignore
/// let result = with_context!(Dispatchers::io() => { fetch_data().await });
/// ```
#[macro_export]
macro_rules! with_context {
    // Using current scope
    ($dispatcher:expr => { $($body:tt)* }) => {{
        $crate::scope::get_current_scope()
            .with_dispatcher($dispatcher, async move {
                $($body)*
            })
            .await
    }};
    // Using explicit scope
    ($scope:expr, $dispatcher:expr => { $($body:tt)* }) => {{
        $scope
            .with_dispatcher($dispatcher, async move {
                $($body)*
            })
            .await
    }};
}

/// Create a coroutine scope and run the block within it
///
/// # Example
/// ```ignore
/// coroutine_scope!(Dispatchers::main() => { work().await; });
/// ```
#[macro_export]
macro_rules! coroutine_scope {
    // With explicit dispatcher
    ($dispatcher:expr => { $($body:tt)* }) => {{
        let __scope__ = std::sync::Arc::new($crate::CoroutineScope::new($dispatcher));
        let __scope_clone__ = std::sync::Arc::clone(&__scope__);
        __scope__.launch(async move {
            // Make scope available via CURRENT_SCOPE
            let _ = __scope_clone__;
            $($body)*
        }).join().await
    }};
    // With default dispatcher
    { $($body:tt)* } => {{
        $crate::coroutine_scope!($crate::Dispatchers::default() => { $($body)* })
    }};
}

/// Create an async task that returns a Deferred
///
/// # Example
/// ```ignore
/// let deferred = async_task!(Dispatchers::io() => { compute().await });
/// ```
#[macro_export]
macro_rules! async_task {
    // Using current scope
    ($dispatcher:expr => { $($body:tt)* }) => {{
        $crate::scope::get_current_scope()
            .async_task($dispatcher, async move {
                $($body)*
            })
    }};
    // Using explicit scope
    ($scope:expr, $dispatcher:expr => { $($body:tt)* }) => {{
        $scope
            .async_task($dispatcher, async move {
                $($body)*
            })
    }};
}

/// Create a StateFlow with initial value
///
/// # Example
/// ```rust
/// use coroflow::state_flow;
///
/// let counter = state_flow!(0);
/// counter.set(1);
/// let value = counter.get();
/// ```
#[macro_export]
macro_rules! state_flow {
    ($initial:expr) => {
        $crate::StateFlow::new($initial)
    };
}

/// Create a SharedFlow with capacity
///
/// # Example
/// ```rust
/// use coroflow::shared_flow;
///
/// let events = shared_flow!(16);
/// events.emit("event");
/// ```
#[macro_export]
macro_rules! shared_flow {
    ($capacity:expr) => {
        $crate::SharedFlow::new($capacity)
    };
    () => {
        $crate::SharedFlow::new(16)
    };
}

/// Apply multiple operators to a flow in a pipeline
///
/// # Example
/// ```ignore
/// let result = flow_ops!(numbers => map_sync |x| x * 2, take 3);
/// ```
#[macro_export]
macro_rules! flow_ops {
    ($flow:expr => $($op:ident $($args:tt)*),+ $(,)?) => {{
        let __flow__ = $flow;
        $(
            let __flow__ = $crate::flow_ops!(@apply __flow__, $op $($args)*);
        )+
        __flow__
    }};

    // Individual operator applications
    (@apply $flow:expr, map_sync |$x:ident| $body:expr) => {
        $crate::FlowExt::map_sync($flow, |$x| $body)
    };
    (@apply $flow:expr, filter_sync |$x:ident| $body:expr) => {
        $crate::FlowExt::filter_sync($flow, |$x| $body)
    };
    (@apply $flow:expr, take $n:expr) => {
        $crate::FlowExt::take($flow, $n)
    };
    (@apply $flow:expr, buffer $n:expr) => {
        $crate::FlowExt::buffer($flow, $n)
    };
    (@apply $flow:expr, flow_on $dispatcher:expr) => {
        $crate::FlowExt::flow_on($flow, $dispatcher)
    };
}

#[cfg(test)]
mod tests {
    use crate::{flow_fn, Flow, FlowExt, StateFlow};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_flow_macro() {
        let numbers: Flow<i32> = flow! {
            for i in 1..=3 {
                emit!(i);
            }
        };

        let collected = Arc::new(Mutex::new(Vec::new()));
        let collected_clone = Arc::clone(&collected);

        numbers
            .collect(move |x| {
                let collected = Arc::clone(&collected_clone);
                async move {
                    collected.lock().await.push(x);
                }
            })
            .await;

        let result = collected.lock().await;
        assert_eq!(*result, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_state_flow_get() {
        let state = StateFlow::new(0i32);
        assert_eq!(state.get(), 0);

        state.set(1);
        assert_eq!(state.get(), 1);

        state.set(42);
        assert_eq!(state.get(), 42);
    }

    #[tokio::test]
    async fn test_sync_operators() {
        let numbers: Flow<i32> = flow! {
            for i in 1..=10 {
                emit!(i);
            }
        };

        let collected = Arc::new(Mutex::new(Vec::new()));
        let collected_clone = Arc::clone(&collected);

        numbers
            .map_sync(|x| x * 2)
            .filter_sync(|x| *x > 5)
            .take(3)
            .collect(move |x| {
                let collected = Arc::clone(&collected_clone);
                async move {
                    collected.lock().await.push(x);
                }
            })
            .await;

        let result = collected.lock().await;
        assert_eq!(*result, vec![6, 8, 10]);
    }

    #[tokio::test]
    async fn test_take_operator() {
        let numbers: Flow<i32> = flow_fn(|collector| async move {
            for i in 1..=10 {
                collector.emit(i).await;
            }
        });

        let collected = Arc::new(Mutex::new(Vec::new()));
        let collected_clone = Arc::clone(&collected);

        numbers
            .take(3)
            .collect(move |x| {
                let collected = Arc::clone(&collected_clone);
                async move {
                    collected.lock().await.push(x);
                }
            })
            .await;

        let result = collected.lock().await;
        assert_eq!(*result, vec![1, 2, 3]);
    }
}
