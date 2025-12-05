pub mod builders;
pub mod combining;
pub mod flow;
pub mod hot_flow;
pub mod lifecycle;
pub mod macros;
pub mod operators;
pub mod suspending_ext;
pub mod terminal;

pub use flow::{flow as flow_fn, flow, Flow, FlowCollector};
pub use hot_flow::{SharedFlow, StateFlow};
pub use operators::FlowExt;
pub use suspending_ext::SuspendingExt;

// Terminal operators
pub use terminal::{FlowError, FlowTerminal};

// Flow builders
pub use builders::{
    channel_flow, empty_flow, flow_of, flow_of_one, flow_range, flow_range_inclusive,
    generate_flow, interval_flow, repeat_flow, IntoFlow,
};

// Lifecycle operators
pub use lifecycle::FlowLifecycle;

// Combining operators
pub use combining::{merge, FlowCombining};

// Re-export common items from rs_coroutine_core
pub use rs_coroutine_core::{
    get_current_scope, suspend_block, with_current_scope, CancelToken, CoroutineScope, Deferred,
    Dispatcher, Dispatchers, Executor, JobHandle, Suspending, TokioExecutor, CURRENT_SCOPE,
};

// Re-export scope module for macros
pub use rs_coroutine_core::scope;

// Re-export macros (they are already exported via #[macro_export])
// Macros: flow!, flow_of!, flow_range!, merge!, emit_to!, collect!,
// launch!, with_context!, coroutine_scope!, async_task!, state_flow!, shared_flow!, flow_ops!
