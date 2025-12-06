# rs-coroutine/rs-flow specification

## 1. Goals

### Primary goals
- Let Kotlin/Android developers apply their coroutines and Flow mental model directly in Rust.
- Provide `CoroutineScope`, `Dispatchers`, `launch`, and `async`‑like primitives, plus a `with_dispatcher` function analogous to Kotlin's `withContext`.
- Offer cold flows via `Flow<T>` plus hot flows via `SharedFlow` and `StateFlow`.
- Hide as much runtime plumbing as possible through a task‑local coroutine context (`CURRENT_SCOPE`) and syntax sugar macros (`#[suspend]`, `suspend_block!`, `flow_block!`).

### Non‑goals
- Not a replacement for the standard `async`/`await` and `Future` traits or the `futures::Stream` ecosystem.
- Not a global “one true runtime.” The framework plugs into any executor (Tokio or a custom executor).
- No magical whole‑program compiler transform—use explicit `.await` at suspension points.

## 2. Crate layout

```
rs-coroutine/
  rs_coroutine_core/  // CoroutineScope, Dispatchers, Job, macros
  rs_flow/            // Flow, Suspending<T>, SharedFlow/StateFlow (published as `coroflow`)
```

- `rs_coroutine_core` implements the coroutine runtime and macros:
  - `CoroutineScope` type.
  - `Dispatcher` and `Executor` abstractions.
  - `Job` and cancellation tree.
  - Task‑local `CURRENT_SCOPE`.
  - The helper `with_current_scope`.
  - Macros: `#[suspend]`, `suspend_block!`.

- `coroflow`:
  - The core `Flow<T>` type and `FlowCollector<T>`.
  - `Suspending<T>` and `.as_flow()`.
  - Builders like `flow`, `flow_block!`.
  - Operators including `map`, `filter`, `flow_on`, `flat_map_latest`, `buffer`.
  - Hot flows: `SharedFlow<T>` and `StateFlow<T>` implemented over channels.

## 3. rs‑coroutine core

### 3.1 Executor & Dispatcher

Define a minimal `Executor` trait:

```rust
pub trait Executor: Send + Sync + 'static {
    fn spawn(&self, fut: impl Future<Output = ()> + Send + 'static);
}
```

Encapsulate an `Executor` in a `Dispatcher`:

```rust
#[derive(Clone)]
pub struct Dispatcher {
    inner: Arc<dyn Executor>,
}

impl Dispatcher {
    pub fn new(inner: Arc<dyn Executor>) -> Self;
    pub fn spawn(&self, fut: impl Future<Output = ()> + Send + 'static);
}
```

Provide a registry of dispatchers (`Dispatchers::main()`, `Dispatchers::io()`, etc.) mapping to underlying executors.

### 3.2 CoroutineScope & task‑local context

Use a task‑local `CURRENT_SCOPE` (e.g., via `tokio::task_local!`) so that each spawned coroutine has access to its current scope. A `CoroutineScope` stores:

- its `dispatcher`;
- a `JobHandle` for structured concurrency;
- a `CancelToken` for cooperative cancellation.

Install the current scope in each spawned task:

```rust
pub fn launch<F>(&self, fut: F) -> JobHandle
where
    F: Future<Output = ()> + Send + 'static,
{
    let scope = Arc::new(self.clone());
    let dispatcher = self.dispatcher.clone();
    let job = self.job.new_child();

    dispatcher.spawn(async move {
        CURRENT_SCOPE
            .scope(scope.clone(), async {
                if !job.is_cancelled() {
                    fut.await;
                }
            })
            .await;
    });

    job
}
```

Provide `with_current_scope` to retrieve the current scope and execute a closure with it:

```rust
pub fn with_current_scope<F, Fut, T>(f: F) -> impl Future<Output = T>
where
    F: FnOnce(&CoroutineScope) -> Fut,
    Fut: Future<Output = T>,
{
    async {
        CURRENT_SCOPE
            .with(|scope| f(scope))
            .await
    }
}
```

### 3.3 Job & cancellation

`JobHandle` tracks a node in a tree of coroutines, similar to Kotlin's `Job`. Child jobs are cancelled when the parent is cancelled. Each `JobHandle` supports:

- `cancel()`, `is_cancelled()`;
- `join()` to wait for completion.

### 3.4 Context switching with `with_dispatcher`

To emulate Kotlin’s `withContext`, provide:

```rust
impl CoroutineScope {
    pub async fn with_dispatcher<F, T>(
        &self,
        dispatcher: Dispatcher,
        fut: F,
    ) -> T
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let child_scope = Arc::new(CoroutineScope {
            dispatcher: dispatcher.clone(),
            job: self.job.new_child(),
            cancel_token: self.cancel_token.clone_child(),
        });

        dispatcher.spawn(async move {
            let res = CURRENT_SCOPE.scope(child_scope, async move {
                fut.await
            }).await;
            let _ = tx.send(res);
        });

        rx.await.expect("dispatcher dropped")
    }
}
```

This splits the work: the parent future remains on its dispatcher; the inner future runs on the new dispatcher.

### 3.5 Parallel work with `async_task`

Provide an `async_task` method returning a `Deferred<T>`:

```rust
pub struct Deferred<T> {
    rx: oneshot::Receiver<T>,
    job: JobHandle,
}

impl<T> Deferred<T> {
    pub async fn await(self) -> T { self.rx.await.expect("task dropped") }
    pub fn job(&self) -> &JobHandle { &self.job }
}

impl CoroutineScope {
    pub fn async_task<F, T>(
        &self,
        dispatcher: Dispatcher,
        fut: F,
    ) -> Deferred<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        // spawn child task on the given dispatcher and return a Deferred
        unimplemented!()
    }
}
```

This is akin to Kotlin's `async` function.

### 3.6 The `#[suspend]` attribute

The `#[suspend]` macro rewrites:

```rust
#[suspend]
async fn do_something() -> T {
    // body can call with_dispatcher, launch, etc.
}
```

into:

```rust
fn do_something() -> impl Future<Output = T> {
    with_current_scope(|scope| async move {
        // original body, with an implicit binding for `scope`
    })
}
```

Call sites use `do_something().await` normally.

### 3.7 Anonymous suspend blocks

Define a `Suspending<T>` wrapper:

```rust
pub struct Suspending<T> {
    invoke: Arc<dyn Fn() -> Pin<Box<dyn Future<Output = T> + Send>> + Send + Sync>,
}
```

A `suspend_block!` macro creates a `Suspending<T>` capturing a block to be run with the current scope.

## 4. rs‑flow: Flow model

### 4.1 `Flow<T>` core

Define `FlowCollector<T>` with an async `emit` method, and `Flow<T>` wrapping an async collect function. Collectors are suspending:

```rust
pub struct Flow<T> {
    collect_fn: Arc<dyn Fn(FlowCollector<T>) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>,
}

impl<T> Flow<T> {
    pub async fn collect<F, Fut>(&self, on_value: F)
    where
        F: FnMut(T) -> Fut,
        Fut: Future<Output = ()>,
    {
        // run collect_fn with a collector that calls on_value for each emission
    }
}
```

A flow does nothing until `collect` is called.

### 4.2 `Suspending<T>::as_flow`

Turning a suspending block into a flow:

```rust
impl<T> Suspending<T> {
    pub fn as_flow(self) -> Flow<T> {
        Flow::from_fn(move |collector| {
            let this = self.clone();
            async move {
                let value = this.call().await;
                collector.emit(value).await;
            }
        })
    }
}
```

Each collection re‑runs the suspending block and emits exactly one value.

### 4.3 Builders and operators

Provide:

- `flow` and `flow_block!` to build flows that can call `emit`.
- Standard operators (`map`, `filter`, `take`, `buffer`, `flat_map_latest`) implemented by wrapping upstream `collect`.
- `flow_on(dispatcher)` switches the upstream collection to a different dispatcher, analogous to Kotlin’s `flowOn`.

### 4.4 Hot flows: SharedFlow and StateFlow

Implement hot, multicasting flows on top of channels:

- `SharedFlow<T>` wraps a `broadcast::Sender<T>`. It has `emit()` and `as_flow()` producing a `Flow<T>` that emits each broadcast message to each subscriber.
- `StateFlow<T>` wraps a `watch::Sender<T>` storing the latest state. Its `as_flow()` first emits the current value then all subsequent updates.

## 5. Integration and usage example

### Compose‑style usage

A `CoroutineScope` exists for each UI component (via a `remember_coroutine_scope()` helper in Compose‑RS). Suspend functions and flows can call `scope.with_dispatcher` to jump between the UI dispatcher and background executors.

Example screen model:

```rust
#[suspend]
async fn load_user() -> User {
    scope.with_dispatcher(Dispatchers::io(), async {
        fetch_user_from_api().await
    }).await
}

#[suspend]
async fn user_screen_model(user_updates: SharedFlow<UserUpdate>) -> Flow<UserUiState> {
    let initial = suspend_block! {
        let user = load_user().await;
        UserUiState::Loaded(user)
    }.as_flow();

    let updates = user_updates.as_flow()
        .map(|update| async { UserUiState::Updated(update) });

    initial.merge(updates)
}
```

### Parallel work

Use `async_task` to start multiple background tasks and `join!` to await them in parallel:

```rust
#[suspend]
async fn load_screen() -> ScreenState {
    let user = scope.async_task(Dispatchers::io(), async { load_user().await });
    let feed = scope.async_task(Dispatchers::io(), async { load_feed().await });

    let (user, feed) = futures::join!(user.await(), feed.await());

    ScreenState { user, feed }
}
```

## 6. Summary of design choices

- **Task‑local scope** ensures suspend functions automatically reference the correct coroutine context, without explicit parameters.
- **Structured concurrency** via a `Job` tree and cancellation tokens matches Kotlin’s semantics.
- **`with_dispatcher`** implements context switching by spawning a child task on a different executor, bridging the result back through a channel.
- **Cold `Flow`** and hot `SharedFlow`/`StateFlow` mirror Kotlin’s Flow API but built on top of Rust futures and channels.
- **Operators** like `map` and `flow_on` work by wrapping the upstream `collect` call, preserving backpressure and context propagation.
