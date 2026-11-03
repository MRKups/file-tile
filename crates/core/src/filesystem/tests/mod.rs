mod controlled;

pub use controlled::ControlledDirectory;

#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::future::Future;
#[cfg(test)]
use std::io;
#[cfg(test)]
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::pin::Pin;
#[cfg(test)]
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
#[cfg(test)]
use std::task::{Context, Poll, Wake, Waker};

#[cfg(test)]
pub(crate) fn wait<F: Future + Unpin>(mut future: F) -> F::Output {
    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    loop {
        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

#[cfg(test)]
pub(crate) struct Fixture {
    root: PathBuf,
}

#[cfg(test)]
impl Fixture {
    pub(crate) fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "file-tile-native-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root)?;
        Ok(Self { root })
    }
    pub(crate) fn path(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!("Native fixture cleanup failed: {error}");
        }
    }
}
