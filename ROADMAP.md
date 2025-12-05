# rs-coroutine/rs-flow Roadmap

This document compares our API with Kotlin's Flow/Coroutines and outlines the implementation roadmap.

## Current Status

### What We Have

#### Flow Builders
| Feature | Kotlin | rs-flow | Status |
|---------|--------|---------|--------|
| `flow { emit(x) }` | `flow { emit(x) }` | `flow! { emit!(x); }` | ✅ Done |
| `flowOf(1, 2, 3)` | `flowOf(vararg)` | - | ❌ Missing |
| `list.asFlow()` | `Iterable.asFlow()` | - | ❌ Missing |
| `suspend {}.asFlow()` | `suspend () -> T` | `Suspending.as_flow()` | ✅ Done |
| `channelFlow { send(x) }` | `channelFlow` | - | ❌ Missing |
| `callbackFlow { trySend(x) }` | `callbackFlow` | - | ❌ Missing |

#### Terminal Operators
| Feature | Kotlin | rs-flow | Status |
|---------|--------|---------|--------|
| `collect { }` | `collect(action)` | `collect(\|x\| async { })` | ✅ Done |
| `first()` | `first()` | - | ❌ Missing |
| `firstOrNull()` | `firstOrNull()` | - | ❌ Missing |
| `single()` | `single()` | - | ❌ Missing |
| `singleOrNull()` | `singleOrNull()` | - | ❌ Missing |
| `toList()` | `toList()` | - | ❌ Missing |
| `toSet()` | `toSet()` | - | ❌ Missing |
| `fold(init) { }` | `fold(initial, operation)` | - | ❌ Missing |
| `reduce { }` | `reduce(operation)` | - | ❌ Missing |
| `lastOrNull()` | `lastOrNull()` | - | ❌ Missing |
| `launchIn(scope)` | `launchIn(scope)` | - | ❌ Missing |
| `count()` | `count()` | - | ❌ Missing |

#### Intermediate Operators - Transformation
| Feature | Kotlin | rs-flow | Status |
|---------|--------|---------|--------|
| `map { }` | `map(transform)` | `map()`, `map_sync()` | ✅ Done |
| `mapNotNull { }` | `mapNotNull(transform)` | - | ❌ Missing |
| `transform { emit() }` | `transform(transform)` | - | ❌ Missing |

#### Intermediate Operators - Filtering
| Feature | Kotlin | rs-flow | Status |
|---------|--------|---------|--------|
| `filter { }` | `filter(predicate)` | `filter()`, `filter_sync()` | ✅ Done |
| `filterNot { }` | `filterNot(predicate)` | - | ❌ Missing |
| `filterNotNull()` | `filterNotNull()` | - | ❌ Missing |
| `distinctUntilChanged()` | `distinctUntilChanged()` | `distinct_until_changed()` | ✅ Done |
| `distinctUntilChangedBy { }` | `distinctUntilChangedBy(selector)` | `distinct_until_changed_by()` | ✅ Done |

#### Intermediate Operators - Size
| Feature | Kotlin | rs-flow | Status |
|---------|--------|---------|--------|
| `take(n)` | `take(count)` | `take()` | ✅ Done |
| `takeWhile { }` | `takeWhile(predicate)` | `take_while()` | ✅ Done |
| `drop(n)` | `drop(count)` | `drop_first()` | ✅ Done |
| `dropWhile { }` | `dropWhile(predicate)` | `drop_while()` | ✅ Done |

#### Intermediate Operators - Flattening
| Feature | Kotlin | rs-flow | Status |
|---------|--------|---------|--------|
| `flatMapConcat { }` | `flatMapConcat(transform)` | `flat_map()` | ✅ Done |
| `flatMapMerge { }` | `flatMapMerge(concurrency, transform)` | - | ❌ Missing |
| `flatMapLatest { }` | `flatMapLatest(transform)` | `flat_map_latest()` | ✅ Done |

#### Intermediate Operators - Combining
| Feature | Kotlin | rs-flow | Status |
|---------|--------|---------|--------|
| `combine(other) { }` | `combine(flow, transform)` | - | ❌ Missing |
| `zip(other) { }` | `zip(other, transform)` | - | ❌ Missing |
| `merge(flows)` | `merge(vararg flows)` | - | ❌ Missing |

#### Intermediate Operators - Buffering
| Feature | Kotlin | rs-flow | Status |
|---------|--------|---------|--------|
| `buffer(capacity)` | `buffer(capacity)` | `buffer()` | ✅ Done |
| `conflate()` | `conflate()` | - | ❌ Missing |

#### Intermediate Operators - Scanning
| Feature | Kotlin | rs-flow | Status |
|---------|--------|---------|--------|
| `scan(init) { }` | `scan(initial, operation)` | - | ❌ Missing |
| `runningFold(init) { }` | `runningFold(initial, operation)` | - | ❌ Missing |
| `runningReduce { }` | `runningReduce(operation)` | - | ❌ Missing |

#### Context & Lifecycle Operators
| Feature | Kotlin | rs-flow | Status |
|---------|--------|---------|--------|
| `flowOn(dispatcher)` | `flowOn(context)` | `flow_on()` | ✅ Done |
| `onStart { }` | `onStart(action)` | - | ❌ Missing |
| `onCompletion { }` | `onCompletion(action)` | - | ❌ Missing |
| `onEmpty { }` | `onEmpty(action)` | - | ❌ Missing |
| `onEach { }` | `onEach(action)` | `on_each()`, `on_each_async()` | ✅ Done |
| `catch { }` | `catch(action)` | - | ❌ Missing |
| `retry(times)` | `retry(retries, predicate)` | - | ❌ Missing |
| `retryWhen { }` | `retryWhen(predicate)` | - | ❌ Missing |
| `timeout(duration)` | `timeout(timeout)` | - | ❌ Missing |

#### Hot Flows
| Feature | Kotlin | rs-flow | Status |
|---------|--------|---------|--------|
| `MutableStateFlow(init)` | `MutableStateFlow(value)` | `StateFlow::new()` | ✅ Done |
| `stateFlow.value` | `.value` | `.get()` | ✅ Done |
| `stateFlow.emit(x)` | `.emit(value)` | `.set()`, `.emit()` | ✅ Done |
| `MutableSharedFlow()` | `MutableSharedFlow(replay, buffer)` | `SharedFlow::new(capacity)` | ✅ Done |
| `sharedFlow.emit(x)` | `.emit(value)` | `.emit()` | ✅ Done |
| `shareIn(scope)` | `shareIn(scope, started, replay)` | - | ❌ Missing |
| `stateIn(scope)` | `stateIn(scope, started, initial)` | - | ❌ Missing |

#### Coroutine Integration
| Feature | Kotlin | rs-flow | Status |
|---------|--------|---------|--------|
| Scope-aware collection | `scope.launch { flow.collect {} }` | Partial | 🔶 Partial |
| Cancellation support | Built-in | Partial | 🔶 Partial |
| Context propagation | Built-in | Via `flowOn` | 🔶 Partial |

---

## Implementation Roadmap

### Phase 1: Core Terminal Operators (High Priority)
These are essential for practical use of flows.

1. **`first()` / `firstOrNull()`** - Get first emitted value
2. **`toList()` / `toSet()`** - Collect all values into a collection
3. **`fold()` / `reduce()`** - Aggregate values
4. **`single()` / `singleOrNull()`** - Ensure exactly one value
5. **`lastOrNull()`** - Get last emitted value
6. **`count()`** - Count emitted values

### Phase 2: Flow Builders (High Priority)
Make it easy to create flows from various sources.

1. **`flowOf()`** - Create flow from fixed values
2. **`iter.as_flow()`** - Convert iterators to flows
3. **`channelFlow {}`** - Concurrent flow builder with `send()`

### Phase 3: Combining Operators (Medium Priority)
Enable working with multiple flows together.

1. **`combine()`** - Combine latest values from multiple flows
2. **`zip()`** - Pair values from two flows
3. **`merge()`** - Merge multiple flows into one

### Phase 4: Lifecycle & Error Handling (Medium Priority)
Production-ready error handling and lifecycle management.

1. **`catch { }`** - Handle upstream errors
2. **`onStart { }`** - Execute action when collection starts
3. **`onCompletion { }`** - Execute action when flow completes
4. **`onEmpty { }`** - Execute action if flow emits nothing
5. **`retry()` / `retryWhen()`** - Retry on failure

### Phase 5: Advanced Operators (Lower Priority)
Nice-to-have operators for advanced use cases.

1. **`transform { }`** - Flexible transformation with multiple emits
2. **`scan()` / `runningFold()`** - Emit intermediate accumulations
3. **`flatMapMerge()`** - Concurrent flat mapping
4. **`conflate()`** - Keep only latest value for slow collectors
5. **`timeout()`** - Timeout for slow flows
6. **`mapNotNull()` / `filterNotNull()`** - Null handling

### Phase 6: Sharing Operators (Lower Priority)
Convert cold flows to hot flows.

1. **`shareIn()`** - Share cold flow as SharedFlow
2. **`stateIn()`** - Share cold flow as StateFlow

---

## Design Decisions

### Rust-specific Adaptations

1. **Async closures**: Rust doesn't have native async closures, so we provide both async and sync (`_sync`) variants of operators.

2. **No exceptions**: Rust uses `Result<T, E>` instead of exceptions. Error handling will use `Result` types and the `catch` operator.

3. **Ownership**: Flows are `Clone` to allow multiple collections. Internal state uses `Arc` for shared ownership.

4. **Cancellation**: Uses `CancelToken` from coroutine scope rather than Kotlin's structured concurrency exceptions.

5. **No null**: Rust has `Option<T>` instead of null. Methods like `firstOrNull()` return `Option<T>`.

### Naming Conventions

| Kotlin | Rust | Reason |
|--------|------|--------|
| `dropWhile` | `drop_while` | Snake case convention |
| `flatMapConcat` | `flat_map` | Concat is default behavior |
| `distinctUntilChanged` | `distinct_until_changed` | Snake case |
| Suspend function | `async fn` | Rust async/await |

---

## API Examples

### Current API (What works today)

```rust
use rs_flow::{flow, Flow, FlowExt, StateFlow, Dispatchers};

// Create a flow
let numbers: Flow<i32> = flow! {
    for i in 1..=10 {
        emit!(i);
    }
};

// Transform and collect
numbers
    .filter_sync(|x| *x % 2 == 0)
    .map_sync(|x| x * x)
    .take(3)
    .collect(|x| async move {
        println!("Got: {}", x);
    })
    .await;

// StateFlow for state management
let state = StateFlow::new(0);
state.set(42);
println!("Value: {}", state.get());
```

### Target API (After roadmap completion)

```rust
use rs_flow::{flow, flowOf, Flow, FlowExt};

// Easy flow creation
let numbers = flowOf!(1, 2, 3, 4, 5);
let from_vec = vec![1, 2, 3].into_flow();

// Terminal operators
let first = numbers.clone().first().await?;
let list = numbers.clone().to_list().await;
let sum = numbers.clone().fold(0, |acc, x| acc + x).await;

// Combining flows
let combined = flow1.combine(flow2, |a, b| a + b);
let zipped = flow1.zip(flow2, |a, b| (a, b));
let merged = merge!(flow1, flow2, flow3);

// Error handling
let safe = flow
    .map_sync(|x| risky_operation(x)?)
    .catch(|e| emit!(default_value))
    .on_completion(|error| {
        if let Some(e) = error {
            log::error!("Failed: {}", e);
        }
    });

// Coroutine integration
coroutine_scope!(Dispatchers::main() => {
    let data = fetch_flow()
        .flow_on(Dispatchers::io())
        .first()
        .await?;

    update_ui(data);
});
```

---

## Contributing

When implementing new operators:

1. Follow existing patterns in `operators.rs`
2. Provide both async and sync variants where applicable
3. Add tests in the module's `#[cfg(test)]` section
4. Update this roadmap to mark features as complete
5. Add examples to `examples/kotlin_style_api.rs`

## Version Targets

- **v0.2.0**: Phase 1 + Phase 2 (Terminal operators + Flow builders)
- **v0.3.0**: Phase 3 + Phase 4 (Combining + Lifecycle)
- **v0.4.0**: Phase 5 + Phase 6 (Advanced + Sharing)
- **v1.0.0**: Full API parity with Kotlin Flow essentials
