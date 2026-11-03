//! One coalesced quit request and one async waiter. Native callbacks never wait.
use iced::futures::task::AtomicWaker;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::{Context, Poll};

pub struct QuitSignal {
    /// Odd epochs accept requests; changing acceptance invalidates queued receipts.
    epoch: AtomicU64,
    pending: AtomicU64,
    stopped: AtomicBool,
    waker: AtomicWaker,
}

impl Default for QuitSignal {
    fn default() -> Self {
        Self {
            epoch: AtomicU64::new(1),
            pending: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
            waker: AtomicWaker::new(),
        }
    }
}

impl QuitSignal {
    #[cfg(all(target_os = "macos", not(test)))]
    pub(super) fn request(&self) {
        let epoch = self.epoch.load(Ordering::Acquire);
        if epoch.is_multiple_of(2) || self.stopped.load(Ordering::Acquire) {
            return;
        }
        self.pending.store(epoch, Ordering::Release);
        self.waker.wake();
    }

    pub fn accepts(&self, epoch: u64) -> bool {
        !epoch.is_multiple_of(2)
            && self.epoch.load(Ordering::Acquire) == epoch
            && !self.stopped.load(Ordering::Acquire)
    }

    pub fn set_accepting(&self, accepting: bool) {
        let epoch = self.epoch.load(Ordering::Acquire);
        if !epoch.is_multiple_of(2) == accepting {
            return;
        }
        let Some(next) = epoch.checked_add(1) else {
            self.stop();
            return;
        };
        self.epoch.store(next, Ordering::Release);
        // Only the runtime changes acceptance. Callback races remain epoch-tagged.
        self.waker.wake();
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.waker.wake();
    }

    pub fn next(self: &Arc<Self>) -> QuitRequest {
        QuitRequest(Arc::clone(self))
    }
}

pub struct QuitRequest(Arc<QuitSignal>);
impl Future for QuitRequest {
    type Output = Option<u64>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.waker.register(cx.waker());
        if self.0.stopped.load(Ordering::Acquire) {
            return Poll::Ready(None);
        }
        let epoch = self.0.pending.swap(0, Ordering::AcqRel);
        if self.0.accepts(epoch) {
            Poll::Ready(Some(epoch))
        } else {
            Poll::Pending
        }
    }
}
