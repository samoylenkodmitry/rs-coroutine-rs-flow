Below is a **coding-agent-ready roadmap** for the existing `samoylenkodmitry/rs-coroutine-rs-flow` repo.

I inspected the current GitHub source, but I did not run a local build because the container could not resolve `github.com` for `git clone`. The roadmap is therefore based on source inspection of current `main`.

Important current anchors:

The workspace is still versioned `0.1.1`, uses edition `2021`, and has two members: `rs_coroutine_core` and `rs_flow`. The workspace dependency list includes Tokio, tokio-util, futures, and pin-project. ([GitHub][1])

`rs_flow` currently re-exports builders, combining, lifecycle, terminal, hot flow, macros, and coroutine-core primitives. It also currently exposes `Flow`, `FlowCollector`, `FlowStream`, `SharedFlow`, `StateFlow`, and `FlowExt`. ([GitHub][2])

The current `Flow<T>` is still a `Send + 'static` style type: `FlowFuture` is boxed with `+ Send`, `FlowCollector` stores an `Arc<dyn Fn... + Send + Sync>`, and `Flow<T>::new` requires `F: Send + Sync + 'static` and `Fut: Send + 'static`. ([GitHub][3])

The current ergonomic `FlowCollector::emit` intentionally ignores downstream early termination, and the source itself warns that this continues producing even after downstream operators like `take()` or `first()` stop. ([GitHub][3])

The current `spawn_in_scope` still clones the ambient scope cancellation token and `ScopeAwareHandle::cancel()` cancels that stored token, which means a child/operator cancel can cancel the parent scope instead of just the spawned task. This is the first thing to fix. ([GitHub][4])

The current `CoroutineScope.job` field is documented as “not actively used” and “vestigial,” so the repo has hierarchical cancellation tokens, but not yet a full Kotlin-like task nursery where parent scopes own and await all children. ([GitHub][5])

The current executor abstraction is explicitly Tokio-only, despite Kotlin-like dispatcher naming. The source says the trait is Tokio-specific and uses Tokio `JoinHandle` for panic detection; `Dispatchers::main()` and `Dispatchers::io()` both wrap the same `TokioExecutor`. ([GitHub][6])

Rust async closures and the `AsyncFn`, `AsyncFnMut`, and `AsyncFnOnce` traits are stable as of Rust 1.85, which makes the Kotlin-like `.map(async |x| ...)` goal realistic. ([Rust Blog][7]) Tokio’s `spawn_local` / `LocalSet` are the right tools for `!Send` local tasks, but Tokio documents that `tokio::spawn` from inside a `LocalSet` does not stay in the `LocalSet`, so local mode must use local spawning deliberately. ([Docs.rs][8])

---

# Target user-facing API

This is the API shape the roadmap should optimize for:

```rust
use coroflow::local::prelude::*;

#[coroflow::main(local)]
async fn main() -> Result<()> {
    coroutine_scope! {
        launch! {
            delay(500.millis()).await;
            println!("child finished");
        }

        let user = async_! {
            api.load_user(42).await?
        };

        flow! {
            emit(1);
            emit(2);
            delay(100.millis()).await;
            emit(3);
        }
        .map(async |x| x + 1)
        .filter(async |x| x % 2 == 0)
        .on_each(async |x| println!("value = {x}"))
        .collect()
        .await?;

        println!("user = {:?}", user.await?);
        Ok(())
    }
}
```

The `send` / cross-thread API should remain available, but it should be explicit:

```rust
use coroflow::send::prelude::*;

#[coroflow::main]
async fn main() -> Result<()> {
    let api = shared(api);

    flow_send! {
        emit(api.load_user(42).await?);
    }
    .flow_on(Dispatchers::io())
    .collect(async |user| println!("{user:?}"))
    .await?;

    Ok(())
}
```

---

# Non-negotiable design rules

* [ ] Keep the Kotlin-like API as the **primary public experience**.
* [ ] Keep raw Rust async plumbing out of normal user code: no visible `Pin<Box<dyn Future>>`, no visible `Arc<dyn Fn>`, no manual cancellation tokens, no manual collector objects.
* [ ] Keep `.await`; do not try to hide Rust suspension points globally.
* [ ] Make `local` mode the ergonomic default for Kotlin-like code.
* [ ] Make `send` mode explicit for cross-thread work.
* [ ] Do not make local flows require `Send`.
* [ ] Do not make local flows require `'static` unless the value is actually stored beyond the local lifetime.
* [ ] Do not expose `CancellationToken` in normal user-facing examples.
* [ ] Do not add new operators that spawn tasks until cancellation and cleanup invariants are tested.
* [ ] Do not use raw `tokio::spawn` inside flow operators without an owned cancellation/join guard.
* [ ] Do not rely on timing sleeps in correctness tests; use `Notify`, `Barrier`, `oneshot`, atomics, or explicit handshakes.
* [ ] Preserve `#![forbid(unsafe_code)]` unless there is an explicit, documented, reviewed decision to introduce a tiny unsafe-scoped-task layer.

---

# Phase 0 — Baseline, repo hygiene, and guardrails

## 0.1 Workspace and versioning

* [ ] Add `rust-version = "1.85"` to workspace package metadata, or choose a newer MSRV explicitly.
* [ ] Decide whether to move `workspace.package.edition` from `2021` to `2024`.
* [ ] If edition changes to `2024`, run `cargo fix --edition` and review generated changes manually.
* [ ] Add a `docs/API_TARGET.md` file containing the exact target API examples from this roadmap.
* [ ] Add a `docs/INVARIANTS.md` file documenting cancellation, structured concurrency, local/send split, and flow early-termination rules.
* [ ] Add a `docs/AGENT_RULES.md` file saying: “Do not add spawning operators without cancellation tests.”
* [ ] Update `ROADMAP.md` so it no longer claims existing exported builders such as `flow_of` and `channel_flow` are missing.
* [ ] Add `trybuild` as a dev-dependency for compile-pass and compile-fail API ergonomics tests.
* [ ] Add `futures-util` as a dev-dependency if needed for stream/operator tests.
* [ ] Add `tokio = { workspace = true, features = ["test-util"] }` only in dev-dependencies where needed.
* [ ] Add a CI job for `cargo fmt --all -- --check`.
* [ ] Add a CI job for `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
* [ ] Add a CI job for `cargo test --workspace --all-targets --all-features`.
* [ ] Add a CI job for `cargo test --doc --workspace --all-features`.
* [ ] Add a CI job for examples: `cargo run --example basic_usage`.
* [ ] Add a new example placeholder: `examples/kotlin_style_local.rs`.
* [ ] Add a new example placeholder: `examples/kotlin_style_send.rs`.
* [ ] Add a new example placeholder: `examples/cancellation_invariants.rs`.

## 0.2 Test harness helpers

* [ ] Create `rs_flow/tests/common/mod.rs`.
* [ ] Add a helper `run_scoped_test(async_block)` that creates a `CoroutineScope`, launches the block, waits for completion, and asserts the scope was not accidentally cancelled.
* [ ] Add a helper `assert_eventually_notified` using `tokio::time::timeout` only as a test failure bound, not as a scheduling mechanism.
* [ ] Add a helper `no_sleep_barrier()` using `Notify` or `Barrier` for deterministic task handshakes.
* [ ] Add a helper `AtomicDropCounter` for verifying producer cleanup.
* [ ] Add a helper `ProbeTask` that records `started`, `cancelled`, and `completed`.
* [ ] Add test documentation explaining that sleeps are allowed only as final timeout guards, never as correctness synchronization.

---

# Phase 1 — P0 cancellation correctness

This phase fixes the most dangerous bug before ergonomic work.

## 1.1 Fix `spawn_in_scope`

Current problem: `spawn_in_scope` stores `scope.cancel_token.clone()` in `ScopeAwareHandle`, then `cancel()` cancels that token. Because this is the ambient scope token, cancelling a flow operator task can cancel siblings and the parent scope. ([GitHub][4])

* [ ] Change `ScopeAwareHandle` to store only `job: JobHandle`.
* [ ] Remove `cancel_token: CancelToken` from `ScopeAwareHandle`.
* [ ] Change `spawn_in_scope` to return `ScopeAwareHandle { job }`.
* [ ] Change `ScopeAwareHandle::cancel(&self)` to call `self.job.cancel()`.
* [ ] Change `ScopeAwareHandle::cancel_and_join(self)` to call `self.job.cancel(); self.job.join().await`.
* [ ] Remove the unused `CancelToken` import from `rs_flow/src/internal_utils.rs`.
* [ ] Update comments in `internal_utils.rs` so they say child job cancellation, not ambient scope cancellation.
* [ ] Add a regression test named `spawn_in_scope_cancel_does_not_cancel_parent_scope`.
* [ ] Add a regression test named `cancel_on_drop_does_not_cancel_parent_scope`.
* [ ] Add a regression test named `flow_stream_drop_does_not_cancel_parent_scope`.
* [ ] Add a regression test named `take_one_does_not_cancel_sibling_task`.
* [ ] Add a regression test named `buffer_downstream_break_does_not_cancel_scope`.
* [ ] Add a regression test named `flat_map_latest_switch_does_not_cancel_scope`.
* [ ] Add a regression test named `channel_flow_downstream_break_does_not_cancel_scope`.
* [ ] Add a regression test named `merge_downstream_break_does_not_cancel_scope`.
* [ ] Ensure all new tests fail on old code and pass after the fix.
* [ ] Run `cargo test -p coroflow cancellation` and record passing output in PR notes.

## 1.2 Audit every spawn site

* [ ] Search the workspace for `tokio::spawn`.
* [ ] Search the workspace for `.spawn(`.
* [ ] Search the workspace for `spawn_in_scope`.
* [ ] Search the workspace for `into_cancel_on_drop`.
* [ ] Search the workspace for `AbortOnDrop`.
* [ ] Create `docs/SPAWN_AUDIT.md`.
* [ ] For each spawn site, document owner, cancellation path, join path, and drop behavior.
* [ ] Mark each spawn site as one of: `structured child`, `flow producer`, `observer`, `runtime integration`, or `test-only`.
* [ ] For each `flow producer` spawn site, add a test showing downstream early stop cancels producer.
* [ ] For each `observer` spawn site, document why it is allowed to complete naturally.
* [ ] For each unstructured spawn site, either wrap it in a guard or document why it cannot leak.
* [ ] Make `cargo clippy` pass after the audit.

## 1.3 Fix `channel_flow` cancellation on downstream break

Current code spawns the builder and then joins it after draining the receiver; if downstream breaks while the producer is infinite, a plain join can hang.

* [ ] Change `channel_flow` to hold `ScopeAwareHandle` rather than immediately converting to a passive join pattern.
* [ ] When downstream returns `Break`, call `producer.cancel_and_join().await`.
* [ ] When receiver closes naturally, call `producer.join().await`.
* [ ] Ensure `tx` is dropped before join where required to unblock sender loops.
* [ ] Add test `channel_flow_take_one_cancels_infinite_producer`.
* [ ] Add test `channel_flow_downstream_break_joins_producer`.
* [ ] Add test `channel_flow_finite_producer_completes_normally`.
* [ ] Add test `channel_flow_producer_panic_is_observable`.
* [ ] Update `channel_flow` docs with early-termination behavior.

## 1.4 Fix `flow_on` structured cleanup

Current `flow_on` uses dispatcher spawning but does not clearly own and cancel the producer on downstream termination.

* [ ] Rewrite `flow_on` so producer work is represented by a child job/handle.
* [ ] On downstream `Break`, cancel producer and join it.
* [ ] On receiver closure, join producer naturally.
* [ ] Ensure receiver drop causes upstream send to fail.
* [ ] Add test `flow_on_take_one_cancels_infinite_upstream`.
* [ ] Add test `flow_on_downstream_break_cancels_producer`.
* [ ] Add test `flow_on_does_not_cancel_parent_scope`.
* [ ] Add test `flow_on_propagates_panic_or_task_error`.
* [ ] Update docs to state `flow_on` is Tokio-only until dispatcher work is improved.

## 1.5 Fix `FlowStream` cleanup after `spawn_in_scope`

`FlowStreamGuard` currently delegates to `ScopeAwareHandle`, so the P0 fix should improve it. Still, test it directly.

* [ ] Add test `flow_stream_drop_cancels_collection_task`.
* [ ] Add test `flow_stream_cancel_and_join_waits_for_cleanup`.
* [ ] Add test `flow_stream_drop_does_not_cancel_sibling`.
* [ ] Add test `flow_stream_cancel_and_join_does_not_cancel_parent`.
* [ ] Ensure `FlowStream::cancel_and_join()` remains available for deterministic cleanup.
* [ ] Add documentation that plain drop cancels but cannot await completion.
* [ ] Add example showing explicit `cancel_and_join()` for deterministic switching.

---

# Phase 2 — Public module split: `local` and `send`

The core ergonomics move is to stop forcing every user-facing flow through `Send + Sync + 'static`.

## 2.1 Create module layout

* [ ] Add `rs_flow/src/local/mod.rs`.
* [ ] Add `rs_flow/src/local/flow.rs`.
* [ ] Add `rs_flow/src/local/collector.rs`.
* [ ] Add `rs_flow/src/local/operators.rs`.
* [ ] Add `rs_flow/src/local/builders.rs`.
* [ ] Add `rs_flow/src/local/terminal.rs`.
* [ ] Add `rs_flow/src/local/prelude.rs`.
* [ ] Add `rs_flow/src/send/mod.rs`.
* [ ] Add `rs_flow/src/send/prelude.rs`.
* [ ] Re-export the existing current `Flow<T>` stack from `send`.
* [ ] Add `pub mod local;` and `pub mod send;` in `rs_flow/src/lib.rs`.
* [ ] Do not remove the existing root exports yet.
* [ ] Add a deprecation plan for root-level exports, but do not break them in this phase.
* [ ] Add docs explaining: `local` is ergonomic, single-thread/local-lifetime; `send` is cross-thread and stricter.

## 2.2 Add boxed future aliases

* [ ] Add `rs_flow/src/future.rs`.
* [ ] Define `pub type LocalBoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>`.
* [ ] Define `pub type SendBoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>`.
* [ ] Re-export these aliases only from advanced/internal modules, not the normal prelude.
* [ ] Use `LocalBoxFuture` in `local` implementation.
* [ ] Use `SendBoxFuture` in `send` implementation.
* [ ] Add docs explaining why boxed futures are intentional for the Kotlin-like API.
* [ ] Add benchmark placeholder `benches/flow_boxing_overhead.rs` but do not optimize yet.

The `futures` crate documents `BoxFuture` as an owned dynamically typed future for cases where the concrete future type cannot or should not be named, which matches this ergonomic API strategy. ([Docs.rs][9])

## 2.3 Implement `LocalFlow<'a, T>`

* [ ] Define `pub struct LocalFlow<'a, T>`.
* [ ] Store a type-erased collect function using `Rc`, not `Arc`.
* [ ] Do not require `Send`.
* [ ] Do not require `Sync`.
* [ ] Do not require `'static`.
* [ ] Ensure `LocalFlow<'a, T>` can capture borrowed values with lifetime `'a`.
* [ ] Decide whether `LocalFlow` is cloneable in v0.2.
* [ ] If cloneable, document that clone means re-collecting the cold pipeline.
* [ ] If not cloneable initially, document that `LocalFlow` is single pipeline value and add cloning later.
* [ ] Implement `LocalFlow::new`.
* [ ] Implement `LocalFlow::from_fn`.
* [ ] Implement `LocalFlow::collect`.
* [ ] Implement `LocalFlow::collect_with_control` as internal/advanced.
* [ ] Implement `LocalFlow::to_stream_local` only if it can be cancellation-safe without requiring `Send`.
* [ ] Add test `local_flow_can_capture_borrowed_string`.
* [ ] Add test `local_flow_can_capture_borrowed_service_reference`.
* [ ] Add test `local_flow_does_not_require_send`.
* [ ] Add compile-pass test where `Rc<RefCell<_>>` is captured in a local flow.
* [ ] Add compile-fail test proving `Rc<RefCell<_>>` cannot be used in `send::Flow`.
* [ ] Add docs comparing `LocalFlow<'a, T>` and `send::Flow<T>`.

## 2.4 Implement `LocalFlowCollector<'a, T>`

* [ ] Define `LocalFlowCollector<'a, T>`.
* [ ] Store emit function using `Rc`.
* [ ] Use `LocalBoxFuture`.
* [ ] Add `emit_control(value) -> LocalBoxFuture<'a, ControlFlow<()>>`.
* [ ] Do not expose raw `ControlFlow` in normal `flow!` macro usage.
* [ ] Keep a low-level escape hatch for advanced operator authors.
* [ ] Add test `local_collector_propagates_break`.
* [ ] Add test `local_collector_emit_does_not_require_send`.
* [ ] Add test `local_collector_can_emit_borrowed_lifetime_values_if_type_allows`.

## 2.5 Keep current `Flow<T>` as send mode

* [ ] Create `pub type SendFlow<T> = crate::flow::Flow<T>` or move implementation into `send::Flow`.
* [ ] Export `send::Flow<T>` as the cross-thread flow type.
* [ ] Preserve old `coroflow::Flow<T>` temporarily for compatibility.
* [ ] Add `flow_send!` macro later for send mode.
* [ ] Add docs: send mode requires `Send + Sync + 'static` because it may cross thread boundaries.
* [ ] Add test `send_flow_still_compiles_old_examples`.
* [ ] Add test `send_flow_rejects_non_send_capture`.
* [ ] Add test `send_prelude_exports_existing_flow_ext`.

---

# Phase 3 — Local operators with async closures

## 3.1 Operator trait design

* [ ] Add `local::FlowExt` trait.
* [ ] Implement `map` using async closure-friendly bounds.
* [ ] Implement `filter` using async closure-friendly bounds.
* [ ] Implement `on_each` using async closure-friendly bounds.
* [ ] Implement `take`.
* [ ] Implement `take_while`.
* [ ] Implement `drop_first`.
* [ ] Implement `drop_while`.
* [ ] Implement `distinct_until_changed`.
* [ ] Implement `distinct_until_changed_by`.
* [ ] Implement `flat_map_concat`.
* [ ] Implement `flat_map_latest` for local mode.
* [ ] Add `map_sync` only as convenience, not as the preferred path.
* [ ] Add `filter_sync` only as convenience, not as the preferred path.
* [ ] Make `.map(async |x| ...)` compile in local examples.
* [ ] Make `.filter(async |x| ...)` compile in local examples.
* [ ] Make `.on_each(async |x| ...)` compile in local examples.
* [ ] Add compile-pass test `local_map_async_closure`.
* [ ] Add compile-pass test `local_filter_async_closure`.
* [ ] Add compile-pass test `local_on_each_async_closure`.
* [ ] Add compile-pass test `local_operator_captures_reference_without_arc`.
* [ ] Add compile-pass test `local_operator_captures_rc_refcell`.
* [ ] Add compile-fail test `send_operator_rejects_rc_refcell`.

## 3.2 Handle `AsyncFn` complexity

* [ ] Create a small spike branch or test module proving the desired `AsyncFn` bounds compile on stable MSRV.
* [ ] Prefer `AsyncFn` over `Fn -> Future` in `local` mode where possible.
* [ ] Use `for<'b> AsyncFn(&'b T)` style bounds for borrowed predicates only if stable and ergonomic.
* [ ] If higher-ranked async closure bounds cause bad diagnostics, add adapter methods with clearer names.
* [ ] Document any remaining cases where users must write `|x| async move { ... }`.
* [ ] Do not expose `AsyncFn` trait names in normal documentation unless explaining internals.
* [ ] Add UI tests for error messages around `filter(async |x| ...)`.
* [ ] Add UI tests for borrowed captures in async closures.
* [ ] Add UI tests for mutable captures if supported.
* [ ] Add a note in `docs/API_TARGET.md` saying `async |x| ...` is the preferred local operator style.

## 3.3 Local terminal operators

* [ ] Implement `collect()` with no callback: drains flow.
* [ ] Implement `collect(async |x| ...)`.
* [ ] Implement `first()`.
* [ ] Implement `first_or_none()`.
* [ ] Implement `single()`.
* [ ] Implement `single_or_none()`.
* [ ] Implement `last()`.
* [ ] Implement `last_or_none()`.
* [ ] Implement `to_vec()`.
* [ ] Implement `to_set()`.
* [ ] Implement `count()`.
* [ ] Implement `fold(initial, async |acc, x| ...)`.
* [ ] Implement `reduce(async |acc, x| ...)`.
* [ ] Add tests for finite flow terminal operators.
* [ ] Add tests for empty flow terminal operators.
* [ ] Add tests for `first()` cancelling upstream early.
* [ ] Add tests for `single()` erroring on zero elements.
* [ ] Add tests for `single()` erroring on more than one element.
* [ ] Add tests for `to_vec()` preserving order.
* [ ] Add tests for terminal operators not leaking producer tasks.

---

# Phase 4 — Macro DSL: remove collector boilerplate

The current `flow!` macro defines an inner `emit!` macro, so users write `emit!(x)`. It expands to `__collector__.emit($value).await`, which currently ignores early termination. ([GitHub][10]) The target is `emit(x);` with automatic cancellation propagation.

## 4.1 Add a proc-macro crate

* [ ] Add workspace member `coroflow_macros`.
* [ ] Configure it as `proc-macro = true`.
* [ ] Add dependencies: `syn`, `quote`, `proc-macro2`.
* [ ] Re-export proc macros from `coroflow`.
* [ ] Keep existing `macro_rules!` macros during migration.
* [ ] Add feature flag `proc-macros` enabled by default.
* [ ] Add compile tests for proc macro expansion.
* [ ] Add docs explaining fallback syntax if proc macros are disabled.

## 4.2 Implement `flow! { emit(x); }`

* [ ] Implement function-like proc macro `flow!`.
* [ ] Parse the block body with `syn`.
* [ ] Rewrite statements of form `emit(expr);`.
* [ ] Expand `emit(expr);` to cancellation-aware internal emit.
* [ ] Ensure generated emit checks `ControlFlow::Break`.
* [ ] Ensure generated emit returns early from the flow body on downstream break.
* [ ] Support `emit(expr)?` only in `try_flow!`, not plain `flow!`.
* [ ] Support `for` loops containing `emit(...)`.
* [ ] Support `while` loops containing `emit(...)`.
* [ ] Support `loop` blocks containing `emit(...)`.
* [ ] Support `if` / `else` blocks containing `emit(...)`.
* [ ] Support `match` arms containing `emit(...)`.
* [ ] Support user `await` expressions inside the block.
* [ ] Preserve line/column spans as much as possible for diagnostics.
* [ ] Add test `flow_macro_emit_function_syntax`.
* [ ] Add test `flow_macro_emit_in_for_loop`.
* [ ] Add test `flow_macro_emit_in_match`.
* [ ] Add test `flow_macro_take_one_stops_infinite_loop`.
* [ ] Add test `flow_macro_error_message_for_emit_outside_flow`.
* [ ] Add test `flow_macro_no_bang_required`.
* [ ] Deprecate old `emit!` docs but keep compatibility.

## 4.3 Implement `try_flow!`

* [ ] Add `LocalTryFlow<'a, T, E>` or equivalent fallible local flow type.
* [ ] Implement `try_flow! { ... }` proc macro.
* [ ] Allow `?` inside `try_flow!`.
* [ ] Rewrite `emit(expr);` to cancellation-aware fallible emit.
* [ ] Ensure upstream errors stop collection.
* [ ] Ensure downstream break is not reported as user error.
* [ ] Add test `try_flow_can_use_question_mark`.
* [ ] Add test `try_flow_emit_stops_after_take`.
* [ ] Add test `try_flow_error_propagates_to_collect`.
* [ ] Add test `try_flow_error_runs_on_completion_hook`.
* [ ] Add test `try_flow_can_map_error`.
* [ ] Add documentation comparing `flow!` and `try_flow!`.

## 4.4 Implement `flow_send!` and `try_flow_send!`

* [ ] Add `flow_send!` macro for send mode.
* [ ] Ensure generated code requires `Send + 'static` captures.
* [ ] Add compile-pass test for `flow_send!` with owned captures.
* [ ] Add compile-fail test for `flow_send!` with `Rc`.
* [ ] Add `try_flow_send!`.
* [ ] Add docs showing when to choose `flow!` versus `flow_send!`.

## 4.5 Replace dangerous `emit` behavior

* [ ] Rename current `FlowCollector::emit` to `emit_ignore_cancel` or equivalent in advanced API.
* [ ] Add deprecation warning to old `emit` if keeping it temporarily.
* [ ] Make macro-generated `emit(...)` always use cancellation-aware emit.
* [ ] Update all examples to stop using raw `collector.emit(...)`.
* [ ] Update tests that currently write `let _ = emit!(i);`.
* [ ] Add regression test `macro_emit_never_ignores_downstream_break`.
* [ ] Add regression test `raw_emit_ignore_cancel_is_not_used_by_flow_macro`.
* [ ] Add docs warning that low-level ignore-cancel emit is advanced only.

---

# Phase 5 — Kotlin-like coroutine macros and prelude

## 5.1 Prelude

* [ ] Add `coroflow::local::prelude`.
* [ ] Export `flow!`.
* [ ] Export `try_flow!`.
* [ ] Export `coroutine_scope!`.
* [ ] Export `supervisor_scope!`.
* [ ] Export `launch!`.
* [ ] Export `async_!`.
* [ ] Export `delay`.
* [ ] Export `yield_now`.
* [ ] Export `is_active`.
* [ ] Export `ensure_active`.
* [ ] Export `DurationExt`.
* [ ] Export `LocalFlow`.
* [ ] Export `LocalTryFlow`.
* [ ] Export local `FlowExt`.
* [ ] Export terminal traits.
* [ ] Export `StateFlow` / `MutableStateFlow`.
* [ ] Export `SharedFlow` / `MutableSharedFlow`.
* [ ] Export `RepeatSpec`.
* [ ] Export `Result`.
* [ ] Keep advanced internals out of the prelude.
* [ ] Add `coroflow::send::prelude`.
* [ ] Ensure examples need only one `use coroflow::local::prelude::*;`.

## 5.2 Duration helpers

* [ ] Add `rs_coroutine_core/src/time.rs`.
* [ ] Implement `delay(duration)`.
* [ ] Implement `yield_now()`.
* [ ] Implement `DurationExt` for `u64`.
* [ ] Implement `DurationExt` for `usize` if desired.
* [ ] Add `.millis()`.
* [ ] Add `.seconds()`.
* [ ] Add `.minutes()`.
* [ ] Add `.hours()` only if useful.
* [ ] Add test `duration_ext_millis`.
* [ ] Add test `duration_ext_seconds`.
* [ ] Update examples to use `500.millis()`.
* [ ] Re-export from both local and send preludes.

## 5.3 `#[coroflow::main]`

* [ ] Implement attribute macro `#[coroflow::main]`.
* [ ] Default mode should use Tokio multi-thread runtime for send mode.
* [ ] Implement `#[coroflow::main(local)]`.
* [ ] Local mode should use current-thread Tokio runtime plus `LocalSet`.
* [ ] Ensure local mode permits `!Send` local flows.
* [ ] Ensure local mode examples can use `Rc`.
* [ ] Add compile-pass test `main_local_allows_rc`.
* [ ] Add compile-pass test `main_send_requires_send`.
* [ ] Add docs for `#[coroflow::main]`.
* [ ] Add docs for using normal `#[tokio::main]` without the macro.

## 5.4 `coroutine_scope!`

* [ ] Replace current `coroutine_scope!` implementation that simply launches a single task and joins it.
* [ ] Implement a scope runner function in `rs_coroutine_core`.
* [ ] Macro should expand to `run_scope(async move { ... }).await`.
* [ ] The scope should not return until body and child jobs finish.
* [ ] Ignored `launch!` handles must still be owned by the scope.
* [ ] Child failure in normal scope should cancel siblings.
* [ ] Parent cancellation should cancel children.
* [ ] Scope body error should cancel children and wait for cleanup.
* [ ] Scope body panic should cancel children and preserve panic outcome.
* [ ] Add test `coroutine_scope_waits_for_ignored_launch`.
* [ ] Add test `coroutine_scope_body_error_cancels_children`.
* [ ] Add test `coroutine_scope_child_failure_cancels_siblings`.
* [ ] Add test `coroutine_scope_parent_cancel_cancels_children`.
* [ ] Add test `coroutine_scope_no_zombie_tasks_after_exit`.
* [ ] Add documentation comparing this to Kotlin `coroutineScope`.

## 5.5 `supervisor_scope!`

* [ ] Add `SupervisorScope` mode to scope internals.
* [ ] Implement `supervisor_scope!`.
* [ ] Child failure should not cancel siblings.
* [ ] Parent cancellation should still cancel all children.
* [ ] Scope exit should still wait for all children.
* [ ] Add test `supervisor_scope_child_failure_does_not_cancel_sibling`.
* [ ] Add test `supervisor_scope_parent_cancel_cancels_all`.
* [ ] Add test `supervisor_scope_waits_for_ignored_launch`.
* [ ] Add docs comparing this to Kotlin `supervisorScope`.

## 5.6 `launch!`

* [ ] Implement `launch! { ... }` using current scope.
* [ ] Implement `launch_on!(dispatcher => { ... })` for explicit dispatcher use.
* [ ] Implement `launch!(scope => { ... })` for explicit scope.
* [ ] Ensure `launch!` registers child with parent scope.
* [ ] Ensure dropping returned job handle does not detach task from parent scope.
* [ ] Add test `launch_ignored_handle_is_still_scoped`.
* [ ] Add test `launch_handle_cancel_cancels_only_that_job`.
* [ ] Add test `launch_cancel_does_not_cancel_parent`.
* [ ] Add docs showing normal `launch!` usage.
* [ ] Add docs showing explicit `launch_on!`.

## 5.7 `async_!` and `Deferred`

* [ ] Add `async_! { ... }` macro as ergonomic alias for current-scope async task.
* [ ] Return a `Deferred<T>`.
* [ ] Support `?` inside `async_!`.
* [ ] Add `.await?` style if possible via `IntoFuture` implementation.
* [ ] If direct `.await?` is not ergonomic, expose `.await_result().await?` but keep target docs pushing `.await?` after `IntoFuture`.
* [ ] Implement `IntoFuture` for `Deferred<T>` if feasible.
* [ ] Preserve current multi-await behavior.
* [ ] Add test `async_macro_returns_deferred`.
* [ ] Add test `deferred_can_be_awaited_multiple_times`.
* [ ] Add test `deferred_child_cancel_does_not_cancel_parent`.
* [ ] Add test `deferred_parent_cancel_interrupts_await`.
* [ ] Add docs comparing to Kotlin `async`.

---

# Phase 6 — Full structured-concurrency nursery

Current scope docs say the `job` field is not actively used for lifecycle tracking. That must change for Kotlin-like behavior. ([GitHub][5])

## 6.1 Scope internals

* [ ] Introduce `ScopeInner`.
* [ ] Move dispatcher, job, cancel token, child registry, and supervisor flag into `ScopeInner`.
* [ ] Make `CoroutineScope` a lightweight cloneable handle to `Arc<ScopeInner>`.
* [ ] Add child registry for all launched jobs.
* [ ] Add child registry for all deferred tasks.
* [ ] Add child registry for flow operator producer tasks when spawned inside scope.
* [ ] Ensure registry removes completed children.
* [ ] Ensure scope can wait for all children.
* [ ] Ensure child outcome is visible to parent scope.
* [ ] Ensure parent can cancel all children.
* [ ] Ensure job completion ordering is deterministic.
* [ ] Add tests for child registration.
* [ ] Add tests for child deregistration.
* [ ] Add tests for waiting on all children.
* [ ] Add tests for cancellation tree.

## 6.2 Outcome propagation

* [ ] Define parent outcome policy for normal scope.
* [ ] Define parent outcome policy for supervisor scope.
* [ ] Preserve `TaskError::Panicked` over cancellation.
* [ ] Preserve `TaskError::Aborted` over cancellation where appropriate.
* [ ] Ensure a child panic is not masked by later cancellation.
* [ ] Ensure a cancelled child does not make supervisor scope fail unless awaited explicitly.
* [ ] Add test `panic_outcome_not_masked_by_cancel`.
* [ ] Add test `normal_scope_child_panic_cancels_sibling`.
* [ ] Add test `supervisor_scope_child_panic_preserved_but_sibling_runs`.
* [ ] Add docs for outcome ordering.

## 6.3 Local task support

* [ ] Add `LocalCoroutineScope`.
* [ ] Add `LocalJobHandle`.
* [ ] Add `LocalDeferred<T>`.
* [ ] Add `LocalDispatcher`.
* [ ] Add a `LocalSet`-based runner.
* [ ] Use `spawn_local` for local tasks, not `tokio::spawn`, when inside local mode.
* [ ] Add test `local_launch_accepts_non_send_future`.
* [ ] Add test `local_async_accepts_non_send_future`.
* [ ] Add test `local_scope_runs_on_current_thread`.
* [ ] Add docs explaining that local mode removes `Send`, but not every possible ownership constraint.

## 6.4 Borrowed scoped task feasibility gate

True borrowed child tasks that can reference variables from the scope body without `move`, `Arc`, or cloning are the hardest part. Implement this only after a deliberate feasibility decision.

* [ ] Create `docs/SCOPED_BORROWED_TASKS.md`.
* [ ] Document whether the crate will remain `forbid(unsafe_code)` for scoped borrowed tasks.
* [ ] Research whether a safe third-party scoped async task crate satisfies the invariants.
* [ ] If using a third-party crate, add it behind feature flag `scoped-borrowed-tasks`.
* [ ] If not using a third-party crate, explicitly document that `launch!` still requires owned captures.
* [ ] Add a `capture!` or `launch!(clone a, b => { ... })` helper if fully borrowed launch is not implemented.
* [ ] Add compile-pass test for the chosen ergonomic capture pattern.
* [ ] Add compile-fail test for unsupported borrowed child patterns.
* [ ] Do not add unsafe code without a separate design document and tests.
* [ ] Do not claim “no `'static` launch” in docs until tests prove it.

---

# Phase 7 — Flow operator cancellation hardening

Every operator that spawns must follow:

```text
downstream stops => producer is cancelled => producer is joined or explicitly known safe
```

## 7.1 Operator audit

* [ ] Create `docs/FLOW_OPERATOR_AUDIT.md`.
* [ ] List every operator in `operators/implementation.rs`.
* [ ] Mark each operator as `pure`, `stateful`, or `spawning`.
* [ ] For each `spawning` operator, document producer owner.
* [ ] For each `spawning` operator, document downstream-break cleanup.
* [ ] For each `spawning` operator, document drop cleanup.
* [ ] For each `spawning` operator, add one infinite-upstream cancellation test.
* [ ] For each `spawning` operator, add one sibling-not-cancelled test.
* [ ] Ensure `take`, `buffer`, `flow_on`, `flat_map_latest`, `channel_flow`, `merge`, `zip`, `combine`, `sample`, `debounce`, and `timeout` are covered.

## 7.2 `take`

* [ ] Ensure `take(0)` never starts upstream if possible.
* [ ] Ensure `take(n)` cancels upstream after `n` values.
* [ ] Ensure `take(n)` does not cancel parent scope.
* [ ] Ensure `take(n)` does not leak producer task.
* [ ] Add test `take_zero_does_not_collect_upstream`.
* [ ] Add test `take_one_cancels_infinite_upstream`.
* [ ] Add test `take_n_preserves_order`.
* [ ] Add test `take_n_no_sibling_cancel`.

## 7.3 `buffer`

* [ ] Ensure `buffer(capacity)` applies backpressure.
* [ ] Ensure downstream break cancels producer.
* [ ] Ensure receiver drop stops upstream sender.
* [ ] Ensure producer is joined where deterministic cleanup is required.
* [ ] Add test `buffer_take_one_cancels_infinite_upstream`.
* [ ] Add test `buffer_backpressure_capacity_one`.
* [ ] Add test `buffer_does_not_busy_loop_after_receiver_drop`.
* [ ] Add test `buffer_no_sibling_cancel`.

## 7.4 `flat_map_latest`

* [ ] Keep current improvement: old inner stream must be cancelled and joined before new inner starts.
* [ ] Ensure outer producer is cancelled on downstream break.
* [ ] Ensure current inner stream is cancelled on downstream break.
* [ ] Ensure final inner stream drains only when outer completes normally.
* [ ] Add test `flat_map_latest_cancels_previous_before_next_starts`.
* [ ] Add test `flat_map_latest_downstream_break_cancels_outer_and_inner`.
* [ ] Add test `flat_map_latest_no_old_values_after_switch`.
* [ ] Add test `flat_map_latest_no_sibling_cancel`.
* [ ] Add test `flat_map_latest_inner_panic_observable`.

## 7.5 `merge`

* [ ] Implement or harden `merge` so each source is a child producer.
* [ ] Downstream break should cancel all remaining sources.
* [ ] Normal completion should join all sources.
* [ ] Source panic/error should obey normal/supervisor semantics.
* [ ] Add test `merge_emits_all_finite_values`.
* [ ] Add test `merge_take_one_cancels_all_sources`.
* [ ] Add test `merge_one_source_panic_cancels_others_in_normal_scope`.
* [ ] Add test `merge_no_sibling_cancel`.

## 7.6 `zip`

* [ ] Implement or harden `zip` with two owned source producers.
* [ ] Stop when either source completes.
* [ ] Cancel the longer source when shorter completes.
* [ ] Downstream break cancels both sources.
* [ ] Add test `zip_pairs_in_order`.
* [ ] Add test `zip_shorter_source_stops_longer_source`.
* [ ] Add test `zip_take_one_cancels_both_sources`.
* [ ] Add test `zip_no_sibling_cancel`.

## 7.7 `combine`

* [ ] Implement or harden `combine` with latest-value storage.
* [ ] Do not emit until all sources have at least one value.
* [ ] Downstream break cancels all producers.
* [ ] Source completion policy should match Kotlin-like combine behavior.
* [ ] Add test `combine_waits_for_all_initial_values`.
* [ ] Add test `combine_emits_on_latest_updates`.
* [ ] Add test `combine_downstream_break_cancels_all_sources`.
* [ ] Add test `combine_no_sibling_cancel`.

---

# Phase 8 — Fallible flows and Kotlin-style error handling

Rust needs typed fallibility, but user code should not become `Flow<Result<T, E>>` everywhere.

## 8.1 Error types

* [ ] Add `rs_flow/src/error.rs`.
* [ ] Define `FlowError` for library-level errors.
* [ ] Define terminal errors: empty flow, more than one value, timeout, cancellation, task error.
* [ ] Define `TryFlowError<E>` if user errors need wrapping.
* [ ] Add `pub type Result<T, E = FlowError> = std::result::Result<T, E>` in prelude.
* [ ] Ensure cancellation is distinguishable from user errors.
* [ ] Ensure downstream early stop is not treated as user error.
* [ ] Add tests for error formatting.
* [ ] Add tests for error conversions.

## 8.2 `LocalTryFlow<'a, T, E>`

* [ ] Implement fallible local flow type.
* [ ] Add `try_collect`.
* [ ] Add `collect` that returns `Result`.
* [ ] Add `map`.
* [ ] Add `try_map`.
* [ ] Add `filter`.
* [ ] Add `try_filter`.
* [ ] Add `on_each`.
* [ ] Add `try_on_each`.
* [ ] Add `take`.
* [ ] Add `flat_map_latest`.
* [ ] Add terminal operators.
* [ ] Add tests for every fallible operator.
* [ ] Add docs showing `try_flow! { let x = foo().await?; emit(x); }`.

## 8.3 `catch`

* [ ] Implement `catch(async |error| ...)`.
* [ ] Allow catch to emit replacement values.
* [ ] Ensure catch catches upstream user errors.
* [ ] Ensure catch does not swallow downstream cancellation.
* [ ] Ensure catch does not swallow task panic unless intentionally mapped.
* [ ] Add test `catch_emits_fallback`.
* [ ] Add test `catch_does_not_catch_downstream_break`.
* [ ] Add test `catch_does_not_mask_cancellation`.
* [ ] Add docs comparing to Kotlin `catch`.

## 8.4 `retry` and `retry_when`

* [ ] Add `RepeatSpec`.
* [ ] Add fixed delay strategy.
* [ ] Add exponential backoff strategy.
* [ ] Add max attempts.
* [ ] Add full jitter.
* [ ] Add cancellation support during delay.
* [ ] Implement `.retry(spec)`.
* [ ] Implement `.retry_when(async |error, attempt| ...)`.
* [ ] Ensure a consumed flow factory can be recreated on retry.
* [ ] Add test `retry_succeeds_after_failure`.
* [ ] Add test `retry_stops_after_max_attempts`.
* [ ] Add test `retry_delay_cancelled_by_scope_cancel`.
* [ ] Add test `retry_when_predicate_false_stops`.
* [ ] Add docs with Kotlin comparison.

## 8.5 `timeout`

* [ ] Implement flow-level timeout operator.
* [ ] Ensure timeout cancels upstream producer.
* [ ] Ensure timeout returns a typed error.
* [ ] Ensure timeout does not cancel parent scope.
* [ ] Add test `timeout_errors_when_no_value`.
* [ ] Add test `timeout_cancels_infinite_upstream`.
* [ ] Add test `timeout_no_sibling_cancel`.

---

# Phase 9 — Hot flows and sharing

## 9.1 Naming alignment

* [ ] Rename or alias `StateFlow` to `MutableStateFlow` where mutation is exposed.
* [ ] Rename or alias `SharedFlow` to `MutableSharedFlow` where mutation is exposed.
* [ ] Keep `StateFlow` read-only view if possible.
* [ ] Keep `SharedFlow` read-only view if possible.
* [ ] Add docs explaining mutable/read-only split.
* [ ] Add migration notes from existing `StateFlow::new` and `SharedFlow::new`.

## 9.2 Local and send hot flows

* [ ] Decide whether hot flows are always send-capable or have local/send variants.
* [ ] If always send-capable, document that they use Tokio channels internally.
* [ ] If local variants exist, implement `LocalStateFlow`.
* [ ] If local variants exist, implement `LocalSharedFlow`.
* [ ] Add `as_flow()` for local mode.
* [ ] Add `as_send_flow()` for send mode if needed.
* [ ] Add tests for multiple subscribers.
* [ ] Add tests for late subscribers.
* [ ] Add tests for dropped subscribers.
* [ ] Add tests for cancellation of `as_flow()` collection.

## 9.3 Replay and buffer policy

* [ ] Add `MutableSharedFlow::new(replay, extra_buffer_capacity)`.
* [ ] Implement replay cache.
* [ ] Implement buffer overflow policy if desired.
* [ ] Add `replay_cache()`.
* [ ] Add `reset_replay_cache()`.
* [ ] Add test `shared_flow_replays_last_n_values`.
* [ ] Add test `shared_flow_no_replay_when_zero`.
* [ ] Add test `shared_flow_dropped_receiver_does_not_block_emit`.

## 9.4 `state_in` and `share_in`

* [ ] Implement `SharingStarted`.
* [ ] Add `SharingStarted::Eagerly`.
* [ ] Add `SharingStarted::Lazily`.
* [ ] Add `SharingStarted::WhileSubscribed`.
* [ ] Implement `.state_in(scope, started, initial)`.
* [ ] Implement `.share_in(scope, started, replay)`.
* [ ] Ensure upstream collection is owned by the provided scope.
* [ ] Ensure upstream stops when scope is cancelled.
* [ ] Ensure `WhileSubscribed` starts and stops based on subscriber count.
* [ ] Add test `state_in_eagerly_collects_immediately`.
* [ ] Add test `state_in_lazily_collects_on_first_subscriber`.
* [ ] Add test `share_in_replay`.
* [ ] Add test `share_in_scope_cancel_stops_upstream`.
* [ ] Add docs comparing to Kotlin `stateIn` and `shareIn`.

---

# Phase 10 — Dispatcher cleanup

The repo should be honest that it is Tokio-based, while still offering Kotlin-like names.

## 10.1 Rename or clarify dispatchers

* [ ] Update docs to say the runtime is Tokio-only.
* [ ] Rename `Dispatchers::main()` docs to avoid implying UI main thread.
* [ ] Rename or alias `Dispatchers::default()`.
* [ ] Clarify that current `Dispatchers::io()` is not yet a true blocking IO pool.
* [ ] Add `Dispatchers::tokio()`.
* [ ] Add `Dispatchers::current_thread()` if useful.
* [ ] Add `Dispatchers::local()` if local runtime support lands.
* [ ] Add docs for each dispatcher.

## 10.2 Real blocking dispatcher

* [ ] Add `with_blocking`.
* [ ] Add `Dispatchers::blocking()` if it can be implemented cleanly.
* [ ] Use `tokio::task::spawn_blocking` internally for blocking work.
* [ ] Return `TaskError` on panic/abort.
* [ ] Add test `with_blocking_returns_value`.
* [ ] Add test `with_blocking_panic_reported`.
* [ ] Add docs: use for CPU-heavy or blocking work.
* [ ] Do not route normal async `io()` through `spawn_blocking` automatically without clear semantics.

## 10.3 `flow_on` and dispatcher semantics

* [ ] Ensure `flow_on(Dispatchers::blocking())` is either rejected or clearly documented.
* [ ] Ensure `flow_on(Dispatchers::io())` behavior is documented accurately.
* [ ] Add test `flow_on_uses_requested_dispatcher_marker` if dispatcher identity can be observed.
* [ ] Add docs warning that Rust futures must not perform blocking work directly on async executors.

---

# Phase 11 — Kotlin Flow parity operators

After cancellation and ergonomics are correct, add operators.

## 11.1 Builders

* [ ] Add `empty_flow()`.
* [ ] Add `flow_of!`.
* [ ] Add `flow_of_one`.
* [ ] Add `iter.into_flow()`.
* [ ] Add `once`.
* [ ] Add `repeat_flow`.
* [ ] Add `generate_flow`.
* [ ] Add `interval_flow`.
* [ ] Add `channel_flow!` macro.
* [ ] Add `callback_flow!` design doc.
* [ ] Add tests for each builder.
* [ ] Add docs with Kotlin equivalents.

## 11.2 Transformation operators

* [ ] Add `map_not_null`.
* [ ] Add `filter_not`.
* [ ] Add `filter_map`.
* [ ] Add `transform`.
* [ ] Add `transform_latest`.
* [ ] Add `flat_map_merge(concurrency)`.
* [ ] Add `flatten_concat`.
* [ ] Add `flatten_merge`.
* [ ] Add tests for each transformation.
* [ ] Add cancellation tests for each transformation that spawns.

## 11.3 Scanning operators

* [ ] Add `scan`.
* [ ] Add `running_fold`.
* [ ] Add `running_reduce`.
* [ ] Add tests for empty input.
* [ ] Add tests for finite input.
* [ ] Add docs with Kotlin equivalents.

## 11.4 Buffering/time operators

* [ ] Add `conflate`.
* [ ] Add `collect_latest`.
* [ ] Add `debounce`.
* [ ] Add `sample`.
* [ ] Add `throttle_first` if desired.
* [ ] Add cancellation tests for each time-based operator.
* [ ] Use `tokio::time::pause` in tests where possible.
* [ ] Do not use real sleeps except as timeout guards.

## 11.5 Lifecycle operators

* [ ] Add `on_start`.
* [ ] Add `on_empty`.
* [ ] Add `on_completion`.
* [ ] Add `on_cancel`.
* [ ] Add `on_error` if useful for Rust naming.
* [ ] Ensure `on_completion` receives completion cause.
* [ ] Ensure `on_completion` runs on cancellation.
* [ ] Ensure `on_completion` runs on error.
* [ ] Ensure `on_completion` runs on normal completion.
* [ ] Add tests for each lifecycle path.

---

# Phase 12 — Kotlin-like utility layer

These are not strictly Flow, but they make the ecosystem feel complete.

## 12.1 `RepeatSpec`

* [ ] Add `rs_coroutine_core/src/repeat.rs` or `rs_flow/src/repeat.rs`.
* [ ] Implement `RepeatSpec::fixed(delay)`.
* [ ] Implement `RepeatSpec::exponential(base)`.
* [ ] Implement `.max_attempts(n)`.
* [ ] Implement `.max_elapsed(duration)`.
* [ ] Implement `.jitter_none()`.
* [ ] Implement `.jitter_full()`.
* [ ] Implement `.predicate(async |error, attempt| ...)`.
* [ ] Add `repeat_until_success`.
* [ ] Add cancellation-aware sleep.
* [ ] Add tests for attempt counts.
* [ ] Add tests for cancellation during backoff.
* [ ] Add docs with Kotlin `retryWhen` comparison.

## 12.2 `RequestCoalescer`

* [ ] Add `rs_coroutine_core/src/coalescer.rs`.
* [ ] Implement `RequestCoalescer<K, V>`.
* [ ] Ensure concurrent callers for same key share one loader.
* [ ] Ensure different keys run independently.
* [ ] Return `Arc<V>` by default to avoid clones.
* [ ] Add fallible loader variant.
* [ ] Decide whether failed loads are cached or not.
* [ ] Ensure cancellation of one waiter does not cancel the shared load if other waiters remain.
* [ ] Ensure loader is cancelled when all waiters cancel if feasible.
* [ ] Add test `coalescer_single_flight_same_key`.
* [ ] Add test `coalescer_parallel_different_keys`.
* [ ] Add test `coalescer_error_fans_out`.
* [ ] Add test `coalescer_cancel_one_waiter_keeps_loader`.
* [ ] Add docs with asset/font/layout loading examples.

## 12.3 Shared mutable state helpers

* [ ] Add `state(value)` helper returning ergonomic mutable state.
* [ ] Add `shared(value)` helper returning `Arc<T>`.
* [ ] Add `mutex(value)` helper returning an async mutex wrapper.
* [ ] Add `atomic_state` only if needed.
* [ ] Add docs explaining Rust does not hide shared mutability like Kotlin.
* [ ] Add examples showing state updates inside `launch!`.
* [ ] Avoid macros that hide locking or mutation too much.

---

# Phase 13 — Documentation, examples, and migration

## 13.1 README rewrite

* [ ] Rewrite README around `local::prelude::*`.
* [ ] Put Kotlin-like examples first.
* [ ] Move raw `Flow::new(|collector| ...)` examples to advanced section.
* [ ] Explain local vs send in one clear section.
* [ ] Explain that `.await` remains normal Rust.
* [ ] Explain that `emit(x);` inside `flow!` is cancellation-safe.
* [ ] Explain that `try_flow!` is the Rust replacement for exception-throwing Kotlin flow builders.
* [ ] Explain Tokio-only runtime honestly.
* [ ] Add examples for `coroutine_scope!`, `launch!`, `async_!`, `flow!`, `try_flow!`, `StateFlow`, and `SharedFlow`.
* [ ] Update installation instructions with new version and feature flags.

## 13.2 Kotlin migration guide

* [ ] Rewrite `KOTLIN_MIGRATION.md`.
* [ ] Add table: Kotlin `flow {}` => Rust `flow! {}`.
* [ ] Add table: Kotlin `emit(x)` => Rust `emit(x);`.
* [ ] Add table: Kotlin `map {}` => Rust `.map(async |x| ...)`.
* [ ] Add table: Kotlin `coroutineScope` => Rust `coroutine_scope!`.
* [ ] Add table: Kotlin `supervisorScope` => Rust `supervisor_scope!`.
* [ ] Add table: Kotlin `launch` => Rust `launch!`.
* [ ] Add table: Kotlin `async` => Rust `async_!`.
* [ ] Add table: Kotlin `StateFlow` => Rust `MutableStateFlow` / `StateFlow`.
* [ ] Add table: Kotlin exceptions => Rust `try_flow!` and `?`.
* [ ] Add section: where Rust intentionally differs.
* [ ] Add section: when to use local mode.
* [ ] Add section: when to use send mode.
* [ ] Add section: how to handle shared state.

## 13.3 Examples

* [ ] Implement `examples/kotlin_style_local.rs`.
* [ ] Implement `examples/kotlin_style_send.rs`.
* [ ] Implement `examples/error_handling_try_flow.rs`.
* [ ] Implement `examples/state_and_shared_flow.rs`.
* [ ] Implement `examples/structured_cancellation.rs`.
* [ ] Implement `examples/retry_repeat_spec.rs`.
* [ ] Implement `examples/request_coalescer.rs`.
* [ ] Ensure every example compiles in CI.
* [ ] Add README links to each example.

## 13.4 API docs

* [ ] Add rustdoc to every public type in `local`.
* [ ] Add rustdoc to every public type in `send`.
* [ ] Add rustdoc to every macro.
* [ ] Add rustdoc examples for `flow!`.
* [ ] Add rustdoc examples for `try_flow!`.
* [ ] Add rustdoc examples for `coroutine_scope!`.
* [ ] Add rustdoc examples for `supervisor_scope!`.
* [ ] Add rustdoc examples for `launch!`.
* [ ] Add rustdoc examples for `async_!`.
* [ ] Add rustdoc examples for `RepeatSpec`.
* [ ] Add rustdoc examples for `RequestCoalescer`.
* [ ] Run `cargo test --doc`.

---

# Phase 14 — Release strategy

## 14.1 Version targets

* [ ] Release `v0.2.0` after Phase 1 and Phase 2: cancellation P0 fixed, local/send module split introduced.
* [ ] Release `v0.3.0` after Phase 3 and Phase 4: async-closure local operators and proc-macro `flow!`.
* [ ] Release `v0.4.0` after Phase 5 and Phase 6: Kotlin-like scope macros and structured child ownership.
* [ ] Release `v0.5.0` after Phase 7 and Phase 8: cancellation-hardened operators and fallible flows.
* [ ] Release `v0.6.0` after Phase 9 and Phase 10: hot flow sharing and dispatcher cleanup.
* [ ] Release `v0.7.0` after Phase 11 and Phase 12: Kotlin parity operators and utility layer.
* [ ] Reserve `v1.0.0` for stable API, tested cancellation invariants, docs parity, and no known task leaks.

## 14.2 Compatibility plan

* [ ] Keep old root `Flow<T>` exports during v0.2.
* [ ] Add deprecation warnings only after local/send docs are complete.
* [ ] Avoid breaking published examples without migration notes.
* [ ] Add `compat` module if old APIs need to remain accessible.
* [ ] Add `MIGRATION_0_1_TO_0_2.md`.
* [ ] Add `MIGRATION_0_2_TO_0_3.md`.
* [ ] Document every breaking change in release notes.
* [ ] Ensure crates.io package docs match GitHub README before publishing.

## 14.3 Pre-publish checklist

* [ ] `cargo fmt --all -- --check`
* [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
* [ ] `cargo test --workspace --all-targets --all-features`
* [ ] `cargo test --doc --workspace --all-features`
* [ ] `cargo run --example kotlin_style_local`
* [ ] `cargo run --example kotlin_style_send`
* [ ] `cargo run --example structured_cancellation`
* [ ] `cargo publish --dry-run -p rs_coroutine_core`
* [ ] `cargo publish --dry-run -p coroflow`
* [ ] Verify docs.rs build locally if possible.
* [ ] Tag release only after dry runs pass.
* [ ] Publish `rs_coroutine_core` before `coroflow`.
* [ ] Update README version numbers after publish.

---

# Immediate first PR checklist

This is the first PR I would give to a coding agent.

* [ ] Create branch `fix/scope-aware-handle-child-cancel`.
* [ ] Modify `rs_flow/src/internal_utils.rs`.
* [ ] Remove `cancel_token: CancelToken` from `ScopeAwareHandle`.
* [ ] Change `spawn_in_scope` to store only the returned `JobHandle`.
* [ ] Change `cancel()` to call `self.job.cancel()`.
* [ ] Change `cancel_and_join()` to call `self.job.cancel(); self.job.join().await`.
* [ ] Remove the now-unused `CancelToken` import.
* [ ] Add test `spawn_in_scope_cancel_does_not_cancel_parent_scope`.
* [ ] Add test `cancel_on_drop_does_not_cancel_parent_scope`.
* [ ] Add test `take_one_does_not_cancel_sibling_task`.
* [ ] Add test `flow_stream_drop_does_not_cancel_sibling_task`.
* [ ] Ensure tests use `Notify` / `oneshot`, not sleeps.
* [ ] Run `cargo fmt --all`.
* [ ] Run `cargo test --workspace --all-targets --all-features`.
* [ ] Update `IMPROVEMENTS.md` with the fix.
* [ ] Open PR with title: `Fix flow task cancellation to cancel child job, not parent scope`.

---

# Definition of “done” for the whole roadmap

* [ ] A Kotlin developer can write `use coroflow::local::prelude::*;` and build flows without manually touching collectors.
* [ ] `flow! { emit(x); }` is cancellation-safe by construction.
* [ ] `.map(async |x| ...)` works in local mode.
* [ ] Local flows can capture references and `Rc` without `Send + 'static` errors.
* [ ] Send flows still support cross-thread work explicitly.
* [ ] `coroutine_scope!` waits for ignored children.
* [ ] `supervisor_scope!` preserves sibling jobs after child failure.
* [ ] Dropping a `JobHandle` does not detach the task from its parent scope.
* [ ] Cancelling a flow operator task never cancels the ambient parent scope accidentally.
* [ ] Every spawning flow operator cancels and joins producers on downstream stop.
* [ ] Fallible flows use `try_flow!` and `?` instead of forcing `Flow<Result<T, E>>`.
* [ ] Hot flows have clear mutable/read-only naming.
* [ ] Dispatchers are documented honestly as Tokio-based.
* [ ] README, migration docs, examples, and docs.rs all describe the same API.
* [ ] CI proves formatting, clippy, unit tests, doc tests, compile-pass tests, compile-fail tests, and examples.

[1]: https://raw.githubusercontent.com/samoylenkodmitry/rs-coroutine-rs-flow/refs/heads/main/Cargo.toml "raw.githubusercontent.com"
[2]: https://raw.githubusercontent.com/samoylenkodmitry/rs-coroutine-rs-flow/refs/heads/main/rs_flow/src/lib.rs "raw.githubusercontent.com"
[3]: https://github.com/samoylenkodmitry/rs-coroutine-rs-flow/blob/main/rs_flow/src/flow.rs "rs-coroutine-rs-flow/rs_flow/src/flow.rs at main · samoylenkodmitry/rs-coroutine-rs-flow · GitHub"
[4]: https://github.com/samoylenkodmitry/rs-coroutine-rs-flow/blob/main/rs_flow/src/internal_utils.rs "rs-coroutine-rs-flow/rs_flow/src/internal_utils.rs at main · samoylenkodmitry/rs-coroutine-rs-flow · GitHub"
[5]: https://github.com/samoylenkodmitry/rs-coroutine-rs-flow/blob/main/rs_coroutine_core/src/scope.rs "rs-coroutine-rs-flow/rs_coroutine_core/src/scope.rs at main · samoylenkodmitry/rs-coroutine-rs-flow · GitHub"
[6]: https://github.com/samoylenkodmitry/rs-coroutine-rs-flow/blob/main/rs_coroutine_core/src/executor.rs "rs-coroutine-rs-flow/rs_coroutine_core/src/executor.rs at main · samoylenkodmitry/rs-coroutine-rs-flow · GitHub"
[7]: https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/?utm_source=chatgpt.com "Announcing Rust 1.85.0 and Rust 2024"
[8]: https://docs.rs/tokio/latest/tokio/task/fn.spawn_local.html?utm_source=chatgpt.com "spawn_local in tokio::task - Rust"
[9]: https://docs.rs/futures/latest/futures/future/type.BoxFuture.html?utm_source=chatgpt.com "BoxFuture in futures::future - Rust"
[10]: https://github.com/samoylenkodmitry/rs-coroutine-rs-flow/blob/main/rs_flow/src/macros.rs "rs-coroutine-rs-flow/rs_flow/src/macros.rs at main · samoylenkodmitry/rs-coroutine-rs-flow · GitHub"

