use rs_coroutine_core::{with_current_scope, CoroutineScope, Dispatchers};
use tokio::sync::oneshot;

#[tokio::test(flavor = "multi_thread")]
async fn launch_installs_scope_and_runs() {
    let scope = CoroutineScope::new(Dispatchers::main());
    let (tx, rx) = oneshot::channel();

    scope.launch(async move {
        let in_scope = with_current_scope(|scope| {
            let job = scope.job.clone();
            async move { job.is_cancelled() }
        })
        .await;
        let _ = tx.send(in_scope);
    });

    let seen = rx.await.expect("task should complete");
    assert!(!seen);
}

#[tokio::test(flavor = "multi_thread")]
async fn async_task_returns_value() {
    let scope = CoroutineScope::new(Dispatchers::main());

    let result = scope
        .async_task(Dispatchers::io(), || async move { 2 + 2 })
        .await;

    assert_eq!(result, 4);
}
