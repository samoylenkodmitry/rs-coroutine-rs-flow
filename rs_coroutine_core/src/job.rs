use tokio_util::sync::CancellationToken;

/// JobHandle tracks a coroutine in the cancellation tree
#[derive(Clone)]
pub struct JobHandle {
    cancel_token: CancellationToken,
}

impl JobHandle {
    pub fn new() -> Self {
        Self {
            cancel_token: CancellationToken::new(),
        }
    }

    pub fn new_child(&self) -> Self {
        Self {
            cancel_token: self.cancel_token.child_token(),
        }
    }

    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    pub async fn join(&self) {
        self.cancel_token.cancelled().await;
    }

    pub fn token(&self) -> &CancellationToken {
        &self.cancel_token
    }
}

impl Default for JobHandle {
    fn default() -> Self {
        Self::new()
    }
}
