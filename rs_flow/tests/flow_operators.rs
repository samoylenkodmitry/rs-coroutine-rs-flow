use coroflow::{flow, flow_of, FlowExt};
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::test]
async fn map_and_filter_pipeline_produces_expected_values() {
    let numbers = flow(|collector| async move {
        for value in 0..5 {
            collector.emit(value).await;
        }
    });

    let results = Arc::new(Mutex::new(Vec::new()));
    let results_clone = Arc::clone(&results);

    numbers
        .filter_sync(|value| *value % 2 == 0)
        .map(|value| async move { value * 10 })
        .on_each(|value| assert_eq!(value % 10, 0))
        .collect(move |value| {
            let results = Arc::clone(&results_clone);
            async move {
                results.lock().await.push(value);
            }
        })
        .await;

    let final_values = results.lock().await.clone();
    assert_eq!(final_values, vec![0, 20, 40]);
}

#[tokio::test]
async fn drop_and_take_limit_flow_size() {
    let flow = flow(|collector| async move {
        for value in 1..=6 {
            collector.emit(value).await;
        }
    });

    let results = Arc::new(Mutex::new(Vec::new()));
    let results_clone = Arc::clone(&results);
    flow.drop_first(2)
        .take(2)
        .collect(move |value| {
            let results = Arc::clone(&results_clone);
            async move {
                results.lock().await.push(value);
            }
        })
        .await;

    let final_values = results.lock().await.clone();
    assert_eq!(final_values, vec![3, 4]);
}

#[tokio::test]
async fn flat_map_latest_switches_to_new_flows() {
    // Test that flat_map_latest switches to new inner flows immediately
    // Each upstream value produces an inner flow [value*10, value*10+1]
    let upstream = flow_of!(1, 2, 3);

    let results = Arc::new(Mutex::new(Vec::new()));
    let results_clone = Arc::clone(&results);

    upstream
        .flat_map_latest(|x| async move {
            flow_of!(x * 10, x * 10 + 1)
        })
        .collect(move |value| {
            let results = Arc::clone(&results_clone);
            async move {
                results.lock().await.push(value);
            }
        })
        .await;

    let final_values = results.lock().await.clone();

    // Should contain values from all inner flows
    // The exact output depends on timing, but we should see values from flow(1), flow(2), flow(3)
    // At minimum, we should get the last flow's values: [30, 31]
    assert!(final_values.contains(&30), "Should contain 30 from last flow");
    assert!(final_values.contains(&31), "Should contain 31 from last flow");

    // All values should be from the valid ranges
    for &v in final_values.iter() {
        assert!(
            v == 10 || v == 11 || v == 20 || v == 21 || v == 30 || v == 31,
            "Unexpected value: {}",
            v
        );
    }
}

#[tokio::test]
async fn flat_map_latest_cancels_previous_flow() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::time::{sleep, Duration};

    // Test that flat_map_latest cancels the previous inner flow when a new one arrives
    let upstream = flow(|collector| async move {
        collector.emit(1).await;
        sleep(Duration::from_millis(10)).await; // Give first flow time to start
        collector.emit(2).await;
    });

    let first_flow_aborted = Arc::new(AtomicBool::new(false));
    let first_flow_aborted_clone = Arc::clone(&first_flow_aborted);

    let results = Arc::new(Mutex::new(Vec::new()));
    let results_clone = Arc::clone(&results);

    upstream
        .flat_map_latest(move |x| {
            let flag = Arc::clone(&first_flow_aborted_clone);
            async move {
                if x == 1 {
                    // First inner flow - should be cancelled when second arrives
                    flow(move |collector| {
                        let flag = flag.clone();
                        async move {
                            collector.emit(100).await;
                            // Long sleep - should be interrupted by cancellation
                            sleep(Duration::from_millis(100)).await;
                            // This should not execute because flow is aborted
                            flag.store(true, Ordering::SeqCst);
                            collector.emit(101).await;
                        }
                    })
                } else {
                    // Second inner flow - completes normally
                    flow_of!(200, 201)
                }
            }
        })
        .collect(move |value| {
            let results = Arc::clone(&results_clone);
            async move {
                results.lock().await.push(value);
            }
        })
        .await;

    let final_values = results.lock().await.clone();

    // Should contain first value from first flow (100)
    // But NOT 101 (because first flow was cancelled before completing)
    // Should contain both values from second flow (200, 201)
    assert!(
        final_values.contains(&100),
        "Should have first value from first flow: {:?}",
        final_values
    );
    assert!(
        final_values.contains(&200),
        "Should have first value from second flow: {:?}",
        final_values
    );
    assert!(
        final_values.contains(&201),
        "Should have second value from second flow: {:?}",
        final_values
    );

    // First flow should have been aborted before setting the flag
    assert!(
        !first_flow_aborted.load(Ordering::SeqCst),
        "First flow should have been aborted before completion"
    );
}
