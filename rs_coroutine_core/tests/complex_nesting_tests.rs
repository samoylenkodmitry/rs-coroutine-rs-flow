use rs_coroutine_core::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, advance, pause};

/// Complex nested structure with multiple levels and branches
#[tokio::test]
async fn test_deeply_nested_cancellation() {
    pause(); // Enable time control

    let root = Arc::new(CoroutineScope::new(Dispatchers::main()));
    let level_counters = Arc::new(vec![
        AtomicUsize::new(0), // Level 1
        AtomicUsize::new(0), // Level 2
        AtomicUsize::new(0), // Level 3
        AtomicUsize::new(0), // Level 4
        AtomicUsize::new(0), // Level 5
    ]);

    let counters = Arc::clone(&level_counters);
    let root_clone = Arc::clone(&root);

    // Create a deeply nested structure: Root -> L1 -> L2 -> L3 -> L4 -> L5
    let job = root.launch(async move {
        counters[0].fetch_add(1, Ordering::SeqCst);

        let root_l1 = root_clone.clone();
        let counters = counters.clone();
        let _result = root_clone.with_dispatcher(Dispatchers::io(), async move {
            counters[1].fetch_add(1, Ordering::SeqCst);

            let root_l2 = root_l1.clone();
            let counters = counters.clone();
            let _result = root_l1.with_dispatcher(Dispatchers::io(), async move {
                counters[2].fetch_add(1, Ordering::SeqCst);

                let root_l3 = root_l2.clone();
                let counters = counters.clone();
                let _result = root_l2.with_dispatcher(Dispatchers::io(), async move {
                    counters[3].fetch_add(1, Ordering::SeqCst);

                    let counters = counters.clone();
                    let _result = root_l3.with_dispatcher(Dispatchers::io(), async move {
                        counters[4].fetch_add(1, Ordering::SeqCst);

                        // Deepest level - long running work
                        sleep(Duration::from_secs(1000)).await;
                    }).await;
                }).await;
            }).await;
        }).await;
    });

    // Advance time to let everything start
    advance(Duration::from_millis(10)).await;

    // All levels should have started
    assert_eq!(level_counters[0].load(Ordering::SeqCst), 1, "Level 1 should have started");
    assert_eq!(level_counters[1].load(Ordering::SeqCst), 1, "Level 2 should have started");
    assert_eq!(level_counters[2].load(Ordering::SeqCst), 1, "Level 3 should have started");
    assert_eq!(level_counters[3].load(Ordering::SeqCst), 1, "Level 4 should have started");
    assert_eq!(level_counters[4].load(Ordering::SeqCst), 1, "Level 5 should have started");

    // Cancel root - should cancel all descendants
    root.cancel();

    // Wait for cancellation to propagate
    advance(Duration::from_millis(10)).await;
    job.join().await;

    // Verify cancellation propagated through all levels
    assert!(root.is_cancelled());
}

/// Test multiple sibling branches with different nesting depths
#[tokio::test]
async fn test_complex_tree_cancellation() {
    pause();

    let root = Arc::new(CoroutineScope::new(Dispatchers::main()));
    let branch_status = Arc::new(vec![
        AtomicBool::new(false), // Branch A (shallow)
        AtomicBool::new(false), // Branch B (medium)
        AtomicBool::new(false), // Branch C (deep)
        AtomicBool::new(false), // Branch D (very deep)
    ]);

    let root_clone = Arc::clone(&root);
    let status = Arc::clone(&branch_status);

    // Branch A: Shallow nesting (2 levels)
    let job_a = root.launch(async move {
        let status = status.clone();
        let root = root_clone.clone();
        let _result = root.with_dispatcher(Dispatchers::io(), async move {
            sleep(Duration::from_secs(100)).await;
            status[0].store(true, Ordering::SeqCst);
        }).await;
    });

    // Branch B: Medium nesting (3 levels)
    let root_clone = Arc::clone(&root);
    let status = Arc::clone(&branch_status);
    let job_b = root.launch(async move {
        let status = status.clone();
        let root_l1 = root_clone.clone();
        let _result = root_clone.with_dispatcher(Dispatchers::io(), async move {
            let status = status.clone();
            let _result = root_l1.with_dispatcher(Dispatchers::io(), async move {
                sleep(Duration::from_secs(100)).await;
                status[1].store(true, Ordering::SeqCst);
            }).await;
        }).await;
    });

    // Branch C: Deep nesting (4 levels)
    let root_clone = Arc::clone(&root);
    let status = Arc::clone(&branch_status);
    let job_c = root.launch(async move {
        let status = status.clone();
        let root_l1 = root_clone.clone();
        let _result = root_clone.with_dispatcher(Dispatchers::io(), async move {
            let status = status.clone();
            let root_l2 = root_l1.clone();
            let _result = root_l1.with_dispatcher(Dispatchers::io(), async move {
                let status = status.clone();
                let _result = root_l2.with_dispatcher(Dispatchers::io(), async move {
                    sleep(Duration::from_secs(100)).await;
                    status[2].store(true, Ordering::SeqCst);
                }).await;
            }).await;
        }).await;
    });

    // Branch D: Very deep nesting (5 levels)
    let root_clone = Arc::clone(&root);
    let status = Arc::clone(&branch_status);
    let job_d = root.launch(async move {
        let status = status.clone();
        let root_l1 = root_clone.clone();
        let _result = root_clone.with_dispatcher(Dispatchers::io(), async move {
            let status = status.clone();
            let root_l2 = root_l1.clone();
            let _result = root_l1.with_dispatcher(Dispatchers::io(), async move {
                let status = status.clone();
                let root_l3 = root_l2.clone();
                let _result = root_l2.with_dispatcher(Dispatchers::io(), async move {
                    let status = status.clone();
                    let _result = root_l3.with_dispatcher(Dispatchers::io(), async move {
                        sleep(Duration::from_secs(100)).await;
                        status[3].store(true, Ordering::SeqCst);
                    }).await;
                }).await;
            }).await;
        }).await;
    });

    // Let all branches start
    advance(Duration::from_millis(10)).await;

    // Cancel root before any branch completes
    root.cancel();

    // Wait for all jobs
    advance(Duration::from_millis(10)).await;
    job_a.join().await;
    job_b.join().await;
    job_c.join().await;
    job_d.join().await;

    // None of the branches should have completed
    assert!(!branch_status[0].load(Ordering::SeqCst), "Branch A should not complete");
    assert!(!branch_status[1].load(Ordering::SeqCst), "Branch B should not complete");
    assert!(!branch_status[2].load(Ordering::SeqCst), "Branch C should not complete");
    assert!(!branch_status[3].load(Ordering::SeqCst), "Branch D should not complete");
}

/// Test diamond-shaped dependency graph
#[tokio::test]
async fn test_diamond_dependency_cancellation() {
    pause();

    let root = Arc::new(CoroutineScope::new(Dispatchers::main()));
    let completion_flags = Arc::new(vec![
        AtomicBool::new(false), // Top
        AtomicBool::new(false), // Left path
        AtomicBool::new(false), // Right path
        AtomicBool::new(false), // Bottom
    ]);

    //       Top
    //      /   \
    //   Left   Right
    //      \   /
    //      Bottom

    let flags = Arc::clone(&completion_flags);
    let root_clone = Arc::clone(&root);

    let job = root.launch(async move {
        flags[0].store(true, Ordering::SeqCst);

        let flags_left = flags.clone();
        let flags_right = flags.clone();
        let root_left = root_clone.clone();
        let root_right = root_clone.clone();

        // Left path
        let left_job = CURRENT_SCOPE.with(|scope| {
            scope.launch(async move {
                let _result = root_left.with_dispatcher(Dispatchers::io(), async move {
                    sleep(Duration::from_millis(50)).await;
                    flags_left[1].store(true, Ordering::SeqCst);
                }).await;
            })
        });

        // Right path
        let right_job = CURRENT_SCOPE.with(|scope| {
            scope.launch(async move {
                let _result = root_right.with_dispatcher(Dispatchers::io(), async move {
                    sleep(Duration::from_millis(50)).await;
                    flags_right[2].store(true, Ordering::SeqCst);
                }).await;
            })
        });

        // Wait for both paths
        left_job.join().await;
        right_job.join().await;

        // Bottom - should never reach here due to cancellation
        flags[3].store(true, Ordering::SeqCst);
    });

    // Let top and paths start
    advance(Duration::from_millis(10)).await;

    // Cancel before paths complete
    root.cancel();

    advance(Duration::from_millis(100)).await;
    job.join().await;

    // Top should have started
    assert!(completion_flags[0].load(Ordering::SeqCst));

    // Bottom should not complete due to cancellation
    assert!(!completion_flags[3].load(Ordering::SeqCst));
}

/// Test fan-out pattern with multiple concurrent children
#[tokio::test]
async fn test_fan_out_cancellation() {
    pause();

    let root = Arc::new(CoroutineScope::new(Dispatchers::main()));
    let num_children = 10;
    let completed = Arc::new(AtomicUsize::new(0));

    let root_clone = Arc::clone(&root);
    let completed_clone = Arc::clone(&completed);

    let job = root.launch(async move {
        let mut jobs = Vec::new();

        for i in 0..num_children {
            let completed = completed_clone.clone();
            let root = root_clone.clone();

            let child_job = CURRENT_SCOPE.with(|scope| {
                scope.launch(async move {
                    let root_for_inner = root.clone();
                    let _result = root.with_dispatcher(Dispatchers::io(), async move {
                        // Each child has different nesting depth
                        let depth = i % 3;
                        let completed = completed.clone();
                        let root = root_for_inner.clone();

                        match depth {
                            0 => {
                                sleep(Duration::from_secs(10)).await;
                                completed.fetch_add(1, Ordering::SeqCst);
                            },
                            1 => {
                                let completed = completed.clone();
                                let root_clone_inner = root.clone();
                                let _result = root_clone_inner.with_dispatcher(Dispatchers::io(), async move {
                                    sleep(Duration::from_secs(10)).await;
                                    completed.fetch_add(1, Ordering::SeqCst);
                                }).await;
                            },
                            _ => {
                                let root_l1 = root.clone();
                                let completed = completed.clone();
                                let root_clone_inner = root.clone();
                                let _result = root_clone_inner.with_dispatcher(Dispatchers::io(), async move {
                                    let completed = completed.clone();
                                    let _result = root_l1.with_dispatcher(Dispatchers::io(), async move {
                                        sleep(Duration::from_secs(10)).await;
                                        completed.fetch_add(1, Ordering::SeqCst);
                                    }).await;
                                }).await;
                            }
                        }
                    }).await;
                })
            });

            jobs.push(child_job);
        }

        // Wait for all children
        for job in jobs {
            job.join().await;
        }
    });

    // Let children start
    advance(Duration::from_millis(10)).await;

    // Cancel before any complete
    root.cancel();

    advance(Duration::from_secs(20)).await;
    job.join().await;

    // None should have completed
    assert_eq!(completed.load(Ordering::SeqCst), 0, "No children should complete");
}

/// Test time-controlled cancellation
#[tokio::test]
async fn test_time_controlled_cancellation() {
    pause();

    let root = Arc::new(CoroutineScope::new(Dispatchers::main()));
    let counter = Arc::new(AtomicUsize::new(0));

    let counter_clone = Arc::clone(&counter);
    let root_clone = Arc::clone(&root);

    let job = root.launch(async move {
        for i in 0..100 {
            // Cooperative cancellation check
            if root_clone.is_cancelled() {
                break;
            }
            counter_clone.store(i, Ordering::SeqCst);
            sleep(Duration::from_millis(10)).await;
        }
    });

    // Advance 250ms (should complete 25 iterations)
    advance(Duration::from_millis(250)).await;

    // Cancel
    root.cancel();

    // Advance more time
    advance(Duration::from_millis(1000)).await;
    job.join().await;

    let count = counter.load(Ordering::SeqCst);
    // Should have stopped around 25 iterations
    assert!(count < 100, "Should not complete all iterations, got {}", count);
    assert!(count >= 20, "Should have completed some iterations, got {}", count);
}

/// Test selective cancellation in complex tree
#[tokio::test]
async fn test_selective_branch_cancellation() {
    pause();

    let root = Arc::new(CoroutineScope::new(Dispatchers::main()));
    let flags = Arc::new(vec![
        AtomicBool::new(false), // Branch A
        AtomicBool::new(false), // Branch B
        AtomicBool::new(false), // Branch C
    ]);

    // Branch A - will be cancelled
    let root_clone_a = Arc::clone(&root);
    let flags_a = Arc::clone(&flags);
    let job_a = root.launch(async move {
        let _result = root_clone_a.with_dispatcher(Dispatchers::io(), async move {
            sleep(Duration::from_secs(10)).await;
            flags_a[0].store(true, Ordering::SeqCst);
        }).await;
    });

    // Branch B - will complete
    let root_clone_b = Arc::clone(&root);
    let flags_b = Arc::clone(&flags);
    let job_b = root.launch(async move {
        let _result = root_clone_b.with_dispatcher(Dispatchers::io(), async move {
            sleep(Duration::from_millis(50)).await;
            flags_b[1].store(true, Ordering::SeqCst);
        }).await;
    });

    // Branch C - will be cancelled
    let root_clone_c = Arc::clone(&root);
    let flags_c = Arc::clone(&flags);
    let job_c = root.launch(async move {
        let _result = root_clone_c.with_dispatcher(Dispatchers::io(), async move {
            sleep(Duration::from_secs(10)).await;
            flags_c[2].store(true, Ordering::SeqCst);
        }).await;
    });

    advance(Duration::from_millis(10)).await;

    // Cancel individual jobs instead of root
    job_a.cancel();
    job_c.cancel();

    // Let Branch B complete
    advance(Duration::from_millis(100)).await;

    job_a.join().await;
    job_b.join().await;
    job_c.join().await;

    // Only Branch B should complete
    assert!(!flags[0].load(Ordering::SeqCst), "Branch A should be cancelled");
    assert!(flags[1].load(Ordering::SeqCst), "Branch B should complete");
    assert!(!flags[2].load(Ordering::SeqCst), "Branch C should be cancelled");
}
