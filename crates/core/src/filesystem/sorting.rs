//! Whole-list sort preparation on the existing bounded worker primitive.
use super::work::Workers;
use crate::{
    application::{
        contracts::directory::{Cancellation, DirectoryReader},
        files::sorting::{
            SortCompletion, SortRequest, SortedEntries,
            ordering::{SortValue, order},
        },
    },
    domain::sorting::SortField,
};
use std::sync::Arc;

pub struct SortWorkers {
    workers: Workers<Result<SortedEntries, String>>,
    reader: Arc<dyn DirectoryReader + Send + Sync>,
}
impl SortWorkers {
    pub fn new(reader: Arc<dyn DirectoryReader + Send + Sync>) -> std::io::Result<Self> {
        Ok(Self {
            workers: Workers::new("sorting", 2, 8)?,
            reader,
        })
    }
    pub fn submit(
        &self,
        request: SortRequest,
    ) -> (Cancellation, impl Future<Output = SortCompletion> + use<>) {
        let reader = Arc::clone(&self.reader);
        let job = request.clone();
        let future = self
            .workers
            .submit(move |cancel| prepare(&job, reader.as_ref(), cancel));
        let cancellation = future.cancellation();
        (cancellation, async move {
            SortCompletion {
                request,
                result: future
                    .await
                    .unwrap_or_else(|error| Err(format!("worker {error:?}"))),
            }
        })
    }
}
pub fn prepare(
    request: &SortRequest,
    reader: &dyn DirectoryReader,
    cancel: &Cancellation,
) -> Result<SortedEntries, String> {
    let settings = request.settings;
    let entries = request.listing.entries();
    let mut rows = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err("cancelled".into());
        }
        if entry.hidden && !request.show_hidden {
            continue;
        }
        let metadata = if matches!(settings.field, SortField::Modified | SortField::Size) {
            reader.sort_metadata(&entry.path, cancel).ok()
        } else {
            None
        };
        rows.push(SortValue { index, metadata });
    }
    if cancel.is_cancelled() {
        return Err("cancelled".into());
    }
    let result = order(request, rows);
    if cancel.is_cancelled() {
        return Err("cancelled".into());
    }
    Ok(result)
}
