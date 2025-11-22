use rs_coroutine_core::{CoroutineScope, Dispatchers};
use rs_flow::hot::{SharedFlow, StateFlow};
use tokio::time::Duration;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let scope = CoroutineScope::new(Dispatchers::main());

    let counter = StateFlow::new(0);
    let events = SharedFlow::new(4);

    let (ui_tx, ui_rx) = tokio::sync::oneshot::channel();
    scope.launch({
        let stream = counter.as_flow().take(3).map(|value| async move { format!("count={value}") });
        let ui_tx = ui_tx;
        async move {
            stream
                .collect(|line| async move {
                    println!("[UI] StateFlow update: {line}");
                })
                .await;
            let _ = ui_tx.send(());
        }
    });

    scope.launch({
        let events = events.clone();
        async move {
            tokio::time::sleep(Duration::from_millis(25)).await;
            events.emit("first").await;
            events.emit("second").await;
            events.emit("third").await;
        }
    });

    let (log_tx, log_rx) = tokio::sync::oneshot::channel();
    scope.launch({
        let flow = events.as_flow().take(3);
        let log_tx = log_tx;
        async move {
            flow
                .collect(|msg| async move {
                    println!("[Logger] shared flow message: {msg}");
                })
                .await;
            let _ = log_tx.send(());
        }
    });

    let computation = scope
        .with_dispatcher(Dispatchers::io(), || async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            41 + 1
        })
        .await;
    println!("Computed on IO dispatcher: {computation}");

    counter.emit(1).await;
    counter.emit(2).await;

    let _ = tokio::join!(ui_rx, log_rx);

    println!("All example tasks finished");
}
