//! Safe main-thread `NSOpenPanel` sheet adapter.
//!
//! Presents parented sheets on the active `NSWindow` without `unsafe` blocks,
//! synchronous modal loops, or off-main Objective-C object drops.

use std::cell::RefCell;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSApplication, NSModalResponse, NSModalResponseCancel, NSModalResponseOK, NSOpenPanel, NSWindow,
};
use objc2_foundation::NSString;

use crate::file_url::{FileUrlError, decode_nsurl, encode_nsurl};

pub type PathChoice = Pin<Box<dyn Future<Output = Result<Option<PathBuf>, String>> + Send>>;

static NEXT_SHEET_ID: AtomicU64 = AtomicU64::new(1);
static ACTIVE_SHEET_ID: AtomicU64 = AtomicU64::new(0);

struct ActiveSheet {
    id: u64,
    window: Retained<NSWindow>,
    panel: Retained<NSOpenPanel>,
}

thread_local! {
    static ACTIVE_SHEET: RefCell<Option<ActiveSheet>> = const { RefCell::new(None) };
}

#[derive(Default)]
struct SheetShared {
    result: Option<Result<Option<PathBuf>, String>>,
    waker: Option<Waker>,
}

struct SheetFuture {
    id: u64,
    cancelled: Arc<AtomicBool>,
    shared: Arc<Mutex<SheetShared>>,
}

impl Future for SheetFuture {
    type Output = Result<Option<PathBuf>, String>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.cancelled.load(Ordering::Acquire) {
            return Poll::Ready(Ok(None));
        }
        let mut guard = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(result) = guard.result.take() {
            Poll::Ready(result)
        } else {
            guard.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

impl Drop for SheetFuture {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if ACTIVE_SHEET_ID.load(Ordering::Acquire) == self.id
            && let Some(mtm) = MainThreadMarker::new()
        {
            dismiss_sheet_on_main(mtm, self.id);
        }
    }
}

/// Presents a folder-only `NSOpenPanel` sheet parented to the active application window.
pub fn choose_folder(directory: PathBuf, cancelled: Arc<AtomicBool>) -> PathChoice {
    open_sheet(directory, true, cancelled)
}

/// Presents a file, folder, or `.app` bundle `NSOpenPanel` sheet parented to the active window.
pub fn choose_target(directory: PathBuf, cancelled: Arc<AtomicBool>) -> PathChoice {
    open_sheet(directory, false, cancelled)
}

fn has_active_sheet() -> bool {
    ACTIVE_SHEET_ID.load(Ordering::Acquire) != 0
}

/// Dismisses any active `NSOpenPanel` sheet when called on the main thread.
///
/// Returns `true` when no sheet remains active, or `false` if a sheet is active
/// and main-thread dispatch is required to end it.
pub fn dismiss_active_sheet() -> bool {
    if !has_active_sheet() {
        return true;
    }
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    dismiss_all_on_main(mtm);
    true
}

fn open_sheet(directory: PathBuf, folder_only: bool, cancelled: Arc<AtomicBool>) -> PathChoice {
    if cancelled.load(Ordering::Acquire) {
        return Box::pin(async { Ok(None) });
    }
    let Some(mtm) = MainThreadMarker::new() else {
        return Box::pin(async {
            Err("Cannot open the chooser off the main window thread. Close configuration and try again.".into())
        });
    };
    dismiss_all_on_main(mtm);
    let app = NSApplication::sharedApplication(mtm);
    let Some(window) = resolve_parent_window(&app) else {
        return Box::pin(async {
            Err("Cannot open the chooser: the parent window is unavailable. Close configuration and try again.".into())
        });
    };

    let panel = configure_panel(mtm, &directory, folder_only);
    let id = NEXT_SHEET_ID.fetch_add(1, Ordering::Relaxed);
    let shared = Arc::new(Mutex::new(SheetShared::default()));
    store_active_sheet(
        mtm,
        ActiveSheet {
            id,
            window: window.clone(),
            panel: panel.clone(),
        },
    );

    let block_cancelled = Arc::clone(&cancelled);
    let block_shared = Arc::clone(&shared);
    let handler = RcBlock::new(move |response: NSModalResponse| {
        complete_sheet(id, response, &block_cancelled, &block_shared);
    });
    panel.beginSheetModalForWindow_completionHandler(&window, &handler);

    Box::pin(SheetFuture {
        id,
        cancelled,
        shared,
    })
}

fn unwrap_sheet_host(mut window: Retained<NSWindow>) -> Option<Retained<NSWindow>> {
    let mut depth = 0_u8;
    while let Some(parent) = window.sheetParent() {
        window = parent;
        depth = depth.saturating_add(1);
        if depth >= 8 {
            break;
        }
    }
    (!window.isSheet() && window.canBecomeMainWindow()).then_some(window)
}

fn resolve_parent_window(app: &NSApplication) -> Option<Retained<NSWindow>> {
    app.keyWindow()
        .and_then(unwrap_sheet_host)
        .or_else(|| app.mainWindow().and_then(unwrap_sheet_host))
        .or_else(|| {
            let windows = app.windows();
            windows
                .iter()
                .filter_map(unwrap_sheet_host)
                .find(|window| window.isVisible())
                .or_else(|| windows.iter().find_map(unwrap_sheet_host))
        })
}

fn configure_panel(
    mtm: MainThreadMarker,
    directory: &Path,
    folder_only: bool,
) -> Retained<NSOpenPanel> {
    let panel = NSOpenPanel::openPanel(mtm);
    let prompt = NSString::from_str(if folder_only {
        "Choose a folder"
    } else {
        "Choose a shortcut target"
    });
    panel.setMessage(Some(&prompt));
    panel.setTitle(Some(&prompt));
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(!folder_only);
    panel.setAllowsMultipleSelection(false);
    panel.setCanCreateDirectories(false);
    panel.setTreatsFilePackagesAsDirectories(false);
    if let Ok(url) = encode_nsurl(directory) {
        panel.setDirectoryURL(Some(&url));
    }
    panel
}

fn complete_sheet(
    id: u64,
    response: NSModalResponse,
    cancelled: &AtomicBool,
    shared: &Mutex<SheetShared>,
) {
    let active = MainThreadMarker::new().and_then(|mtm| take_active_sheet(mtm, id));
    let outcome = if cancelled.load(Ordering::Acquire) || response == NSModalResponseCancel {
        Ok(None)
    } else if response == NSModalResponseOK {
        match active.and_then(|sheet| sheet.panel.URL()) {
            Some(url) => decode_nsurl(&url, true).map(Some).map_err(describe_url_error),
            None => Err(
                "Cannot read the selected path from macOS. Choose the item again or enter an absolute path."
                    .into(),
            ),
        }
    } else {
        Err(
            "Cannot complete native selection: macOS ended the chooser unexpectedly. Try again or enter an absolute path."
                .into(),
        )
    };
    let mut guard = shared.lock().unwrap_or_else(PoisonError::into_inner);
    guard.result = Some(outcome);
    if let Some(waker) = guard.waker.take() {
        waker.wake();
    }
}

fn describe_url_error(error: FileUrlError) -> String {
    match error {
        FileUrlError::NonFileUrl | FileUrlError::NonLocalHost => {
            "Cannot use the selected item: only local files and folders are supported. Choose a local item or enter an absolute path.".into()
        }
        FileUrlError::UnresolvableReference
        | FileUrlError::MalformedUrl
        | FileUrlError::InvalidPercentEncoding => {
            "Cannot read the selected path from macOS. Choose the item again or enter an absolute path.".into()
        }
        FileUrlError::NonUtf8 | FileUrlError::InvalidDecodedPath => {
            "Cannot use the selected path: its name cannot be represented as valid UTF-8 text. Rename the item in Finder or choose another path.".into()
        }
        FileUrlError::TooLongOrInvalid => {
            "Cannot use the selected path: the path exceeds 4,096 bytes or contains unsupported characters. Choose a shorter path.".into()
        }
    }
}

fn store_active_sheet(_mtm: MainThreadMarker, sheet: ActiveSheet) {
    ACTIVE_SHEET_ID.store(sheet.id, Ordering::Release);
    ACTIVE_SHEET.with(|slot| {
        *slot.borrow_mut() = Some(sheet);
    });
}

fn take_active_sheet(_mtm: MainThreadMarker, id: u64) -> Option<ActiveSheet> {
    let taken = ACTIVE_SHEET.with(|slot| {
        let mut borrow = slot.borrow_mut();
        if borrow.as_ref().is_some_and(|sheet| sheet.id == id) {
            borrow.take()
        } else {
            None
        }
    });
    if taken.is_some() {
        ACTIVE_SHEET_ID
            .compare_exchange(id, 0, Ordering::AcqRel, Ordering::Acquire)
            .ok();
    }
    taken
}

fn dismiss_sheet_on_main(mtm: MainThreadMarker, id: u64) {
    if let Some(sheet) = take_active_sheet(mtm, id) {
        sheet
            .window
            .endSheet_returnCode(&sheet.panel, NSModalResponseCancel);
    }
}

fn dismiss_all_on_main(_mtm: MainThreadMarker) {
    let taken = ACTIVE_SHEET.with(|slot| slot.borrow_mut().take());
    if let Some(sheet) = taken {
        ACTIVE_SHEET_ID
            .compare_exchange(sheet.id, 0, Ordering::AcqRel, Ordering::Acquire)
            .ok();
        sheet
            .window
            .endSheet_returnCode(&sheet.panel, NSModalResponseCancel);
    }
}
