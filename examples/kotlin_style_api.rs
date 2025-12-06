//! Demonstrates the Kotlin-like API with macros and sync operators.
//!
//! This example shows how to use the ergonomic API that reduces boilerplate
//! and provides a familiar experience for Kotlin developers.

use coroflow::{flow, flow_fn, shared_flow, state_flow, Flow, FlowExt};
use std::time::Duration;
use tokio::time::sleep;

/// Example 1: Using the flow! macro with implicit emit!
///
/// Kotlin equivalent:
/// ```kotlin
/// val numbers = flow {
///     for (i in 1..5) {
///         emit(i)
///     }
/// }
/// ```
async fn example_flow_macro() {
    println!("=== Example 1: flow! macro with emit! ===");

    // Kotlin-like syntax using flow! macro
    let numbers: Flow<i32> = flow! {
        for i in 1..=5 {
            println!("  Emitting {}", i);
            emit!(i);
        }
    };

    // Collect with simple println
    numbers
        .collect(|x| async move {
            println!("  Received: {}", x);
        })
        .await;

    println!();
}

/// Example 2: Using sync operators (map_sync, filter_sync)
///
/// Kotlin equivalent:
/// ```kotlin
/// numbers
///     .map { it * 2 }
///     .filter { it > 5 }
///     .take(3)
///     .collect { println(it) }
/// ```
async fn example_sync_operators() {
    println!("=== Example 2: Sync Operators (Kotlin-like) ===");

    let numbers: Flow<i32> = flow! {
        for i in 1..=10 {
            emit!(i);
        }
    };

    // Kotlin-like: .map { it * 2 }.filter { it > 5 }
    // Instead of: .map(|x| async move { x * 2 }).filter(|x| { let x = *x; async move { x > 5 } })
    numbers
        .map_sync(|x| x * 2)
        .filter_sync(|x| *x > 5)
        .take(3)
        .collect(|x| async move {
            println!("  Got: {}", x);
        })
        .await;

    println!();
}

/// Example 3: Using on_each (like Kotlin's onEach)
///
/// Kotlin equivalent:
/// ```kotlin
/// numbers
///     .onEach { println("Processing: $it") }
///     .map { it * 2 }
///     .collect { println("Result: $it") }
/// ```
async fn example_on_each() {
    println!("=== Example 3: on_each (Kotlin's onEach) ===");

    let numbers: Flow<i32> = flow! {
        for i in 1..=3 {
            emit!(i);
        }
    };

    numbers
        .on_each(|x| println!("  Processing: {}", x))
        .map_sync(|x| x * 2)
        .collect(|x| async move {
            println!("  Result: {}", x);
        })
        .await;

    println!();
}

/// Example 4: distinct_until_changed (like Kotlin's distinctUntilChanged)
///
/// Kotlin equivalent:
/// ```kotlin
/// flowOf(1, 1, 2, 2, 3, 1, 1)
///     .distinctUntilChanged()
///     .collect { println(it) }  // 1, 2, 3, 1
/// ```
async fn example_distinct_until_changed() {
    println!("=== Example 4: distinct_until_changed ===");

    let values: Flow<i32> = flow! {
        for x in [1, 1, 2, 2, 3, 1, 1] {
            emit!(x);
        }
    };

    print!("  Distinct values: ");
    values
        .distinct_until_changed()
        .collect(|x| async move {
            print!("{} ", x);
        })
        .await;
    println!();
    println!();
}

/// Example 5: drop_first and take_while
///
/// Kotlin equivalent:
/// ```kotlin
/// (1..10).asFlow()
///     .drop(3)
///     .takeWhile { it < 8 }
///     .collect { println(it) }  // 4, 5, 6, 7
/// ```
async fn example_drop_take_while() {
    println!("=== Example 5: drop_first and take_while ===");

    let numbers: Flow<i32> = flow! {
        for i in 1..=10 {
            emit!(i);
        }
    };

    print!("  After drop(3) + takeWhile(< 8): ");
    numbers
        .drop_first(3)
        .take_while(|x| *x < 8)
        .collect(|x| async move {
            print!("{} ", x);
        })
        .await;
    println!();
    println!();
}

/// Example 6: flat_map_sync
///
/// Kotlin equivalent:
/// ```kotlin
/// flowOf(1, 2, 3)
///     .flatMap { x -> flowOf(x, x * 10) }
///     .collect { println(it) }  // 1, 10, 2, 20, 3, 30
/// ```
async fn example_flat_map() {
    println!("=== Example 6: flat_map_sync ===");

    let numbers: Flow<i32> = flow! {
        for i in 1..=3 {
            emit!(i);
        }
    };

    print!("  Flat mapped: ");
    numbers
        .flat_map_sync(|x| {
            // Need to copy x since it's captured by value
            let x = x;
            let x2 = x * 10;
            flow_fn(move |collector| async move {
                collector.emit(x).await;
                collector.emit(x2).await;
            })
        })
        .collect(|x| async move {
            print!("{} ", x);
        })
        .await;
    println!();
    println!();
}

/// Example 7: Using state_flow! macro
///
/// Kotlin equivalent:
/// ```kotlin
/// val counter = MutableStateFlow(0)
/// counter.value = 1
/// println(counter.value)  // 1
/// ```
async fn example_state_flow_macro() {
    println!("=== Example 7: state_flow! macro ===");

    let counter = state_flow!(0);

    println!("  Initial value: {}", counter.get());

    counter.set(1);
    println!("  After set(1): {}", counter.get());

    counter.set(42);
    println!("  After set(42): {}", counter.get());

    println!();
}

/// Example 8: Using shared_flow! macro
///
/// Kotlin equivalent:
/// ```kotlin
/// val events = MutableSharedFlow<String>(replay = 0)
/// events.emit("Hello")
/// ```
async fn example_shared_flow_macro() {
    println!("=== Example 8: shared_flow! macro ===");

    let events = shared_flow!(16);

    // Start a subscriber with timeout to avoid blocking
    let flow = events.as_flow();
    let handle = tokio::spawn(async move {
        // Use tokio::select! to add a timeout
        let taken_flow = flow.take(2);
        let collect_future = taken_flow.collect(|event: String| async move {
            println!("  Subscriber received: {}", event);
        });
        tokio::select! {
            _ = collect_future => {}
            _ = sleep(Duration::from_secs(2)) => {
                println!("  Subscriber timed out");
            }
        }
    });

    // Give subscriber time to start
    sleep(Duration::from_millis(50)).await;

    // Emit events
    events.emit("Hello".to_string());
    events.emit("World".to_string());

    // Wait a bit then drop events to close the channel
    sleep(Duration::from_millis(100)).await;
    drop(events);

    let _ = handle.await;
    println!();
}

/// Example 9: Chaining multiple operators (complex pipeline)
///
/// Kotlin equivalent:
/// ```kotlin
/// (1..10).asFlow()
///     .filter { it % 2 == 0 }
///     .map { it * it }
///     .take(3)
///     .collect { println("Square: $it") }
/// ```
async fn example_complex_pipeline() {
    println!("=== Example 9: Complex Pipeline ===");

    let numbers: Flow<i32> = flow! {
        for i in 1..=10 {
            emit!(i);
        }
    };

    // Note: on_each can cause issues with async flow composition
    // Using collect to print instead
    numbers
        .filter_sync(|x| *x % 2 == 0) // Even numbers: 2, 4, 6, 8, 10
        .map_sync(|x| x * x) // Square them: 4, 16, 36, 64, 100
        .take(3) // First 3: 4, 16, 36
        .collect(|x| async move {
            println!("  Square: {}", x);
        })
        .await;

    println!();
}

/// Example 10: distinct_until_changed_by with key selector
///
/// Kotlin equivalent:
/// ```kotlin
/// data class User(val id: Int, val name: String)
/// flowOf(User(1, "A"), User(1, "B"), User(2, "C"))
///     .distinctUntilChangedBy { it.id }
///     .collect { println(it) }  // User(1, A), User(2, C)
/// ```
async fn example_distinct_by_key() {
    println!("=== Example 10: distinct_until_changed_by ===");

    #[derive(Clone, Debug)]
    struct User {
        id: i32,
        name: String,
    }

    let users: Flow<User> = flow! {
        emit!(User { id: 1, name: "Alice".to_string() });
        emit!(User { id: 1, name: "Alice v2".to_string() });  // Same id, different name
        emit!(User { id: 2, name: "Bob".to_string() });
        emit!(User { id: 2, name: "Bob v2".to_string() });  // Same id
    };

    println!("  Distinct by user ID:");
    users
        .distinct_until_changed_by(|u| u.id)
        .collect(|u| async move {
            println!("    {:?}", u);
        })
        .await;

    println!();
}

/// Example 11: Comparing old vs new syntax
async fn example_comparison() {
    println!("=== Example 11: Syntax Comparison ===");

    // OLD (verbose) syntax:
    println!("  Old syntax (verbose):");
    let old_flow = flow_fn(|collector| async move {
        for i in 1..=3 {
            collector.emit(i).await;
        }
    });

    old_flow
        .clone()
        .map(|x| async move { x * 2 })
        .filter(|x| {
            let x = *x;
            async move { x > 2 }
        })
        .collect(|x| async move {
            println!("    Got: {}", x);
        })
        .await;

    // NEW (Kotlin-like) syntax:
    println!("  New syntax (Kotlin-like):");
    let new_flow: Flow<i32> = flow! {
        for i in 1..=3 {
            emit!(i);
        }
    };

    new_flow
        .map_sync(|x| x * 2)
        .filter_sync(|x| *x > 2)
        .collect(|x| async move {
            println!("    Got: {}", x);
        })
        .await;

    println!();
}

#[tokio::main]
async fn main() {
    println!("Kotlin-Style API Examples\n");
    println!("These examples demonstrate the ergonomic, boilerplate-free API.\n");

    example_flow_macro().await;
    example_sync_operators().await;
    example_on_each().await;
    example_distinct_until_changed().await;
    example_drop_take_while().await;
    example_flat_map().await;
    example_state_flow_macro().await;
    example_shared_flow_macro().await;
    example_complex_pipeline().await;
    example_distinct_by_key().await;
    example_comparison().await;

    println!("All examples completed!");
}
