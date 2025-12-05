use rs_coroutine_core::{CoroutineScope, Dispatchers};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::time::{sleep, Duration};

#[tokio::test]
async fn launched_coroutines_complete() {
    let scope = CoroutineScope::new(Dispatchers::main());
    let flag = Arc::new(AtomicBool::new(false));
    let flag_clone = Arc::clone(&flag);

    let job = scope.launch(async move {
        flag_clone.store(true, Ordering::SeqCst);
    });

    job.join().await;
    assert!(flag.load(Ordering::SeqCst));
}

#[tokio::test]
async fn cancellation_stops_future_progress() {
    let scope = CoroutineScope::new(Dispatchers::main());
    let flag = Arc::new(AtomicBool::new(false));
    let flag_clone = Arc::clone(&flag);

    let job = scope.launch(async move {
        sleep(Duration::from_millis(50)).await;
        flag_clone.store(true, Ordering::SeqCst);
    });

    scope.cancel();
    job.join().await;

    assert!(!flag.load(Ordering::SeqCst));
}
