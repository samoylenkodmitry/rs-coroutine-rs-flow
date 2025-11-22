use rs_flow::hot::{SharedFlow, StateFlow};
use rs_flow::{flow, Flow};
use tokio::sync::oneshot;
use tokio::time::Duration;

#[tokio::test(flavor = "multi_thread")]
async fn map_filter_take_collects_expected_values() {
    let numbers: Flow<i32> = flow(|collector| async move {
        for i in 0..10 {
            collector.emit(i).await;
        }
    });

    let filtered = numbers
        .map(|value| async move { value * 2 })
        .filter(|value| {
            let copy = *value;
            async move { copy % 4 == 0 }
        })
        .take(3);

    let collected = collect_flow(filtered).await;

    assert_eq!(collected, vec![0, 4, 8]);
}

#[tokio::test(flavor = "multi_thread")]
async fn shared_flow_multicasts() {
    let shared = SharedFlow::new(4);
    let (tx, rx) = oneshot::channel();

    tokio::spawn({
        let flow = shared.as_flow().take(3);
        async move {
            let values = collect_flow(flow).await;
            let _ = tx.send(values);
        }
    });

    tokio::time::sleep(Duration::from_millis(10)).await;
    shared.emit(1).await;
    shared.emit(2).await;
    shared.emit(3).await;

    drop(shared);

    let values = tokio::time::timeout(Duration::from_secs(1), rx)
        .await
        .expect("collector timed out")
        .expect("collector should finish");
    assert_eq!(values, vec![1, 2, 3]);
}

#[tokio::test(flavor = "multi_thread")]
async fn state_flow_emits_initial_and_updates() {
    let state = StateFlow::new(0);
    let (tx, rx) = oneshot::channel();

    tokio::spawn({
        let flow = state.as_flow().take(2);
        async move {
            let values = collect_flow(flow).await;
            let _ = tx.send(values);
        }
    });

    tokio::time::sleep(Duration::from_millis(10)).await;
    state.emit(1).await;

    let values = tokio::time::timeout(Duration::from_secs(1), rx)
        .await
        .expect("collector timed out")
        .expect("collector should finish");
    assert_eq!(values, vec![0, 1]);
}

async fn collect_flow<T: Send + 'static + std::fmt::Debug>(flow: Flow<T>) -> Vec<T> {
    let acc = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let handle_acc = acc.clone();

    flow.collect(move |value| {
        let acc = handle_acc.clone();
        async move {
            acc.lock().await.push(value);
        }
    })
    .await;

    std::sync::Arc::try_unwrap(acc)
        .expect("no other refs remain")
        .into_inner()
}
