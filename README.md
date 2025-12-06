# rs-coroutine / rs-flow

A Kotlin-style coroutine and Flow framework for Rust, bringing familiar coroutine patterns to Rust developers who are accustomed to Kotlin's coroutines and Flow API.

## Overview

This project provides two main crates:

- **rs_coroutine_core**: Core coroutine runtime with structured concurrency, dispatchers, and task-local scopes
- **coroflow**: Cold and hot Flow implementations with operators, similar to Kotlin's Flow API

## Features

### rs_coroutine_core

- **CoroutineScope**: Manages the lifecycle of coroutines with structured concurrency
- **Dispatchers**: Pluggable executor abstraction (Main, IO, Default)
- **Job System**: Hierarchical cancellation with JobHandle
- **Context Switching**: `with_dispatcher` for switching execution contexts
- **Parallel Work**: `async_task` returning `Deferred<T>` for concurrent execution
- **Task-local Scope**: `CURRENT_SCOPE` provides ambient scope access
- **Suspending Blocks**: `suspend_block!` macro for creating reusable suspending computations

### coroflow

- **Cold Flows**: `Flow<T>` that only executes when collected
- **Hot Flows**: `SharedFlow<T>` and `StateFlow<T>` for multicasting values
- **Flow Builders**: `flow` function and `flow_block!` macro
- **Rich Operators**:
  - `map` - Transform emitted values
  - `filter` - Filter values based on predicate
  - `take` - Take only first N values
  - `buffer` - Buffer emissions with backpressure
  - `flow_on` - Switch upstream collection to different dispatcher
  - `flat_map_latest` - Map to flow, cancelling previous
- **Suspending Conversion**: Convert `Suspending<T>` to `Flow<T>` with `as_flow()`

## Quick Start

Add the dependencies to your `Cargo.toml`:

```toml
[dependencies]
rs_coroutine_core = "0.1.1"
coroflow = "0.1.1"
tokio = { version = "1.35", features = ["full"] }
```

### Basic Coroutine Scope

```rust
use rs_coroutine_core::{CoroutineScope, Dispatchers};

#[tokio::main]
async fn main() {
    let scope = CoroutineScope::new(Dispatchers::main());

    let job = scope.launch(async {
        println!("Hello from coroutine!");
    });

    job.join().await;
}
```

### Context Switching

```rust
use rs_coroutine_core::{CoroutineScope, Dispatchers};
use std::sync::Arc;

async fn fetch_data() -> String {
    // Simulate API call
    "data".to_string()
}

#[tokio::main]
async fn main() {
    let scope = Arc::new(CoroutineScope::new(Dispatchers::main()));

    let job = scope.launch(async move {
        // Switch to IO dispatcher for background work
        let data = scope.with_dispatcher(Dispatchers::io(), async {
            fetch_data().await
        }).await;

        println!("Received: {}", data);
    });

    job.join().await;
}
```

### Cold Flows

```rust
use coroflow::{flow, FlowExt};

#[tokio::main]
async fn main() {
    let numbers = flow(|collector| async move {
        for i in 1..=5 {
            collector.emit(i).await;
        }
    });

    numbers
        .map(|x| async move { x * 2 })
        .filter(|x| {
            let x = *x;
            async move { x > 5 }
        })
        .collect(|x| async move {
            println!("Value: {}", x);
        })
        .await;
}
```

### Hot Flows (SharedFlow and StateFlow)

```rust
use coroflow::{SharedFlow, StateFlow, FlowExt};
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() {
    // SharedFlow - multicast to multiple subscribers
    let shared = SharedFlow::new(16);

    let flow = shared.as_flow();
    tokio::spawn(async move {
        flow.take(3)
            .collect(|x| async move {
                println!("Subscriber: {}", x);
            })
            .await;
    });

    for i in 1..=3 {
        shared.emit(i);
        sleep(Duration::from_millis(100)).await;
    }

    // StateFlow - holds latest state
    let state = StateFlow::new(0);
    println!("Current state: {}", state.get());

    state.set(42);
    println!("Updated state: {}", state.get());
}
```

### Parallel Work with async_task

```rust
use rs_coroutine_core::{CoroutineScope, Dispatchers};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let scope = Arc::new(CoroutineScope::new(Dispatchers::main()));

    let task1 = scope.async_task(Dispatchers::io(), async {
        // Some work
        42
    });

    let task2 = scope.async_task(Dispatchers::io(), async {
        // More work
        "Hello"
    });

    let (result1, result2) = futures::join!(
        task1.await_result(),
        task2.await_result()
    );

    println!("Results: {} and {}", result1, result2);
}
```

### Suspending Blocks

```rust
use rs_coroutine_core::suspend_block;
use coroflow::SuspendingExt;

#[tokio::main]
async fn main() {
    let load_data = suspend_block! {
        // This block can be called multiple times
        println!("Loading...");
        "data".to_string()
    };

    // Convert to flow
    let flow = load_data.as_flow();

    // Each collection re-executes the block
    flow.collect(|data| async move {
        println!("Received: {}", data);
    }).await;
}
```

## Examples

Run the comprehensive example:

```bash
cargo run --example basic_usage
```

This demonstrates:
- Basic coroutine scope usage
- Context switching with dispatchers
- Parallel work with async_task
- Cold flows with operators
- SharedFlow and StateFlow
- Suspending blocks

## Architecture

### Task-Local Scope

The framework uses Tokio's `task_local!` to provide ambient access to the current `CoroutineScope`, eliminating the need to pass scope references explicitly.

### Structured Concurrency

Each `CoroutineScope` maintains a `Job` tree. When a parent scope is cancelled, all child jobs are automatically cancelled, preventing resource leaks.

### Dispatcher Abstraction

The `Dispatcher` wraps an `Executor` trait, allowing you to plug in any async runtime (Tokio by default) or custom executors.

## Specification

See [rs-coroutine-rs-flow-spec.md](rs-coroutine-rs-flow-spec.md) for the complete specification.

## Releases and crates.io publishing

- A GitHub Actions workflow builds and tests every push and pull request.
- Creating a tag that starts with `v` (for example `v0.1.1`) triggers a release build and publishes both crates to crates.io.
- The workflow expects a repository secret named `CRATES_IO_TOKEN` containing a crates.io API token with publish rights for
  `rs_coroutine_core` and `coroflow`. The Flow crate was renamed to **coroflow** to meet crates.io naming guidelines; be sure to depend on the new name when publishing.

Ensure the workspace version matches the tag before publishing.

## License

MIT OR Apache-2.0
