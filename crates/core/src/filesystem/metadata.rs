//! Bounded file stats, independent of directory and persistence worker admission.
use super::work::{WorkError, WorkFuture, Workers};
use crate::application::contracts::directory::{Cancellation, DirectoryReader};
use crate::application::files::metadata::{
    BATCH_LIMIT, FileMetadata, MetadataCompletion, MetadataError, MetadataRequest,
};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
type Results = Vec<(std::path::PathBuf, Result<FileMetadata, MetadataError>)>;
pub struct MetadataWorkers {
    workers: Workers<Results>,
    reader: Arc<dyn DirectoryReader + Send + Sync>,
}
impl MetadataWorkers {
    pub fn new(reader: Arc<dyn DirectoryReader + Send + Sync>) -> std::io::Result<Self> {
        Ok(Self {
            workers: Workers::new("metadata", 2, 8)?,
            reader,
        })
    }
    pub fn submit(&self, request: MetadataRequest) -> MetadataFuture {
        let paths = request.paths.clone();
        let reader = Arc::clone(&self.reader);
        let future = self.workers.submit(move |cancellation| {
            if paths.len() > BATCH_LIMIT {
                return paths
                    .into_iter()
                    .map(|path| (path, Err(MetadataError::Unsupported)))
                    .collect();
            }
            paths
                .into_iter()
                .map(|path| {
                    let value = reader.metadata(&path, cancellation);
                    (path, value)
                })
                .collect()
        });
        MetadataFuture { request, future }
    }
}
pub struct MetadataFuture {
    request: MetadataRequest,
    future: WorkFuture<Results>,
}
impl MetadataFuture {
    pub fn cancellation(&self) -> Cancellation {
        self.future.cancellation()
    }
}
impl Future for MetadataFuture {
    type Output = MetadataCompletion;
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        Pin::new(&mut this.future)
            .poll(context)
            .map(|result| MetadataCompletion {
                request: this.request.clone(),
                result: result.map_err(|error| match error {
                    WorkError::Busy => MetadataError::Busy,
                    WorkError::Cancelled => MetadataError::Cancelled,
                    WorkError::Unavailable => MetadataError::Unavailable,
                }),
            })
    }
}
