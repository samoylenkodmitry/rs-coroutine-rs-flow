# coroflow

Composable Flow utilities inspired by Kotlin's Flow API, built on top of `rs_coroutine_core` for coroutine-style structured concurrency in Rust.

## Features
- Cold `Flow<T>` builders that only execute when collected.
- Hot `SharedFlow<T>` and `StateFlow<T>` for broadcasting values.
- Rich operators including `map`, `filter`, `take`, `buffer`, `flow_on`, and `flat_map_latest`.
- Conversion from suspending computations via `SuspendingExt::as_flow`.

## Installation
Add `coroflow` and its companion runtime to your `Cargo.toml`:

```toml
[dependencies]
rs_coroutine_core = "0.1.0"
coroflow = "0.1.0"
tokio = { version = "1.35", features = ["full"] }
```

## Example
Create and collect a simple flow with operators:

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

For more examples, run `cargo run --example basic_usage` from the workspace root.

## License
MIT OR Apache-2.0
