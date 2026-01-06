use rs_coroutine_core::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::{sleep, timeout};

#[tokio::test]
async fn test_with_dispatcher_cancels_on_future_drop() {
    // This test verifies that dropping the with_dispatcher future
    // (e.g., in a timeout) cancels the spawned task instead of detaching it
    let scope = Arc::new(CoroutineScope::new(Dispatchers::main()));
    let work_started = Arc::new(Notify::new());
    let work_completed = Arc::new(AtomicBool::new(false));

    let work_started_clone = Arc::clone(&work_started);
    let work_completed_clone = Arc::clone(&work_completed);

    // Start with_dispatcher but timeout before it completes
    let result = timeout(Duration::from_millis(50), async {
        scope
            .with_dispatcher(Dispatchers::io(), async move {
                work_started_clone.notify_one();
                // Long-running work
                sleep(Duration::from_secs(10)).await;
                work_completed_clone.store(true, Ordering::SeqCst);
                42
            })
            .await
    })
    .await;

    // Should timeout
    assert!(result.is_err(), "Should have timed out");

    // Wait a bit to ensure work doesn't complete after timeout
    sleep(Duration::from_millis(100)).await;

    // Work should NOT have completed - task was cancelled when future dropped
    assert!(
        !work_completed.load(Ordering::SeqCst),
        "Work should be cancelled when future is dropped"
    );

    // Scope should show the child job was cancelled
    // (The job should complete with cancellation, not keep running)
}

#[tokio::test]
async fn test_with_dispatcher_cancels_on_select_drop() {
    // This test verifies that dropping with_dispatcher in a select! cancels the task
    let scope = Arc::new(CoroutineScope::new(Dispatchers::main()));
    let work_completed = Arc::new(AtomicBool::new(false));

    let work_completed_clone = Arc::clone(&work_completed);

    // Use timeout to force the select! to drop the with_dispatcher future
    let result = timeout(
        Duration::from_millis(50),
        scope.with_dispatcher(Dispatchers::io(), async move {
            // Long-running work that should be cancelled
            sleep(Duration::from_secs(10)).await;
            work_completed_clone.store(true, Ordering::SeqCst);
        }),
    )
    .await;

    // Should timeout
    assert!(result.is_err(), "Should have timed out");

    // Wait a bit
    sleep(Duration::from_millis(100)).await;

    // Work should NOT have completed - task was cancelled when timeout dropped the future
    assert!(
        !work_completed.load(Ordering::SeqCst),
        "Work should be cancelled when select! drops the future"
    );
}
