//! Bounded renderer resources and demand-driven image tasks, without navigation state.
use crate::application::{
    LoadState,
    contracts::directory::{Cancellation, DirectoryListing, EntryKind, ListingOrigin},
    contracts::preview::{PreviewCompletion, PreviewData, PreviewRequest, image_candidate},
};
use crate::domain::{FileView, ViewId};
use crate::{Desktop, Message, boxes::file_list};
use iced::{Element, Task};
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
};

const CACHE_LIMIT: usize = 256;
const DEMAND_LIMIT: usize = 128;
const IN_FLIGHT_LIMIT: usize = 4;
type Key = (ViewId, u64, PathBuf);
pub(crate) type Values = HashMap<Key, Option<Result<iced::widget::image::Handle, String>>>;
struct Pending {
    cancellation: Cancellation,
    handle: iced::task::Handle,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.handle.abort();
    }
}
#[derive(Default)]
pub(crate) struct Runtime {
    pub values: Values,
    pub workers: Option<Arc<crate::filesystem::PreviewWorkers>>,
    pending: HashMap<Key, Pending>,
    queue: VecDeque<Key>,
    order: VecDeque<Key>,
}

impl Runtime {
    pub fn clear(&mut self) {
        self.pending.clear();
        self.values.clear();
        self.queue.clear();
        self.order.clear();
    }
}

impl Desktop {
    pub(crate) fn observe_thumbnail_demand(&mut self, action: &crate::application::Action) {
        if let crate::application::Action::MetadataNeeded(view, paths) = action {
            self.demand_thumbnails(*view, paths);
        }
    }
    pub(crate) fn demand_thumbnails(&mut self, view: ViewId, paths: &[PathBuf]) {
        if self.thumbnails.workers.is_none() || !self.application.workspace().can_browse(view) {
            return;
        }
        let Some(item) = self.application.workspace().view(view) else {
            return;
        };
        let menu_target = self
            .entry_menu
            .as_ref()
            .filter(|menu| menu.view == view)
            .map(|menu| menu.path.as_path());
        if item.settings.file_view == FileView::List && menu_target.is_none() {
            return;
        }
        let Some(listing) = self.application.load(view).and_then(LoadState::listing) else {
            return;
        };
        let primary_local_ok = local_thumbnails_allowed(listing);
        let preview_local_ok = self
            .column_previews
            .retained_listing(view)
            .as_deref()
            .is_some_and(local_thumbnails_allowed);
        let revision = self
            .views
            .get(&view)
            .map_or(0, |state| state.listing_revision);
        let selected = self
            .views
            .get(&view)
            .and_then(|state| state.selected.as_ref());
        let supported: Vec<_> = paths
            .iter()
            .filter(|path| {
                image_candidate(path)
                    && listing
                        .entry(path)
                        .filter(|_| primary_local_ok)
                        .or_else(|| {
                            preview_local_ok
                                .then(|| self.column_previews.entry(view, path))
                                .flatten()
                        })
                        .is_some_and(|entry| {
                            entry.kind == EntryKind::File
                                && !entry.dataless
                                && (item.settings.show_hidden || !entry.hidden)
                        })
                    && (item.settings.file_view == FileView::Grid
                        || selected == Some(*path)
                        || menu_target == Some(path.as_path()))
            })
            .take(DEMAND_LIMIT)
            .cloned()
            .collect();
        self.prune_queued_view_thumbnails(view, revision, &supported);
        for path in supported {
            self.enqueue_thumbnail((view, revision, path), false);
        }
    }

    pub(crate) fn demand_single_thumbnail(&mut self, view: ViewId, path: &Path) {
        if self.thumbnails.workers.is_none()
            || !self.application.workspace().can_browse(view)
            || !image_candidate(path)
        {
            return;
        }
        let Some(item) = self.application.workspace().view(view) else {
            return;
        };
        let menu_target = self
            .entry_menu
            .as_ref()
            .filter(|menu| menu.view == view)
            .map(|menu| menu.path.as_path());
        if item.settings.file_view == FileView::List && menu_target.is_none() {
            return;
        }
        let Some(listing) = self.application.load(view).and_then(LoadState::listing) else {
            return;
        };
        let primary_local_ok = local_thumbnails_allowed(listing);
        let preview_local_ok = self
            .column_previews
            .retained_listing(view)
            .as_deref()
            .is_some_and(local_thumbnails_allowed);
        let valid = listing
            .entry(path)
            .filter(|_| primary_local_ok)
            .or_else(|| {
                preview_local_ok
                    .then(|| self.column_previews.entry(view, path))
                    .flatten()
            })
            .is_some_and(|entry| {
                entry.kind == EntryKind::File
                    && !entry.dataless
                    && (item.settings.show_hidden || !entry.hidden)
            });
        if !valid {
            return;
        }
        let revision = self
            .views
            .get(&view)
            .map_or(0, |state| state.listing_revision);
        self.enqueue_thumbnail((view, revision, path.to_path_buf()), true);
    }

    fn enqueue_thumbnail(&mut self, key: Key, front: bool) {
        if self.thumbnails.values.contains_key(&key) {
            if front
                && let Some(pos) = self.thumbnails.queue.iter().position(|item| item == &key)
                && pos > 0
                && let Some(queued) = self.thumbnails.queue.remove(pos)
            {
                self.thumbnails.queue.push_front(queued);
            }
            return;
        }
        while self.thumbnails.values.len() >= CACHE_LIMIT {
            let Some(oldest) = self.thumbnails.order.pop_front() else {
                break;
            };
            self.thumbnails.values.remove(&oldest);
            self.thumbnails.pending.remove(&oldest);
            self.thumbnails.queue.retain(|item| item != &oldest);
        }
        self.thumbnails.order.push_back(key.clone());
        self.thumbnails.values.insert(key.clone(), None);
        if front {
            self.thumbnails.queue.push_front(key);
        } else {
            self.thumbnails.queue.push_back(key);
        }
    }

    fn prune_queued_view_thumbnails(&mut self, view: ViewId, revision: u64, supported: &[PathBuf]) {
        let mut evicted = Vec::new();
        self.thumbnails.queue.retain(|key| {
            let keep =
                key.0 != view || (key.1 == revision && supported.iter().any(|path| path == &key.2));
            if !keep {
                evicted.push(key.clone());
            }
            keep
        });
        for key in evicted {
            if matches!(self.thumbnails.values.get(&key), Some(None))
                && !self.thumbnails.pending.contains_key(&key)
            {
                self.thumbnails.values.remove(&key);
                self.thumbnails.order.retain(|item| item != &key);
            }
        }
    }

    pub(crate) fn thumbnail_completed(&mut self, receipt: PreviewCompletion) -> Task<Message> {
        let key = (
            receipt.request.view,
            receipt.request.generation,
            receipt.request.path,
        );
        if self.thumbnails.pending.remove(&key).is_some()
            && self.thumbnails.values.contains_key(&key)
        {
            let value = receipt.result.and_then(|data| {
                if !data.valid() {
                    return Err("Thumbnail has invalid pixel data. Use Open.".into());
                }
                let PreviewData::Image {
                    width,
                    height,
                    rgba,
                } = data;
                if width > 256 || height > 256 {
                    return Err("Thumbnail exceeds its display size limit. Use Open.".into());
                }
                Ok(iced::widget::image::Handle::from_rgba(
                    width,
                    height,
                    iced::advanced::graphics::core::Bytes::from_owner(rgba),
                ))
            });
            self.thumbnails.values.insert(key, Some(value));
        }
        self.fill_thumbnails()
    }

    pub(crate) fn fill_thumbnails(&mut self) -> Task<Message> {
        let valid = |key: &Key| {
            let is_menu_target = self
                .entry_menu
                .as_ref()
                .is_some_and(|menu| menu.view == key.0 && menu.path == key.2);
            self.application.workspace().is_active(key.0)
                && self
                    .application
                    .workspace()
                    .view(key.0)
                    .is_some_and(|v| v.settings.file_view != FileView::List || is_menu_target)
                && self
                    .views
                    .get(&key.0)
                    .is_some_and(|state| state.listing_revision == key.1)
                && self
                    .application
                    .load(key.0)
                    .and_then(LoadState::listing)
                    .is_some_and(|listing| {
                        listing.entry(&key.2).is_some()
                            || self.column_previews.entry(key.0, &key.2).is_some()
                    })
        };
        self.thumbnails.values.retain(|key, _| valid(key));
        self.thumbnails.pending.retain(|key, _| valid(key));
        self.thumbnails.queue.retain(|key| valid(key));
        self.thumbnails.order.retain(|key| valid(key));
        let Some(workers) = &self.thumbnails.workers else {
            return Task::none();
        };
        let mut tasks = vec![];
        while self.thumbnails.pending.len() < IN_FLIGHT_LIMIT {
            let Some(key) = self.thumbnails.queue.pop_front() else {
                break;
            };
            let request = PreviewRequest {
                view: key.0,
                generation: key.1,
                path: key.2.clone(),
            };
            let future = workers.read(request);
            let cancellation = future.cancellation();
            let epoch = self.application.workspace_epoch();
            let (task, handle) = Task::perform(future, move |receipt| {
                Message::WorkspaceEpoch(epoch, Box::new(Message::ThumbnailCompleted(receipt)))
            })
            .abortable();
            self.thumbnails.pending.insert(
                key,
                Pending {
                    cancellation,
                    handle,
                },
            );
            tasks.push(task);
        }
        Task::batch(tasks)
    }
}

pub(crate) fn image<'a>(
    values: Option<&Values>,
    view: ViewId,
    revision: u64,
    path: &Path,
    size: f32,
) -> Option<Element<'a, Message>> {
    image_with_opacity(values, view, revision, path, size, 1.0)
}

pub(crate) fn image_with_opacity<'a>(
    values: Option<&Values>,
    view: ViewId,
    revision: u64,
    path: &Path,
    size: f32,
    opacity: f32,
) -> Option<Element<'a, Message>> {
    let handle = values?
        .get(&(view, revision, path.into()))?
        .as_ref()?
        .as_ref()
        .ok()?;
    Some(
        iced::widget::image(handle.clone())
            .width(size)
            .height(size)
            .opacity(opacity)
            .content_fit(iced::ContentFit::Contain)
            .into(),
    )
}

pub(crate) fn error(props: file_list::Props<'_>, path: &Path) -> Option<String> {
    if !image_candidate(path) {
        return None;
    }
    match props
        .thumbnails?
        .get(&(props.view, props.revision, path.into()))
    {
        Some(Some(Ok(_))) | None => None,
        Some(Some(Err(error))) => Some(error.clone()),
        Some(None) => Some("Loading thumbnail…".into()),
    }
}

fn local_thumbnails_allowed(listing: &DirectoryListing) -> bool {
    listing.origin != ListingOrigin::Local
        || crate::platform::volumes::is_local_measurable_mount(&listing.directory)
}
