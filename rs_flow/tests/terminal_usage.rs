use rs_flow::{flow, FlowCollector, FlowError, FlowTerminal};

#[tokio::test]
async fn terminal_collectors_handle_counts_and_aggregation() {
    let series = flow(|collector| async move {
        for value in [1, 2, 3] {
            collector.emit(value).await;
        }
    });

    let first = series.clone().first().await.expect("flow should emit");
    assert_eq!(first, 1);

    let total = series
        .clone()
        .reduce(|mut acc, value| {
            acc += value;
            acc
        })
        .await
        .expect("reduce should succeed");
    assert_eq!(total, 6);

    let count = series.count().await;
    assert_eq!(count, 3);
}

#[tokio::test]
async fn single_reports_errors_on_extra_elements() {
    let with_two = flow(|collector| async move {
        collector.emit(1).await;
        collector.emit(2).await;
    });

    let error = with_two
        .single()
        .await
        .expect_err("should fail with more than one element");
    assert_eq!(error, FlowError::MoreThanOneElement);

    let empty_flow = flow(|_: FlowCollector<i32>| async move {});
    assert!(empty_flow.single().await.is_err());
}
