use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tokio::sync::Notify;

/// Cooperative cancellation shared by provider requests, agent loops, and tools.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    inner: Arc<CancellationState>,
}

#[derive(Debug, Default)]
struct CancellationState {
    cancelled: AtomicBool,
    notify: Notify,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        if !self.inner.cancelled.swap(true, Ordering::SeqCst) {
            self.inner.notify.notify_waiters();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    pub async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }

            let notified = self.inner.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::CancellationToken;

    #[tokio::test]
    async fn wakes_existing_and_future_waiters() {
        let token = CancellationToken::new();
        let waiting = token.clone();
        let waiter = tokio::spawn(async move { waiting.cancelled().await });

        token.cancel();
        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("existing waiter did not wake")
            .expect("waiter panicked");

        tokio::time::timeout(Duration::from_secs(1), token.cancelled())
            .await
            .expect("future waiter did not observe cancellation");
    }
}
