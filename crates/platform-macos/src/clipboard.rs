//! Native file-reference clipboard transport. No file reads or transfer execution.
use file_tile_platform_macos_appkit::file_url::{FileUrlError, decode_nsurl};
use objc2::{MainThreadMarker, runtime::ProtocolObject};
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
use objc2_foundation::{NSArray, NSString, NSURL, NSUTF8StringEncoding};
use std::path::PathBuf;

use super::file_urls::MAX_PATH_BYTES;

const FILE_LIMIT: usize = 128;
const CUT_TYPE: &str = "com.michalkups.file-tile.cut-files";

pub struct Files {
    pub paths: Vec<PathBuf>,
    pub cut: bool,
    pub change_count: isize,
}

pub fn copy_files(paths: &[PathBuf]) -> Result<(), String> {
    let _main = MainThreadMarker::new().ok_or("File Copy requires the main thread. Retry Copy.")?;
    write_to(&NSPasteboard::generalPasteboard(), paths, false)
}

pub fn cut_files(paths: &[PathBuf]) -> Result<(), String> {
    let _main = MainThreadMarker::new().ok_or("File Cut requires the main thread. Retry Cut.")?;
    write_to(&NSPasteboard::generalPasteboard(), paths, true)
}

/// Consume admitted move references only if no other app has replaced them.
pub fn consume_move(change_count: isize) {
    if MainThreadMarker::new().is_some() {
        let board = NSPasteboard::generalPasteboard();
        if board.changeCount() == change_count {
            board.clearContents();
        }
    }
}

pub fn copy_text(value: &str) -> Result<(), String> {
    let _main =
        MainThreadMarker::new().ok_or("Copy path requires the main thread. Retry Copy path.")?;
    let value = NSString::from_str(value);
    let kind = NSString::from_str("public.utf8-plain-text");
    let board = NSPasteboard::generalPasteboard();
    board.clearContents();
    if board.setString_forType(&value, &kind) {
        Ok(())
    } else {
        Err("Cannot copy path to the clipboard. Retry Copy path.".into())
    }
}

fn write_to(board: &NSPasteboard, paths: &[PathBuf], cut: bool) -> Result<(), String> {
    if paths.is_empty() || paths.len() > FILE_LIMIT {
        return Err(format!(
            "Cannot copy file references: select 1–{FILE_LIMIT} entries."
        ));
    }
    let kind = NSString::from_str("public.file-url");
    let mut items = Vec::with_capacity(paths.len());
    for path in paths {
        let value = super::file_urls::encode(path)?;
        let item = NSPasteboardItem::new();
        if !item.setString_forType(&value, &kind) {
            return Err("Cannot prepare file references. Retry Copy.".into());
        }
        if cut && !item.setString_forType(&NSString::from_str("1"), &NSString::from_str(CUT_TYPE)) {
            return Err("Cannot prepare Cut references. Retry Cut.".into());
        }
        items.push(item);
    }
    let objects: Vec<&ProtocolObject<dyn NSPasteboardWriting>> = items
        .iter()
        .map(|item| ProtocolObject::from_ref(&**item))
        .collect();
    let objects = NSArray::from_slice(&objects);
    board.clearContents();
    if board.writeObjects(&objects) {
        Ok(())
    } else {
        Err("Cannot write file references to the clipboard. Retry Copy.".into())
    }
}

pub fn paste_files() -> Result<Files, String> {
    let _main =
        MainThreadMarker::new().ok_or("File Paste requires the main thread. Retry Paste.")?;
    read_from(&NSPasteboard::generalPasteboard())
}

fn read_from(board: &NSPasteboard) -> Result<Files, String> {
    let before = board.changeCount();
    let items = board
        .pasteboardItems()
        .ok_or("Clipboard file references are unavailable. Copy files and check macOS clipboard access, then retry Paste.")?;
    if items.is_empty() || items.len() > FILE_LIMIT {
        return Err(format!(
            "Cannot paste these references: copy 1–{FILE_LIMIT} files first."
        ));
    }
    let kind = NSString::from_str("public.file-url");
    let cut_kind = NSString::from_str(CUT_TYPE);
    let cut = items.iter().all(|item| {
        item.stringForType(&cut_kind)
            .is_some_and(|value| value.to_string() == "1")
    });
    let mut paths = Vec::with_capacity(items.len());
    for item in items.iter() {
        let value = item
            .stringForType(&kind)
            .ok_or("Clipboard does not provide readable file references. Copy files in file-tile or Finder, allow clipboard access if asked, and retry Paste.")?;
        if !value.canBeConvertedToEncoding(NSUTF8StringEncoding) {
            return Err("Cannot decode this native file URL without losing characters. Use Finder for this entry.".into());
        }
        if value.len_utf16() > MAX_PATH_BYTES * 3 + 8 {
            return Err("Clipboard file URL exceeds the supported path limit. Use Finder.".into());
        }
        let url = NSURL::URLWithString(&value)
            .ok_or("Clipboard contains a non-file URL. Copy local files first.")?;
        let path = decode_nsurl(&url, false).map_err(describe_clipboard_url_error)?;
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    if board.changeCount() != before {
        return Err("Clipboard changed while reading file references. Retry Paste.".into());
    }
    Ok(Files {
        paths,
        cut,
        change_count: before,
    })
}

fn describe_clipboard_url_error(error: FileUrlError) -> String {
    match error {
        FileUrlError::NonFileUrl => {
            "Clipboard contains a non-file URL. Copy local files first.".into()
        }
        FileUrlError::NonLocalHost => {
            "Clipboard contains a nonlocal file URL. Copy local files first.".into()
        }
        FileUrlError::UnresolvableReference => {
            "Cannot locate the copied file. Copy it again in Finder and retry Paste.".into()
        }
        FileUrlError::MalformedUrl => {
            "Cannot parse a file reference. Copy it again in Finder.".into()
        }
        FileUrlError::InvalidPercentEncoding => "Invalid encoded file path. Copy it again.".into(),
        FileUrlError::NonUtf8 => {
            "Cannot decode this native filename exactly. Use Finder for this entry.".into()
        }
        FileUrlError::TooLongOrInvalid => {
            "Clipboard contains an unsupported file path. Copy the files again in Finder.".into()
        }
        FileUrlError::InvalidDecodedPath => {
            "Clipboard contains an invalid file path. Copy the files again.".into()
        }
    }
}
