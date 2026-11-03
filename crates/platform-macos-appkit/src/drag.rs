//! Isolated AppKit outbound file drag session wrapper.
//!
//! Encapsulates the Objective-C `NSDraggingSource` protocol implementation and
//! `NSDraggingItem::setDraggingFrame_contents` call behind a safe, defensive API.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession,
    NSDraggingSource, NSEvent, NSEventType, NSImage, NSPasteboardWriting,
};
use objc2_foundation::{
    NSArray, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL,
};

const MAX_DRAG_ITEMS: usize = 128;

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);
static ACTIVE_SESSION_ID: AtomicU64 = AtomicU64::new(0);
static COMPLETED_SESSION_ID: AtomicU64 = AtomicU64::new(0);
static COMPLETED_MOVED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DragCompletion {
    pub session_id: u64,
    pub moved: bool,
}

struct DragSourceIvars {
    session_id: u64,
    allow_move: bool,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FileTileDraggingSource"]
    #[ivars = DragSourceIvars]
    struct DraggingSource;

    unsafe impl NSObjectProtocol for DraggingSource {}

    unsafe impl NSDraggingSource for DraggingSource {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn source_operation_mask(
            &self,
            _session: &NSDraggingSession,
            context: NSDraggingContext,
        ) -> NSDragOperation {
            if context == NSDraggingContext::WithinApplication {
                NSDragOperation::None
            } else if self.ivars().allow_move {
                NSDragOperation::Copy.union(NSDragOperation::Move)
            } else {
                NSDragOperation::Copy
            }
        }

        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn ended_at_point(
            &self,
            _session: &NSDraggingSession,
            _screen_point: NSPoint,
            operation: NSDragOperation,
        ) {
            let session_id = self.ivars().session_id;
            ACTIVE_SESSION_ID
                .compare_exchange(session_id, 0, Ordering::SeqCst, Ordering::SeqCst)
                .ok();
            COMPLETED_MOVED.store(operation.contains(NSDragOperation::Move), Ordering::SeqCst);
            COMPLETED_SESSION_ID.store(session_id, Ordering::SeqCst);
        }
    }
);

impl DraggingSource {
    fn new(mtm: MainThreadMarker, session_id: u64, allow_move: bool) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DragSourceIvars {
            session_id,
            allow_move,
        });
        // SAFETY: `NSObject::init` is safe on a freshly allocated `DraggingSource`
        // whose `DragSourceIvars` have been initialized above.
        unsafe { msg_send![super(this), init] }
    }
}

/// Clears any stale in-flight session marker if AppKit ended a drag without
/// delivering `endedAtPoint:operation:`.
pub fn reset_stale_session() {
    ACTIVE_SESSION_ID.store(0, Ordering::SeqCst);
}

/// Confirms that the main window thread is handling an active primary mouse
/// press or drag with the button still held down before handing window movement
/// to AppKit.
#[forbid(unsafe_code)]
pub fn can_start_window_drag() -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    if (NSEvent::pressedMouseButtons() & 1) == 0 {
        return false;
    }
    let app = NSApplication::sharedApplication(mtm);
    app.currentEvent().is_some_and(|event| {
        matches!(
            event.r#type(),
            NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged
        )
    })
}

/// Returns the most recent completed outbound drag receipt, if any.
pub fn take_completed_drag() -> Option<DragCompletion> {
    let session_id = COMPLETED_SESSION_ID.swap(0, Ordering::SeqCst);
    if session_id == 0 {
        None
    } else {
        Some(DragCompletion {
            session_id,
            moved: COMPLETED_MOVED.load(Ordering::SeqCst),
        })
    }
}

/// Starts a native macOS `NSDraggingSession` for `paths` from the active window.
///
/// Validates main-thread execution, single-session state, path safety, active
/// window/view availability, and that the current AppKit event is a left-mouse
/// press or drag before initiating the session.
pub fn start_outbound_drag(paths: &[PathBuf], allow_move: bool) -> Result<u64, String> {
    if paths.is_empty() || paths.len() > MAX_DRAG_ITEMS {
        return Err(format!(
            "Select 1–{MAX_DRAG_ITEMS} items to drag outside the window."
        ));
    }
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "Outbound drag requires the main window thread.".to_string())?;
    if ACTIVE_SESSION_ID.load(Ordering::SeqCst) != 0 {
        return Err("Another drag session is already active.".into());
    }
    let app = NSApplication::sharedApplication(mtm);
    let window = app
        .keyWindow()
        .or_else(|| app.mainWindow())
        .ok_or_else(|| "Cannot start drag: no active window.".to_string())?;
    let view = window
        .contentView()
        .ok_or_else(|| "Cannot start drag: window has no content view.".to_string())?;
    let event = window
        .currentEvent()
        .or_else(|| app.currentEvent())
        .ok_or_else(|| "Cannot start drag: no current mouse event.".to_string())?;
    let event_type = event.r#type();
    if event_type != NSEventType::LeftMouseDragged && event_type != NSEventType::LeftMouseDown {
        return Err("Cannot start drag: pointer is no longer dragging.".into());
    }
    let location = view.convertPoint_fromView(event.locationInWindow(), None);
    let preview = drag_preview_image(paths.len() > 1);
    let mut items = Vec::with_capacity(paths.len());
    for (index, path) in paths.iter().enumerate() {
        let url = validated_file_url(path)?;
        let writer = ProtocolObject::<dyn NSPasteboardWriting>::from_retained(url);
        let item = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), &writer);
        let offset = (index.min(4) as f64) * 3.0;
        let frame = NSRect::new(
            NSPoint::new(location.x - 16.0 + offset, location.y - 16.0 - offset),
            NSSize::new(32.0, 32.0),
        );
        let contents: &AnyObject = preview.as_ref();
        // SAFETY: `preview` is a valid, retained `NSImage` instance and `frame`
        // has finite, positive dimensions.
        unsafe {
            item.setDraggingFrame_contents(frame, Some(contents));
        }
        items.push(item);
    }
    let session_id = NEXT_SESSION_ID.fetch_add(1, Ordering::SeqCst);
    ACTIVE_SESSION_ID.store(session_id, Ordering::SeqCst);
    let source = DraggingSource::new(mtm, session_id, allow_move);
    let source_proto = ProtocolObject::<dyn NSDraggingSource>::from_ref(&*source);
    let array = NSArray::from_retained_slice(&items);
    let session = view.beginDraggingSessionWithItems_event_source(&array, &event, source_proto);
    session.setAnimatesToStartingPositionsOnCancelOrFail(true);
    Ok(session_id)
}

fn validated_file_url(path: &Path) -> Result<Retained<NSURL>, String> {
    let bytes = path.as_os_str().as_encoded_bytes();
    if !path.is_absolute() || bytes.is_empty() || bytes.len() > 4096 || bytes.contains(&0) {
        return Err("Cannot drag an invalid or non-absolute file path.".into());
    }
    let text = path
        .to_str()
        .ok_or_else(|| "Cannot drag a non-UTF-8 file path.".to_string())?;
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| "Cannot drag a file that is no longer available.".to_string())?;
    let ns_path = NSString::from_str(text);
    Ok(NSURL::fileURLWithPath_isDirectory(
        &ns_path,
        metadata.is_dir(),
    ))
}

fn drag_preview_image(multiple: bool) -> Retained<NSImage> {
    let symbol = NSString::from_str(if multiple { "doc.on.doc" } else { "doc" });
    if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(&symbol, None)
    {
        image.setSize(NSSize::new(32.0, 32.0));
        return image;
    }
    NSImage::initWithSize(NSImage::alloc(), NSSize::new(32.0, 32.0))
}
