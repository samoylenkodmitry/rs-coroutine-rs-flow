# rs_coroutine_core

Core primitives for building coroutine-style asynchronous workflows in Rust with structured concurrency and dispatcher-aware execution.

## Features
- `CoroutineScope` for lifecycle-managed coroutines and hierarchical cancellation.
- Dispatcher abstraction to run work on `Tokio` executors (Main, IO, Default) or custom executors.
- `async_task` and `Deferred<T>` for parallel work with join-style awaiting.
- Task-local scope access via `CURRENT_SCOPE`.
- `suspend_block!` macro for reusable suspending computations.

## Installation
Add the crate to your project:

```toml
[dependencies]
rs_coroutine_core = "0.1.1"
tokio = { version = "1.35", features = ["full"] }
```

## Example
Launch a coroutine and switch dispatchers for blocking work:

```rust
use rs_coroutine_core::{CoroutineScope, Dispatchers};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let scope = Arc::new(CoroutineScope::new(Dispatchers::main()));

    let job = scope.launch(async move {
        let data = scope.with_dispatcher(Dispatchers::io(), async {
            // Background work
            "data".to_string()
        }).await;

        println!("Received: {}", data);
    });

    job.join().await;
}
```

For more examples, run `cargo run --example basic_usage` from the workspace root.

## License
MIT OR Apache-2.0
