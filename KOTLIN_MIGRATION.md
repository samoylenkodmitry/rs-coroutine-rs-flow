# Kotlin Coroutines to Rust Migration Guide

This guide provides a 1:1 mapping between Kotlin coroutines and the `rs-coroutine` / `coroflow` libraries to help Kotlin/Android developers transition to Rust.

## Table of Contents

- [Basic Coroutines](#basic-coroutines)
- [Cancellation](#cancellation)
- [Flows](#flows)
- [Structured Concurrency](#structured-concurrency)
- [Dispatchers](#dispatchers)
- [Common Patterns](#common-patterns)

## Basic Coroutines

### Creating and Launching Coroutines

**Kotlin:**
```kotlin
import kotlinx.coroutines.*

fun main() = runBlocking {
    val job = launch {
        delay(1000)
        println("Hello")
    }
    job.join()
}
```

**Rust:**
```rust
use coroflow::*;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() {
    let scope = Arc::new(CoroutineScope::new(Dispatchers::main()));
    let job = scope.launch(async {
        sleep(Duration::from_millis(1000)).await;
        println!("Hello");
    });
    job.join().await;
}
```

### Using coroutineScope

**Kotlin:**
```kotlin
suspend fun doWork() = coroutineScope {
    launch { task1() }
    launch { task2() }
}
```

**Rust:**
```rust
async fn do_work() {
    coroutine_scope!(Dispatchers::main() => {
        launch! {
            task1().await;
        };
        launch! {
            task2().await;
        };
    });
}
```

### Async/Await for Results

**Kotlin:**
```kotlin
suspend fun fetchData() = coroutineScope {
    val deferred = async {
        // Heavy computation
        42
    }
    deferred.await()
}
```

**Rust:**
```rust
async fn fetch_data() -> i32 {
    with_current_scope(|scope| async move {
        let deferred = scope.async_task(Dispatchers::io(), async {
            // Heavy computation
            42
        });
        deferred.await_result().await
    }).await
}
```

## Cancellation

### Hierarchical Cancellation

**Kotlin:**
```kotlin
val job = launch {
    val child = launch {
        delay(Long.MAX_VALUE)
    }
}
job.cancel() // Cancels child too
```

**Rust:**
```rust
let scope = Arc::new(CoroutineScope::new(Dispatchers::main()));
let job = scope.launch(async {
    let child_job = with_current_scope(|scope| async {
        scope.launch(async {
            sleep(Duration::from_secs(1000)).await;
        })
    }).await;
});
scope.cancel(); // Cancels all children
```

### Checking Cancellation

**Kotlin:**
```kotlin
suspend fun doWork() {
    while (isActive) {
        // Do work
        yield()
    }
}
```

**Rust:**
```rust
async fn do_work() {
    loop {
        // Check cancellation (macro)
        check_cancelled!();

        // Do work
        yield_now().await;
    }
}
```

### Cooperative Cancellation with ensureActive

**Kotlin:**
```kotlin
suspend fun processItems(items: List<Int>) {
    for (item in items) {
        ensureActive()
        process(item)
    }
}
```

**Rust:**
```rust
async fn process_items(items: Vec<i32>) {
    for item in items {
        check_cancellation()?;  // Or use check_cancelled!() macro
        process(item).await;
    }
}
```

### CancellationException Handling

**Kotlin:**
```kotlin
try {
    longRunningTask()
} catch (e: CancellationException) {
    // Cleanup
    throw e // Rethrow
}
```

**Rust:**
```rust
// Check cancellation and return early
if let Err(_) = check_cancellation() {
    // Cleanup
    return;
}
long_running_task().await;
```

## Flows

### Creating Flows

**Kotlin:**
```kotlin
val flow = flow {
    for (i in 1..3) {
        emit(i)
    }
}
```

**Rust:**
```rust
let flow = flow! {
    for i in 1..=3 {
        emit!(i);
    }
};
```

### Flow Operators

**Kotlin:**
```kotlin
flow
    .map { it * 2 }
    .filter { it > 5 }
    .take(3)
    .collect { value ->
        println(value)
    }
```

**Rust:**
```rust
flow
    .map_sync(|x| x * 2)
    .filter_sync(|x| *x > 5)
    .take(3)
    .collect(|value| async move {
        println!("{}", value);
    })
    .await;
```

### StateFlow

**Kotlin:**
```kotlin
val stateFlow = MutableStateFlow(0)
stateFlow.value = 42
stateFlow.collect { value ->
    println(value)
}
```

**Rust:**
```rust
let state_flow = StateFlow::new(0);
state_flow.set(42);
state_flow.as_flow().collect(|value| async move {
    println!("{}", value);
}).await;
```

### SharedFlow

**Kotlin:**
```kotlin
val sharedFlow = MutableSharedFlow<Int>()
sharedFlow.emit(42)
sharedFlow.collect { value ->
    println(value)
}
```

**Rust:**
```rust
let shared_flow = SharedFlow::new(16); // Buffer size
shared_flow.emit(42).await;
shared_flow.as_flow().collect(|value| async move {
    println!("{}", value);
}).await;
```

### flatMapLatest

**Kotlin:**
```kotlin
flow
    .flatMapLatest { value ->
        flow { emit(fetchData(value)) }
    }
    .collect { println(it) }
```

**Rust:**
```rust
flow
    .flat_map_latest(|value| async move {
        flow! {
            emit!(fetch_data(value).await);
        }
    })
    .collect(|value| async move {
        println!("{}", value);
    })
    .await;
```

### Combining Flows

**Kotlin:**
```kotlin
// Merge
merge(flow1, flow2)

// Zip
flow1.zip(flow2) { a, b -> a + b }

// Combine
combine(flow1, flow2) { a, b -> a + b }
```

**Rust:**
```rust
// Merge
merge!(flow1, flow2)

// Zip
flow1.zip(flow2, |a, b| a + b)

// Combine
flow1.combine(flow2, |a, b| a + b)
```

## Structured Concurrency

### Scope with Context

**Kotlin:**
```kotlin
withContext(Dispatchers.IO) {
    // IO work
}
```

**Rust:**
```rust
with_context!(Dispatchers::io() => {
    // IO work
});
```

### supervisorScope

**Kotlin:**
```kotlin
supervisorScope {
    launch {
        // Child failure doesn't cancel siblings
    }
}
```

**Rust:**
```rust
// Each launch creates independent job
let scope = Arc::new(CoroutineScope::new(Dispatchers::main()));
scope.launch(async {
    // Independent task
});
scope.launch(async {
    // Independent task
});
```

## Dispatchers

### Dispatcher Types

**Kotlin:**
```kotlin
Dispatchers.Main      // Main/UI thread
Dispatchers.IO        // IO operations
Dispatchers.Default   // CPU-intensive work
```

**Rust:**
```rust
Dispatchers::main()   // Main dispatcher (Tokio)
Dispatchers::io()     // IO dispatcher (Tokio)
// Custom dispatcher: Dispatcher::new(executor)
```

### Switching Dispatchers

**Kotlin:**
```kotlin
suspend fun fetchData() = withContext(Dispatchers.IO) {
    // IO work
    networkCall()
}
```

**Rust:**
```rust
async fn fetch_data() -> Result<Data> {
    with_current_scope(|scope| async move {
        scope.with_dispatcher(Dispatchers::io(), async {
            // IO work
            network_call().await
        }).await
    }).await
}
```

## Common Patterns

### Parallel Decomposition

**Kotlin:**
```kotlin
suspend fun loadData() = coroutineScope {
    val users = async { fetchUsers() }
    val posts = async { fetchPosts() }
    UserFeed(users.await(), posts.await())
}
```

**Rust:**
```rust
async fn load_data() -> UserFeed {
    with_current_scope(|scope| async move {
        let users = scope.async_task(Dispatchers::io(), async {
            fetch_users().await
        });
        let posts = scope.async_task(Dispatchers::io(), async {
            fetch_posts().await
        });

        UserFeed {
            users: users.await_result().await,
            posts: posts.await_result().await,
        }
    }).await
}
```

### Flow Collection with Lifecycle

**Kotlin:**
```kotlin
flow
    .onStart { println("Starting") }
    .onCompletion { println("Complete") }
    .collect { value -> println(value) }
```

**Rust:**
```rust
flow
    .on_start(|| async { println!("Starting"); })
    .on_completion(|| async { println!("Complete"); })
    .collect(|value| async move {
        println!("{}", value);
    })
    .await;
```

### Timeout

**Kotlin:**
```kotlin
withTimeout(1000) {
    longRunningTask()
}
```

**Rust:**
```rust
use tokio::time::timeout;

let result = timeout(Duration::from_millis(1000), long_running_task()).await;
match result {
    Ok(value) => println!("Success: {:?}", value),
    Err(_) => println!("Timeout"),
}
```

Or with Flow:
```rust
flow.with_timeout(Duration::from_millis(1000))
    .collect(|value| async move {
        println!("{}", value);
    })
    .await;
```

### Retry Logic

**Kotlin:**
```kotlin
flow
    .retry(3) { cause ->
        cause is IOException
    }
```

**Rust:**
```rust
flow
    .retry(3, |_| true)  // Retry on all errors
    .collect(|value| async move {
        println!("{}", value);
    })
    .await;
```

## Key Differences

### 1. Explicit Arc Wrapping
**Kotlin** automatically manages references, while **Rust** requires explicit `Arc` for shared ownership:

```rust
let scope = Arc::new(CoroutineScope::new(Dispatchers::main()));
let scope_clone = Arc::clone(&scope);
```

### 2. Async/Await Syntax
**Kotlin** uses `suspend` modifier, **Rust** uses `async` blocks and `.await`:

```kotlin
// Kotlin
suspend fun fetchData(): Data { }
```

```rust
// Rust
async fn fetch_data() -> Data { }
```

### 3. Type Annotations
**Rust** requires more explicit type annotations for closures:

```rust
// May need explicit types
flow.map(|x: i32| async move { x * 2 })

// Or use _sync for non-async operations
flow.map_sync(|x| x * 2)
```

### 4. Macro Usage
**Rust** uses macros for ergonomic APIs:

```rust
flow! { emit!(value); }          // Instead of flow { emit(value) }
launch! { task().await; }         // Instead of launch { task() }
check_cancelled!();               // Instead of ensureActive()
```

### 5. Borrowing and Lifetimes
**Rust's** ownership system requires cloning for move closures:

```rust
let counter = Arc::new(AtomicUsize::new(0));
let counter_clone = Arc::clone(&counter);
scope.launch(async move {
    counter_clone.fetch_add(1, Ordering::SeqCst);
});
```

## Best Practices

### 1. Always Use Scopes
Create a scope for structured concurrency:

```rust
let scope = Arc::new(CoroutineScope::new(Dispatchers::main()));
// Use scope for all coroutine operations
```

### 2. Check Cancellation in Loops
Add cooperative cancellation checks:

```rust
for item in items {
    check_cancelled!();
    process(item).await;
}
```

### 3. Use Task-Local Scopes
Leverage `CURRENT_SCOPE` for cleaner code:

```rust
launch! {
    // CURRENT_SCOPE is automatically available
}
```

### 4. Prefer _sync Variants
Use `map_sync` and `filter_sync` for non-async operations:

```rust
flow.map_sync(|x| x * 2)  // More efficient than map
```

### 5. Handle Cancellation Gracefully
Always cleanup on cancellation:

```rust
if check_cancellation().is_err() {
    // Cleanup resources
    return;
}
```

## Migration Checklist

- [ ] Replace `suspend fun` with `async fn`
- [ ] Replace `delay()` with `sleep().await`
- [ ] Replace `flow { emit() }` with `flow! { emit!() }`
- [ ] Replace `launch { }` with `launch! { }`
- [ ] Replace `isActive` with `check_cancellation().is_ok()`
- [ ] Replace `ensureActive()` with `check_cancellation()` or `check_cancelled!()` macro
- [ ] Add `Arc` wrappers for shared state
- [ ] Add `.await` after async operations
- [ ] Use `_sync` variants for synchronous operations
- [ ] Replace `MutableStateFlow` with `StateFlow::new()`
- [ ] Replace `MutableSharedFlow` with `SharedFlow::new()`

## Additional Resources

- [Kotlin Coroutines Guide](https://kotlinlang.org/docs/coroutines-guide.html)
- [Tokio Documentation](https://tokio.rs/)
- [rs-coroutine Examples](./examples/)
- [coroflow Tests](./rs_flow/tests/)

## Contributing

Found a missing pattern or have suggestions? Please open an issue or submit a PR!
