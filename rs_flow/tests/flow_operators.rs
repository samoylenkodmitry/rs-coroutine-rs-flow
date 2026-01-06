use coroflow::{flow_fn, FlowExt};
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::test]
async fn map_and_filter_pipeline_produces_expected_values() {
    let numbers = flow_fn(|collector| async move {
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
    use coroflow::{CoroutineScope, Dispatcher};

    let flow = flow_fn(|collector| async move {
        for value in 1..=6 {
            collector.emit(value).await;
        }
    });

    let results = Arc::new(Mutex::new(Vec::new()));
    let results_clone = Arc::clone(&results);

    let scope = CoroutineScope::new(Dispatcher::default());
    let results_clone2 = Arc::clone(&results);
    scope
        .launch(async move {
            flow.drop_first(2)
                .take(2)
                .collect(move |value| {
                    let results = Arc::clone(&results_clone);
                    async move {
                        results.lock().await.push(value);
                    }
                })
                .await;
        })
        .join()
        .await;

    let final_values = results_clone2.lock().await.clone();
    assert_eq!(final_values, vec![3, 4]);
}
