# Next Tasks for rs-flow

## Recently Completed

The following features have been implemented and are working:

### Terminal Operators (`terminal.rs`)
- `first()`, `first_or_none()` - Get first value
- `single()`, `single_or_none()` - Expect exactly one value
- `last_or_none()` - Get last value
- `to_vec()`, `to_set()` - Collect into collections
- `fold()`, `reduce()` - Aggregate values
- `count()`, `any()`, `all()`, `none()` - Boolean/count checks

### Flow Builders (`builders.rs`)
- `IntoFlow` trait - `vec![1,2,3].into_flow()`
- `flow_of()`, `flow_of!()` macro
- `channel_flow()` - Concurrent emission
- `generate_flow()`, `repeat_flow()`, `interval_flow()`

### Lifecycle Operators (`lifecycle.rs`)
- `on_start()`, `on_completion()`, `on_empty()`
- `catch_panic()`, `retry()`, `with_timeout()`

### Combining Operators (`combining.rs`)
- `combine()`, `zip()`, `sample()`
- `concat()`, `start_with()`
- `merge()` function and `merge!()` macro

---

## Next Priority Tasks

### 1. Error Handling with Result Types (High Priority)
Currently `catch_panic` catches panics, but we need proper error handling for flows that emit `Result<T, E>`.

```rust
// Target API
flow
    .map_sync(|x| fallible_operation(x)) // Returns Result<T, E>
    .catch(|error| {
        log::warn!("Error: {}", error);
        emit!(default_value)
    })
    .collect(...)
```

**Tasks:**
- [ ] Add `FlowResult<T, E>` type alias
- [ ] Add `catch()` operator for Result-based error handling
- [ ] Add `map_result()` / `filter_result()` operators
- [ ] Add `unwrap_or()` / `unwrap_or_else()` terminal operators

### 2. Scanning/Accumulating Operators (Medium Priority)
Emit intermediate values during accumulation (useful for running totals, state machines).

```rust
// Target API
flow_of!(1, 2, 3, 4, 5)
    .scan(0, |acc, x| acc + x)
    .collect(...) // Emits: 1, 3, 6, 10, 15
```

**Tasks:**
- [ ] Add `scan()` operator (like `fold` but emits each step)
- [ ] Add `running_fold()` alias
- [ ] Add `running_reduce()` variant

### 3. Transform Operator (Medium Priority)
Flexible transformation that can emit zero, one, or multiple values per input.

```rust
// Target API
flow_of!(1, 2, 3)
    .transform(|value, emit| async move {
        if value > 1 {
            emit(value).await;
            emit(value * 10).await;
        }
    })
    .collect(...) // Emits: 2, 20, 3, 30
```

**Tasks:**
- [ ] Add `transform()` operator with multi-emit capability
- [ ] Add `transform_sync()` variant

### 4. Conflate Operator (Medium Priority)
Keep only the latest value when collector is slow (backpressure handling).

```rust
// Target API
fast_producer
    .conflate() // Drop intermediate values if consumer is slow
    .collect(|x| async move {
        slow_processing(x).await;
    })
```

**Tasks:**
- [ ] Add `conflate()` operator
- [ ] Consider `collectLatest()` as alternative pattern

### 5. FlatMapMerge (Medium Priority)
Concurrent flat mapping with configurable concurrency.

```rust
// Target API
user_ids
    .flat_map_merge(4, |id| fetch_user(id)) // Up to 4 concurrent fetches
    .collect(...)
```

**Tasks:**
- [ ] Add `flat_map_merge()` with concurrency parameter
- [ ] Add `flat_map_merge_sync()` variant

### 6. Sharing Operators (Lower Priority)
Convert cold flows to hot flows for multiple subscribers.

```rust
// Target API
let shared = cold_flow
    .share_in(scope, SharingStarted::Eagerly, replay = 1);

let state = cold_flow
    .state_in(scope, SharingStarted::Lazily, initial_value);
```

**Tasks:**
- [ ] Add `share_in()` - Convert to SharedFlow
- [ ] Add `state_in()` - Convert to StateFlow
- [ ] Add `SharingStarted` enum (Eagerly, Lazily, WhileSubscribed)

### 7. LaunchIn Operator (Lower Priority)
Launch flow collection in a coroutine scope.

```rust
// Target API
flow
    .on_each(|x| println!("{}", x))
    .launch_in(scope);
```

**Tasks:**
- [ ] Add `launch_in()` operator
- [ ] Integrate with CoroutineScope lifecycle

---

## Bug Fixes & Improvements

### Code Quality
- [ ] Update ROADMAP.md to reflect completed work
- [ ] Add more comprehensive doc examples
- [ ] Improve error messages

### Performance
- [ ] Profile `take()` operator with large counts
- [ ] Consider lock-free alternatives for terminal operators
- [ ] Optimize channel buffer sizes

### Testing
- [ ] Add stress tests for concurrent operators
- [ ] Add tests for error propagation
- [ ] Add benchmarks comparing to Kotlin Flow performance

---

## Suggested Order of Implementation

1. **Error handling** - Essential for production use
2. **scan()** - Common pattern, builds on existing `fold`
3. **transform()** - Enables many other patterns
4. **conflate()** - Important for UI/reactive scenarios
5. **flat_map_merge()** - Common async pattern
6. **share_in/state_in** - Hot flow conversion
7. **launch_in** - Ergonomic collection

---

## Notes for Contributors

When implementing:

1. Follow patterns in existing modules (`terminal.rs`, `lifecycle.rs`, etc.)
2. Provide both async and `_sync` variants where the closure could be sync
3. Add unit tests in the module's `#[cfg(test)]` section
4. Update ROADMAP.md status when complete
5. Add usage example to `examples/kotlin_style_api.rs`
6. Ensure all operators work with the `take()` cancellation pattern
