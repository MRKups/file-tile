//! Native menus present application state and emit guarded application commands.
#[cfg(all(target_os = "macos", not(test)))]
mod application_menu;
#[cfg(any(target_os = "macos", test))]
mod menu_signal;
#[cfg(any(target_os = "macos", test))]
mod quit_signal;

#[cfg(all(target_os = "macos", not(test)))]
use application_menu::ApplicationMenu;
#[cfg(any(target_os = "macos", test))]
use menu_signal::MenuSignal;
#[cfg(any(target_os = "macos", test))]
use quit_signal::QuitSignal;

use crate::application::Action;
use crate::application::contracts::menu::{MenuCommand, MenuRequest, MenuState};
#[cfg(any(target_os = "macos", test))]
use crate::application::contracts::storage::PersistenceState;
use crate::{Desktop, Message, widgets::focus_control, window::appearance as a};
use iced::widget::{button, column, container};
use iced::{Element, Fill, Task};
#[cfg(any(target_os = "macos", test))]
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct Runtime {
    #[cfg(all(target_os = "macos", not(test)))]
    menu: Option<ApplicationMenu>,
    #[cfg(any(target_os = "macos", test))]
    signal: Option<Arc<QuitSignal>>,
    #[cfg(any(target_os = "macos", test))]
    commands: Option<Arc<MenuSignal>>,
    listener: Option<iced::task::Handle>,
    command_listener: Option<iced::task::Handle>,
    published: Option<MenuState>,
    refresh: bool,
    error: Option<String>,
}
impl Drop for Runtime {
    fn drop(&mut self) {
        #[cfg(any(target_os = "macos", test))]
        if let Some(signal) = &self.signal {
            signal.stop();
        }
        #[cfg(any(target_os = "macos", test))]
        if let Some(signal) = &self.commands {
            signal.stop();
        }
        if let Some(handle) = self.listener.take() {
            handle.abort();
        }
        if let Some(handle) = self.command_listener.take() {
            handle.abort();
        }
    }
}
impl Runtime {
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

impl Desktop {
    pub(crate) fn install_quit_menu(&mut self) -> Task<Message> {
        // Unit/widget fixtures run on worker threads and never mutate global AppKit.
        #[cfg(all(target_os = "macos", not(test)))]
        if self.native_menu.menu.is_none() {
            crate::platform::menu::set_application_icon(crate::window::window_shell::APP_ICON_ICNS);
            let menu = ApplicationMenu::install();
            self.native_menu.error = menu.error().map(str::to_owned);
            if let Some(error) = &self.native_menu.error {
                eprintln!("{error}");
            }
            self.native_menu.signal = Some(menu.signal());
            self.native_menu.commands = Some(menu.commands());
            self.native_menu.menu = Some(menu);
            self.native_menu.published = None;
            self.native_menu.refresh = true;
            self.install_updater();
        }
        self.sync_quit_signal();
        Task::batch([self.listen_for_quit(), self.listen_for_menu()])
    }

    pub(crate) fn sync_quit_signal(&self) {
        #[cfg(any(target_os = "macos", test))]
        if let Some(signal) = &self.native_menu.signal {
            signal.set_accepting(
                self.application.accepts_edits()
                    || self.application.workspace_management_pending()
                        && !self.application.workspace_management_close_waiting()
                    || matches!(self.application.persistence(), PersistenceState::Loading(_)),
            );
        }
    }

    fn listen_for_quit(&mut self) -> Task<Message> {
        #[cfg(any(target_os = "macos", test))]
        if self.native_menu.listener.is_none()
            && let Some(signal) = &self.native_menu.signal
        {
            let (task, handle) = Task::perform(signal.next(), Message::NativeQuit).abortable();
            self.native_menu.listener = Some(handle);
            return task;
        }
        Task::none()
    }

    pub(crate) fn native_quit(&mut self, epoch: Option<u64>) -> Task<Message> {
        if let Some(handle) = self.native_menu.listener.take() {
            handle.abort();
        }
        #[cfg(any(target_os = "macos", test))]
        if let Some(epoch) = epoch
            && self
                .native_menu
                .signal
                .as_ref()
                .is_some_and(|signal| signal.accepts(epoch))
        {
            self.cancel_card_drag();
            self.tab_drag = None;
            let task = self.dispatch(Action::RequestClose);
            return Task::batch([task, self.listen_for_quit()]);
        }
        if epoch.is_some() {
            self.listen_for_quit()
        } else {
            Task::none()
        }
    }

    fn menu_state(&self) -> MenuState {
        let recovery_blocking = self.storage_details && self.application.storage_detail().is_some();
        MenuState {
            about_enabled: self.workspace_input_enabled(),
            check_updates_enabled: self.updates.can_check()
                && self.native_menu.error.is_none()
                && !recovery_blocking
                && !self.context_menu_open(),
            settings_enabled: self.application.can_configure_application()
                && !self.modal_open()
                && !self.context_menu_open(),
            modes_enabled: self.workspace_input_enabled()
                && self.application.file_operation().is_none(),
            mode: self.application.workspace().mode(),
        }
    }

    pub(crate) fn sync_application_menu(&mut self) {
        let state = self.menu_state();
        let changed = self.native_menu.published != Some(state);
        #[cfg(any(target_os = "macos", test))]
        if changed && let Some(signal) = &self.native_menu.commands {
            signal.set_state(state);
        }
        #[cfg(all(target_os = "macos", not(test)))]
        if (changed || self.native_menu.refresh)
            && let Some(menu) = &self.native_menu.menu
        {
            menu.update(state);
        }
        self.native_menu.published = Some(state);
        self.native_menu.refresh = false;
    }

    fn listen_for_menu(&mut self) -> Task<Message> {
        #[cfg(any(target_os = "macos", test))]
        if self.native_menu.command_listener.is_none()
            && let Some(signal) = &self.native_menu.commands
        {
            let (task, handle) = Task::perform(signal.next(), Message::NativeMenu).abortable();
            self.native_menu.command_listener = Some(handle);
            return task;
        }
        Task::none()
    }

    pub(crate) fn native_menu(&mut self, request: Option<MenuRequest>) -> Task<Message> {
        if let Some(handle) = self.native_menu.command_listener.take() {
            handle.abort();
        }
        // Reapply item state after handling a menu command, including a no-op selection.
        self.native_menu.refresh = true;
        let mut task = Task::none();
        #[cfg(any(target_os = "macos", test))]
        if let Some(request) = request
            && self
                .native_menu
                .commands
                .as_ref()
                .is_some_and(|signal| signal.accepts(request))
            && self.menu_state().permits(request.command)
        {
            task = match request.command {
                MenuCommand::About => self.about_message(crate::dialogs::about::Action::Open),
                MenuCommand::CheckForUpdates => {
                    self.updates_action(crate::dialogs::updates::Action::CheckNow)
                }
                MenuCommand::Settings => self.app_settings_message(Message::OpenAppSettings),
                MenuCommand::SelectMode(mode) if mode != self.application.workspace().mode() => {
                    self.dispatch(Action::SelectMode(mode))
                }
                MenuCommand::SelectMode(_) => Task::none(),
            };
        }
        if request.is_some() {
            Task::batch([task, self.listen_for_menu()])
        } else {
            task
        }
    }
}

pub(crate) fn error_view(error: &str, palette: a::Palette) -> Element<'_, Message> {
    let ty = palette.typography;
    let close = focus_control::themed(
        "native-menu-close",
        Message::Action(Action::RequestClose),
        button(ty.control("Close application"))
            .on_press(Message::Action(Action::RequestClose))
            .style(move |_, status| palette.text_button(status)),
        palette,
    );
    container(
        container(
            column![
                ty.heading("Application menus unavailable")
                    .color(palette.text),
                a::scroll_area(ty.text(error).color(palette.text))
                    .height(Fill)
                    .direction(iced::widget::scrollable::Direction::Vertical(a::scrollbar()))
                    .style(palette.scrollable()),
                close
            ]
            .spacing(14),
        )
        .padding(20)
        .max_width(460)
        .height(220)
        .style(move |_| palette.modal_panel()),
    )
    .padding(16)
    .center_x(Fill)
    .center_y(Fill)
    .style(|_| container::Style {
        background: Some(a::MODAL_SCRIM.into()),
        ..container::Style::default()
    })
    .into()
}
