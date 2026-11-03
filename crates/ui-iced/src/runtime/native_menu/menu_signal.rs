//! One latest native command, invalidated when its presentation context changes.
use iced::futures::task::AtomicWaker;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::{Context, Poll};

use crate::application::contracts::menu::{MenuCommand, MenuRequest, MenuState};
use crate::domain::Mode;

#[derive(Default)]
pub struct MenuSignal {
    // Low six bits are permissions; remaining bits identify their context.
    state: AtomicU64,
    pending: AtomicU64,
    stopped: AtomicBool,
    waker: AtomicWaker,
}

const PERMISSION_MASK: u64 = 63;
const EPOCH_SHIFT: u32 = 6;

fn code(command: MenuCommand) -> u64 {
    match command {
        MenuCommand::Settings => 1,
        MenuCommand::SelectMode(Mode::Browse) => 2,
        MenuCommand::SelectMode(Mode::Configure) => 3,
        MenuCommand::SelectMode(Mode::Layout) => 4,
        MenuCommand::About => 5,
        MenuCommand::CheckForUpdates => 6,
    }
}

fn command(code: u64) -> Option<MenuCommand> {
    match code {
        1 => Some(MenuCommand::Settings),
        2 => Some(MenuCommand::SelectMode(Mode::Browse)),
        3 => Some(MenuCommand::SelectMode(Mode::Configure)),
        4 => Some(MenuCommand::SelectMode(Mode::Layout)),
        5 => Some(MenuCommand::About),
        6 => Some(MenuCommand::CheckForUpdates),
        _ => None,
    }
}

impl MenuSignal {
    #[cfg(all(target_os = "macos", not(test)))]
    pub(super) fn request(&self, command: MenuCommand) {
        let state = self.state.load(Ordering::Acquire);
        let code = code(command);
        if self.stopped.load(Ordering::Acquire) || state & (1 << (code - 1)) == 0 {
            return;
        }
        self.pending
            .store((state & !PERMISSION_MASK) | code, Ordering::Release);
        self.waker.wake();
    }

    pub fn set_state(&self, state: MenuState) {
        let epoch = self.state.load(Ordering::Acquire) >> EPOCH_SHIFT;
        let Some(next) = epoch
            .checked_add(1)
            .and_then(|epoch| epoch.checked_mul(1 << EPOCH_SHIFT))
        else {
            self.stop();
            return;
        };
        let permissions = u64::from(state.settings_enabled)
            | if state.modes_enabled { 14 } else { 0 }
            | if state.about_enabled { 16 } else { 0 }
            | if state.check_updates_enabled { 32 } else { 0 };
        self.state.store(next | permissions, Ordering::Release);
        self.waker.wake();
    }

    pub fn accepts(&self, request: MenuRequest) -> bool {
        let state = self.state.load(Ordering::Acquire);
        !self.stopped.load(Ordering::Acquire)
            && state >> EPOCH_SHIFT == request.epoch
            && state & (1 << (code(request.command) - 1)) != 0
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.waker.wake();
    }

    pub fn next(self: &Arc<Self>) -> MenuFuture {
        MenuFuture(Arc::clone(self))
    }
}

pub struct MenuFuture(Arc<MenuSignal>);

impl Future for MenuFuture {
    type Output = Option<MenuRequest>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.waker.register(cx.waker());
        if self.0.stopped.load(Ordering::Acquire) {
            return Poll::Ready(None);
        }
        let pending = self.0.pending.swap(0, Ordering::AcqRel);
        if let Some(command) = command(pending & PERMISSION_MASK) {
            let request = MenuRequest {
                command,
                epoch: pending >> EPOCH_SHIFT,
            };
            if self.0.accepts(request) {
                return Poll::Ready(Some(request));
            }
        }
        Poll::Pending
    }
}
