//! Explicit default-app handoffs, independent of image decoding and presentation.
use crate::application::contracts::directory::EntryKind;
use crate::application::{Application, Effect, LoadState};
use crate::domain::ViewId;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRequest {
    pub view: ViewId,
    pub generation: u64,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct OpenCompletion {
    pub request: OpenRequest,
    pub result: Result<(), String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opening {
    Pending(u64),
    HandedOff,
    Failed(String),
}

#[derive(Debug)]
pub struct ExternalOpening {
    pub request: OpenRequest,
    pub phase: Opening,
    directory: PathBuf,
}

#[derive(Debug, Default)]
pub(in crate::application) struct OpeningBook {
    pub current: Option<ExternalOpening>,
    generation: u64,
}

impl Application {
    pub fn external_opening(&self) -> Option<&ExternalOpening> {
        self.opening.current.as_ref()
    }

    pub fn external_open_status(&self, view: ViewId) -> Option<&str> {
        let opening = self
            .external_opening()
            .filter(|open| open.request.view == view)?;
        match &opening.phase {
            Opening::Pending(_) => Some("Requesting default-app opening…"),
            Opening::Failed(error) => Some(error),
            Opening::HandedOff => None,
        }
    }

    pub(in crate::application) fn open_file(&mut self, view: ViewId, path: PathBuf) -> Vec<Effect> {
        if !self.accepts_workspace_input()
            || !self.workspace.can_browse(view)
            || crate::application::contracts::configuration::validate_saved_path(&path).is_err()
        {
            return vec![];
        }
        let Some(listing) = self
            .loads
            .get(&view)
            .and_then(LoadState::retained_listing)
            .filter(|listing| listing.entry(&path).is_some())
            .or_else(|| {
                self.valid_column_preview(view)
                    .filter(|listing| listing.entry(&path).is_some())
                    .cloned()
            })
        else {
            return vec![];
        };
        let show_hidden = self
            .workspace
            .view(view)
            .is_some_and(|item| item.settings.show_hidden);
        let Some(entry) = listing
            .entry(&path)
            .filter(|entry| (show_hidden || !entry.hidden) && entry.kind != EntryKind::Directory)
        else {
            return vec![];
        };
        if self.external_opening().is_some_and(|open| {
            open.request.view == view
                && open.request.path == path
                && matches!(open.phase, Opening::Pending(_))
        }) {
            return vec![];
        }
        let Some(generation) = self.opening.generation.checked_add(1) else {
            return vec![];
        };
        self.opening.generation = generation;
        let request = OpenRequest {
            view,
            generation,
            path,
        };
        let regular = entry.kind == EntryKind::File;
        self.opening.current = Some(ExternalOpening {
            request: request.clone(),
            phase: if regular {
                Opening::Pending(generation)
            } else {
                Opening::Failed(
                    "Cannot open this entry: only regular files are supported. Choose a file."
                        .into(),
                )
            },
            directory: listing.directory.clone(),
        });
        if regular {
            vec![Effect::OpenExternal(request)]
        } else {
            vec![]
        }
    }

    pub(in crate::application) fn external_opened(&mut self, completion: OpenCompletion) {
        if let Some(opening) = &mut self.opening.current
            && opening.request == completion.request
            && opening.phase == Opening::Pending(completion.request.generation)
        {
            opening.phase = match completion.result {
                Ok(()) => Opening::HandedOff,
                Err(error) => Opening::Failed(error),
            };
        }
    }

    pub(in crate::application) fn sync_opening(&mut self) {
        if self.external_opening().is_some_and(|opening| {
            !self.accepts_edits()
                || !self.workspace.can_browse(opening.request.view)
                || !self.has_active_directory(opening.request.view, &opening.directory)
        }) {
            self.opening.current = None;
        }
    }
}
