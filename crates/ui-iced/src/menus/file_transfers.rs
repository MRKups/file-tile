//! Clipboard command intent and admission. Effects and operation state stay elsewhere.
use crate::{Desktop, Message};
use crate::{
    application::{
        Action,
        files::file_operations::{self as ops, Item},
    },
    domain::ViewId,
};
use iced::Task;

impl Desktop {
    pub(crate) fn copy_selected_files(&mut self, view: ViewId) -> Task<Message> {
        self.transfer_selected(view, false)
    }

    pub(crate) fn cut_selected_files(&mut self, view: ViewId) -> Task<Message> {
        self.transfer_selected(view, true)
    }

    fn transfer_selected(&mut self, view: ViewId, cut: bool) -> Task<Message> {
        if !self.file_view_input_enabled(view) {
            return Task::none();
        }
        if let Some((_, selected)) = self.column_previews.selected_entry(view) {
            return self.transfer_paths(view, vec![selected], cut);
        }
        let Some(state) = self.views.get(&view) else {
            return Task::none();
        };
        let paths: Vec<_> = self
            .application
            .ordered_entries(view)
            .filter(|entry| {
                state.selection.contains(&entry.path)
                    || state.selected.as_ref() == Some(&entry.path)
            })
            .map(|entry| entry.path.clone())
            .collect();
        self.transfer_paths(view, paths, cut)
    }

    pub(crate) fn transfer_paths(
        &mut self,
        view: ViewId,
        paths: Vec<std::path::PathBuf>,
        cut: bool,
    ) -> Task<Message> {
        if !self.file_view_input_enabled(view) {
            return Task::none();
        }
        if cut && self.application.workspace().mode() != crate::domain::Mode::Browse {
            self.views.entry(view).or_default().hint =
                Some("Switch to Browse to cut files.".into());
            return Task::none();
        }
        let action = if cut { "Cut" } else { "Copy" };
        if paths.is_empty() || paths.len() > ops::ITEM_LIMIT {
            self.views.entry(view).or_default().hint =
                Some(format!("Select 1–{} entries to {action}.", ops::ITEM_LIMIT));
            return Task::none();
        }
        if cut
            && let Some(error) = paths
                .iter()
                .find_map(|path| self.application.move_guidance(path).err())
        {
            self.views.entry(view).or_default().hint = Some(error.into());
            return Task::none();
        }
        // Window-thread transport preserves command ordering; it performs no move.
        let result = if cut {
            crate::platform::clipboard::cut_files(&paths)
        } else {
            crate::platform::clipboard::copy_files(&paths)
        };
        self.views.entry(view).or_default().hint = result.err();
        Task::none()
    }

    pub(crate) fn paste_into(&mut self, view: ViewId) -> Task<Message> {
        let Some(directory) = self
            .application
            .workspace()
            .view(view)
            .map(|view| view.navigation.current().to_path_buf())
        else {
            return Task::none();
        };
        self.paste_into_directory(view, directory)
    }

    pub(crate) fn move_into(&mut self, view: ViewId) -> Task<Message> {
        let Some(directory) = self
            .application
            .workspace()
            .view(view)
            .map(|view| view.navigation.current().to_path_buf())
        else {
            return Task::none();
        };
        self.move_into_directory(view, directory)
    }

    pub(crate) fn paste_into_directory(
        &mut self,
        view: ViewId,
        directory: std::path::PathBuf,
    ) -> Task<Message> {
        self.paste_transfer(view, directory, false)
    }

    pub(crate) fn move_into_directory(
        &mut self,
        view: ViewId,
        directory: std::path::PathBuf,
    ) -> Task<Message> {
        self.paste_transfer(view, directory, true)
    }

    fn paste_transfer(
        &mut self,
        view: ViewId,
        directory: std::path::PathBuf,
        force_move: bool,
    ) -> Task<Message> {
        if !self.file_view_input_enabled(view) {
            return Task::none();
        }
        if !self.application.can_write_folder(view, &directory) {
            self.views.entry(view).or_default().hint =
                Some(self.file_write_guidance_for(view, &directory).into());
            return Task::none();
        }
        let files = match crate::platform::clipboard::paste_files() {
            Ok(files) => files,
            Err(error) => {
                self.views.entry(view).or_default().hint = Some(error);
                return Task::none();
            }
        };
        let moving = force_move || files.cut;
        let task = self.transfer_paths_into_directory(view, directory, files.paths, moving);
        if moving && self.application.files_busy() {
            // Do not leave destructive references to old names on the clipboard.
            // Failed or skipped moves need a fresh explicit Copy/Cut to retry.
            crate::platform::clipboard::consume_move(files.change_count);
        }
        task
    }

    pub(crate) fn transfer_paths_into(
        &mut self,
        view: ViewId,
        paths: Vec<std::path::PathBuf>,
        moving: bool,
    ) -> Task<Message> {
        let Some(directory) = self
            .application
            .workspace()
            .view(view)
            .map(|view| view.navigation.current().to_path_buf())
        else {
            return Task::none();
        };
        self.transfer_paths_into_directory(view, directory, paths, moving)
    }

    pub(crate) fn transfer_paths_into_directory(
        &mut self,
        view: ViewId,
        directory: std::path::PathBuf,
        paths: Vec<std::path::PathBuf>,
        moving: bool,
    ) -> Task<Message> {
        if !self.file_view_input_enabled(view) {
            return Task::none();
        }
        if !self.application.can_write_folder(view, &directory) {
            self.views.entry(view).or_default().hint =
                Some(self.file_write_guidance_for(view, &directory).into());
            return Task::none();
        }
        if paths.is_empty() || paths.len() > ops::ITEM_LIMIT {
            let action = if moving { "move" } else { "copy" };
            self.views.entry(view).or_default().hint =
                Some(format!("Select 1–{} entries to {action}.", ops::ITEM_LIMIT));
            return Task::none();
        }
        if moving {
            if paths
                .iter()
                .all(|path| path.parent() == Some(directory.as_path()))
            {
                return Task::none();
            }
            if let Some(error) = paths
                .iter()
                .find_map(|path| self.application.move_guidance(path).err())
            {
                self.views.entry(view).or_default().hint = Some(error.into());
                return Task::none();
            }
        }
        let items = paths
            .into_iter()
            .map(|path| {
                let name = crate::domain::files::source_name(&path)?;
                Ok((
                    if moving {
                        Item::Move(path)
                    } else {
                        Item::Copy(path)
                    },
                    name,
                ))
            })
            .collect::<Result<Vec<_>, String>>();
        match items {
            Ok(items) => self.dispatch(Action::StartFileOperation {
                view,
                directory,
                items,
            }),
            Err(error) => {
                self.views.entry(view).or_default().hint = Some(error);
                Task::none()
            }
        }
    }
}
