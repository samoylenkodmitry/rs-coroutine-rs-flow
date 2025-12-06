use coroflow::{flow, FlowExt, SharedFlow, StateFlow, SuspendingExt};
use rs_coroutine_core::{CoroutineScope, Dispatchers};
use coroflow::{flow, FlowExt, SharedFlow, StateFlow, SuspendingExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

// Simulate a user model
#[derive(Clone, Debug)]
struct User {
    id: u32,
    name: String,
}

// Simulate UI state
#[derive(Clone, Debug)]
enum UserUiState {
    Loading,
    Loaded(User),
    Error(String),
}

fn describe_state(state: &UserUiState) -> String {
    match state {
        UserUiState::Loading => "Loading".to_string(),
        UserUiState::Loaded(user) => format!("Loaded {} ({})", user.name, user.id),
        UserUiState::Error(message) => format!("Error: {message}"),
    }
}

/// Simulate fetching a user from an API
async fn fetch_user_from_api() -> User {
    sleep(Duration::from_millis(500)).await;
    User {
        id: 1,
        name: "Alice".to_string(),
    }
}

/// Example demonstrating basic coroutine scope usage
async fn example_basic_scope() {
    println!("=== Example: Basic Scope ===");
    let scope = CoroutineScope::new(Dispatchers::main());

    // Launch a simple coroutine
    let job = scope.launch(async {
        println!("Hello from coroutine!");
        sleep(Duration::from_millis(100)).await;
        println!("Coroutine completed!");
    });

    job.join().await;
    println!();
}

/// Example demonstrating context switching with with_dispatcher
async fn example_with_dispatcher() {
    println!("=== Example: Context Switching ===");
    let scope = Arc::new(CoroutineScope::new(Dispatchers::main()));

    let scope_clone = Arc::clone(&scope);
    let job = scope.launch(async move {
        println!("Starting on main dispatcher");

        // Switch to IO dispatcher for background work
        let user = scope_clone
            .with_dispatcher(Dispatchers::io(), async {
                println!("Fetching user on IO dispatcher...");
                fetch_user_from_api().await
            })
            .await;

        println!(
            "Back on main dispatcher with user: {} ({})",
            user.name, user.id
        );
    });

    job.join().await;
    println!();
}

/// Example demonstrating parallel work with async_task
async fn example_parallel_work() {
    println!("=== Example: Parallel Work ===");
    let scope = Arc::new(CoroutineScope::new(Dispatchers::main()));

    let scope_clone = Arc::clone(&scope);
    let job = scope.launch(async move {
        println!("Starting parallel tasks...");

        let task1 = scope_clone.async_task(Dispatchers::io(), async {
            sleep(Duration::from_millis(300)).await;
            println!("Task 1 completed");
            42
        });

        let task2 = scope_clone.async_task(Dispatchers::io(), async {
            sleep(Duration::from_millis(200)).await;
            println!("Task 2 completed");
            "Hello"
        });

        let (result1, result2) = futures::join!(task1.await_result(), task2.await_result());

        println!("Results: {} and {}", result1, result2);
    });

    job.join().await;
    println!();
}

/// Example demonstrating cold flows
async fn example_cold_flows() {
    println!("=== Example: Cold Flows ===");

    // Create a simple flow
    let numbers = flow(|collector| async move {
        for i in 1..=5 {
            println!("Emitting {}", i);
            collector.emit(i).await;
            sleep(Duration::from_millis(100)).await;
        }
    });

    // Collect with transformation
    println!("Collecting with map:");
    numbers
        .clone()
        .map(|x| async move { x * 2 })
        .take(3)
        .collect(|x| async move { println!("Received: {}", x) })
        .await;

    println!();
}

/// Example demonstrating flow operators
async fn example_flow_operators() {
    println!("=== Example: Flow Operators ===");

    let numbers = flow(|collector| async move {
        for i in 1..=10 {
            collector.emit(i).await;
        }
    });

    println!("Filter and map:");
    numbers
        .filter(|x| {
            let x = *x;
            async move { x % 2 == 0 }
        })
        .map(|x| async move { x * x })
        .take(3)
        .collect(|x| async move { println!("  {}", x) })
        .await;

    println!();
}

/// Example demonstrating SharedFlow
async fn example_shared_flow() {
    println!("=== Example: SharedFlow ===");

    let shared = SharedFlow::new(16);

    // Create multiple subscribers
    let flow1 = shared.as_flow();
    let flow2 = shared.as_flow();

    let collector1 = tokio::spawn(async move {
        flow1
            .take(3)
            .collect(|x| async move { println!("Subscriber 1: {}", x) })
            .await;
    });

    let collector2 = tokio::spawn(async move {
        flow2
            .take(3)
            .collect(|x| async move { println!("Subscriber 2: {}", x) })
            .await;
    });

    // Give subscribers time to set up
    sleep(Duration::from_millis(50)).await;

    // Emit values
    for i in 1..=3 {
        println!("Emitting {}", i);
        shared.emit(i);
        sleep(Duration::from_millis(100)).await;
    }

    let _ = collector1.await;
    let _ = collector2.await;
    println!();
}

/// Example demonstrating StateFlow
async fn example_state_flow() {
    println!("=== Example: StateFlow ===");

    let state = StateFlow::new(UserUiState::Loading);

    // Subscribe to state changes
    let flow = state.as_flow();
    let collector = tokio::spawn(async move {
        flow.take(4)
            .collect(|state| async move { println!("State: {}", describe_state(&state)) })
            .await;
    });

    sleep(Duration::from_millis(50)).await;

    // Update state
    println!("Loading user...");
    sleep(Duration::from_millis(200)).await;

    state.emit(UserUiState::Loaded(User {
        id: 1,
        name: "Bob".to_string(),
    }));
    sleep(Duration::from_millis(200)).await;

    state.emit(UserUiState::Loaded(User {
        id: 2,
        name: "Charlie".to_string(),
    }));

    state.emit(UserUiState::Error("Simulated failure".to_string()));

    let _ = collector.await;
    println!();
}

/// Example demonstrating Suspending blocks
async fn example_suspending() {
    println!("=== Example: Suspending Blocks ===");

    // Create a suspending block
    let load_user = rs_coroutine_core::suspend_block! {
        println!("Loading user in suspending block...");
        sleep(Duration::from_millis(200)).await;
        User {
            id: 3,
            name: "Dave".to_string(),
        }
    };

    // Convert to flow
    let user_flow = load_user.as_flow();

    // Collect multiple times (each collection re-executes the block)
    println!("First collection:");
    user_flow
        .clone()
        .collect(|u| async move { println!("  {:?}", u) })
        .await;

    println!("Second collection:");
    user_flow
        .collect(|u| async move { println!("  {:?}", u) })
        .await;

    println!();
}

#[tokio::main]
async fn main() {
    println!("rs-coroutine/rs-flow Examples\n");

    example_basic_scope().await;
    example_with_dispatcher().await;
    example_parallel_work().await;
    example_cold_flows().await;
    example_flow_operators().await;
    example_shared_flow().await;
    example_state_flow().await;
    example_suspending().await;

    println!("All examples completed!");
}
