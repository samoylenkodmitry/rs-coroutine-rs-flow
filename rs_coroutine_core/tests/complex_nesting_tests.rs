//! Complex nesting tests for hierarchical cancellation
//!
//! These tests verify that structured concurrency works correctly with:
//! - Deep nesting (5+ levels)
//! - Diamond dependencies
//! - Fan-out patterns
//! - Selective cancellation
//! - Mixed cooperative and forced cancellation

use rs_coroutine_core::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// Test deeply nested scopes (5 levels) with cancellation propagating from root
#[tokio::test]
async fn test_deeply_nested_cancellation() {
    let root = Arc::new(CoroutineScope::new(Dispatchers::main()));
    let counters = Arc::new([
        AtomicUsize::new(0), // Level 0 (root)
        AtomicUsize::new(0), // Level 1
        AtomicUsize::new(0), // Level 2
        AtomicUsize::new(0), // Level 3
        AtomicUsize::new(0), // Level 4
    ]);

    let counters_clone = Arc::clone(&counters);
    let root_clone = Arc::clone(&root);

    let job = root.launch(async move {
        counters_clone[0].fetch_add(1, Ordering::SeqCst);

        let root_1 = Arc::clone(&root_clone);
        let c1 = Arc::clone(&counters_clone);
        let level1_result = root_clone
            .with_dispatcher(Dispatchers::io(), async move {
                c1[1].fetch_add(1, Ordering::SeqCst);

                let root_2 = Arc::clone(&root_1);
                let c2 = Arc::clone(&c1);
                let level2_result = root_1
                    .with_dispatcher(Dispatchers::io(), async move {
                        c2[2].fetch_add(1, Ordering::SeqCst);

                        let root_3 = Arc::clone(&root_2);
                        let c3 = Arc::clone(&c2);
                        let level3_result = root_2
                            .with_dispatcher(Dispatchers::io(), async move {
                                c3[3].fetch_add(1, Ordering::SeqCst);

                                let c4 = Arc::clone(&c3);
                                let level4_result = root_3
                                    .with_dispatcher(Dispatchers::io(), async move {
                                        c4[4].fetch_add(1, Ordering::SeqCst);

                                        // Deepest level - simulate work with cooperative cancellation
                                        for _ in 0..10 {
                                            if check_cancellation().is_err() {
                                                return Err("cancelled at level 4");
                                            }
                                            sleep(Duration::from_millis(5)).await;
                                        }
                                        Ok("completed level 4")
                                    })
                                    .await;

                                match level4_result {
                                    Ok(Ok(_)) => Ok("completed level 3"),
                                    _ => Err("cancelled at level 3"),
                                }
                            })
                            .await;

                        match level3_result {
                            Ok(Ok(_)) => Ok("completed level 2"),
                            _ => Err("cancelled at level 2"),
                        }
                    })
                    .await;

                match level2_result {
                    Ok(Ok(_)) => Ok("completed level 1"),
                    _ => Err("cancelled at level 1"),
                }
            })
            .await;

        // Verify the result indicates cancellation
        assert!(level1_result.is_err() || level1_result.unwrap().is_err());
    });

    // Let all levels start
    sleep(Duration::from_millis(10)).await;

    // Cancel from root - should propagate to all 5 levels
    root.cancel();

    // Wait for completion - no hacky sleeps!
    job.join().await;

    // All levels should have started
    assert_eq!(counters[0].load(Ordering::SeqCst), 1, "Level 0 should start");
    assert_eq!(counters[1].load(Ordering::SeqCst), 1, "Level 1 should start");
    assert_eq!(counters[2].load(Ordering::SeqCst), 1, "Level 2 should start");
    assert_eq!(counters[3].load(Ordering::SeqCst), 1, "Level 3 should start");
    assert_eq!(counters[4].load(Ordering::SeqCst), 1, "Level 4 should start");

    // Root should be cancelled
    assert!(root.is_cancelled());
}

/// Test diamond dependency pattern: A -> B,C -> D
/// When A is cancelled, both B and C should cancel, which should cancel D
#[tokio::test]
async fn test_diamond_dependency_cancellation() {
    let root = Arc::new(CoroutineScope::new(Dispatchers::main()));
    let visited = Arc::new([
        AtomicBool::new(false), // Node A (root)
        AtomicBool::new(false), // Node B
        AtomicBool::new(false), // Node C
        AtomicBool::new(false), // Node D
    ]);

    let visited_clone = Arc::clone(&visited);
    let root_a = Arc::clone(&root);

    let job_a = root.launch(async move {
        visited_clone[0].store(true, Ordering::SeqCst);

        let root_b = Arc::clone(&root_a);
        let visited_b = Arc::clone(&visited_clone);
        let visited_c = Arc::clone(&visited_clone);

        // Launch B and C in parallel
        let task_b = root_a.async_task(Dispatchers::io(), async move {
            visited_b[1].store(true, Ordering::SeqCst);

            // B spawns D
            let visited_d1 = Arc::clone(&visited_b);
            root_b
                .with_dispatcher(Dispatchers::io(), async move {
                    visited_d1[3].store(true, Ordering::SeqCst);

                    // D does work
                    for _ in 0..10 {
                        if check_cancellation().is_err() {
                            return;
                        }
                        sleep(Duration::from_millis(5)).await;
                    }
                })
                .await
        });

        let task_c = root_a.async_task(Dispatchers::io(), async move {
            visited_c[2].store(true, Ordering::SeqCst);

            // C also depends on root, works independently
            for _ in 0..10 {
                if check_cancellation().is_err() {
                    return;
                }
                sleep(Duration::from_millis(5)).await;
            }
        });

        // Wait for both branches
        let _ = futures::join!(task_b.await_result(), task_c.await_result());
    });

    // Let diamond form
    sleep(Duration::from_millis(10)).await;

    // Cancel A - should cancel B, C, and D
    root.cancel();
    job_a.join().await;

    // All nodes should have been visited
    assert!(visited[0].load(Ordering::SeqCst), "Node A should execute");
    assert!(visited[1].load(Ordering::SeqCst), "Node B should execute");
    assert!(visited[2].load(Ordering::SeqCst), "Node C should execute");
    assert!(visited[3].load(Ordering::SeqCst), "Node D should execute");

    // Root should be cancelled
    assert!(root.is_cancelled());
}

/// Test fan-out pattern: one parent spawns many children
#[tokio::test]
async fn test_fan_out_cancellation() {
    let root = Arc::new(CoroutineScope::new(Dispatchers::main()));
    let completed = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(AtomicUsize::new(0));

    let root_clone = Arc::clone(&root);
    let completed_clone = Arc::clone(&completed);
    let started_clone = Arc::clone(&started);

    let job = root.launch(async move {
        // Spawn 10 parallel tasks
        let mut tasks = Vec::new();

        for i in 0..10 {
            let scope = Arc::clone(&root_clone);
            let completed_inner = Arc::clone(&completed_clone);
            let started_inner = Arc::clone(&started_clone);

            let task = scope.async_task(Dispatchers::io(), async move {
                started_inner.fetch_add(1, Ordering::SeqCst);

                // Each task does work
                for _ in 0..20 {
                    if check_cancellation().is_err() {
                        return i;
                    }
                    sleep(Duration::from_millis(2)).await;
                }

                completed_inner.fetch_add(1, Ordering::SeqCst);
                i
            });

            tasks.push(task);
        }

        // Wait for all tasks
        for task in tasks {
            let _ = task.await_result().await;
        }
    });

    // Let tasks start
    sleep(Duration::from_millis(10)).await;

    // Cancel root - should cancel all 10 children
    root.cancel();
    job.join().await;

    // All tasks should have started
    assert_eq!(started.load(Ordering::SeqCst), 10, "All 10 tasks should start");

    // Most/all tasks should NOT have completed (cancelled mid-flight)
    let completed_count = completed.load(Ordering::SeqCst);
    assert!(
        completed_count < 10,
        "Most tasks should be cancelled, but {} completed",
        completed_count
    );
}

/// Test cancellation propagation through multiple dispatcher switches
#[tokio::test]
async fn test_dispatcher_hopping_cancellation() {
    let root = Arc::new(CoroutineScope::new(Dispatchers::main()));
    let hops = Arc::new(AtomicUsize::new(0));

    let root_clone = Arc::clone(&root);
    let hops_clone = Arc::clone(&hops);

    let job = root.launch(async move {
        let root1 = Arc::clone(&root_clone);
        hops_clone.fetch_add(1, Ordering::SeqCst); // Hop 0: main

        let result1 = root_clone
            .with_dispatcher(Dispatchers::io(), async move {
                let root2 = Arc::clone(&root1);
                hops_clone.fetch_add(1, Ordering::SeqCst); // Hop 1: io

                root1
                    .with_dispatcher(Dispatchers::main(), async move {
                        let root3 = Arc::clone(&root2);
                        hops_clone.fetch_add(1, Ordering::SeqCst); // Hop 2: main

                        root2
                            .with_dispatcher(Dispatchers::io(), async move {
                                hops_clone.fetch_add(1, Ordering::SeqCst); // Hop 3: io

                                root3
                                    .with_dispatcher(Dispatchers::main(), async move {
                                        hops_clone.fetch_add(1, Ordering::SeqCst); // Hop 4: main

                                        // Do work on 5th dispatcher
                                        for _ in 0..20 {
                                            if check_cancellation().is_err() {
                                                return;
                                            }
                                            sleep(Duration::from_millis(3)).await;
                                        }
                                    })
                                    .await
                            })
                            .await
                    })
                    .await
            })
            .await;

        // Should be cancelled
        assert!(result1.is_err());
    });

    // Let dispatcher hops execute
    sleep(Duration::from_millis(15)).await;

    // Cancel root
    root.cancel();
    job.join().await;

    // Should have hopped through all 5 dispatchers
    let hop_count = hops.load(Ordering::SeqCst);
    assert_eq!(hop_count, 5, "Should hop through 5 dispatchers");
}
