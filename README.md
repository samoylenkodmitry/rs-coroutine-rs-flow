# rs-coroutine / rs-flow

A Kotlin-inspired coroutine and Flow framework for Rust, bringing familiar concurrency patterns to Rust developers.

## Overview

This framework provides:

- **CoroutineScope**: Structured concurrency with lifecycle management
- **Dispatchers**: Context switching between different executors (like Kotlin's `withContext`)
- **Flow**: Cold streams with operators (`map`, `filter`, `flow_on`, etc.)
- **Suspending blocks**: Reusable async computations
- **Job management**: Hierarchical cancellation

## Project Structure

```
rs-coroutine/
├── rs_coroutine_core/  # Core coroutine runtime
│   ├── executor.rs     # Executor trait and implementations
│   ├── dispatcher.rs   # Dispatcher and context switching
│   ├── scope.rs        # CoroutineScope and launch/async_task
│   └── job.rs          # Job tree and cancellation
├── rs_flow/            # Flow implementation
│   ├── flow.rs         # Flow core type
│   ├── collector.rs    # FlowCollector
│   └── operators.rs    # Flow operators (map, filter, etc.)
└── examples/
    └── web_fetch.rs    # Web data fetching example
```

## Examples

### Web Fetch Example

The `web_fetch` example demonstrates real-world usage patterns:

1. **Simple web fetch** with dispatcher switching
2. **Parallel fetching** using `async_task`
3. **Combined operations** (fetching user + their posts)
4. **Flow processing** with operators
5. **Suspending blocks** for reusable computations

Run the example:

```bash
cargo run --example web_fetch
```

### Key Patterns

#### Simple Fetch with Context Switching

```rust
let post = scope
    .with_dispatcher(Dispatchers::io(), async {
        fetch_post(1).await
    })
    .await;
```

#### Parallel Operations

```rust
let user = scope.async_task(Dispatchers::io(), async { fetch_user(1).await });
let posts = scope.async_task(Dispatchers::io(), async { fetch_posts().await });

let (user, posts) = (user.await_value().await, posts.await_value().await);
```

#### Flow Processing

```rust
let flow = create_post_flow(post_ids);
let filtered = flow
    .filter(|post| async move { post.title.len() > 30 })
    .map(|post| async move { post.title.to_uppercase() });

filtered.collect(|title| async move {
    println!("{}", title);
}).await;
```

#### Suspending Blocks

```rust
let fetch_operation = suspend_block! {
    fetch_post(1).await.unwrap()
};

// Convert to flow (cold - only executes on collect)
let flow = fetch_operation.as_flow();
flow.collect(|post| async move {
    println!("Received: {}", post.title);
}).await;
```

## Features

### CoroutineScope

- `launch(fut)` - Fire and forget
- `async_task(dispatcher, fut)` - Returns `Deferred<T>` for parallel work
- `with_dispatcher(dispatcher, fut)` - Context switching (like Kotlin's `withContext`)

### Dispatchers

- `Dispatchers::main()` - Main dispatcher
- `Dispatchers::io()` - IO-bound operations
- `Dispatchers::default()` - Default dispatcher

### Flow

- **Cold streams**: Nothing happens until `collect()` is called
- **Operators**: `map`, `filter`, `take`, `flow_on`
- **Builder macros**: `flow_block!`, `suspend_block!`

## Design Philosophy

This framework follows Kotlin's coroutine model:

1. **Task-local context**: Suspend functions automatically access their scope
2. **Structured concurrency**: Child jobs are cancelled when parent cancels
3. **Cold Flows**: Flows are lazy and restart on each collection
4. **Explicit dispatching**: Use `with_dispatcher` to switch execution context

## Requirements

- Rust 1.75+
- Tokio runtime

## License

MIT
