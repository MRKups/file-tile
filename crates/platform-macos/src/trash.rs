//! Native macOS Trash placement through Foundation's `NSFileManager`.
use objc2_foundation::NSFileManager;
use std::path::Path;

pub fn trash(target: &Path) -> Result<(), String> {
    let target = super::file_urls::encode_url(
        target,
        "Cannot create a native file URL for this path. Use Finder for this entry.",
    )?;
    NSFileManager::new()
        .trashItemAtURL_resultingItemURL_error(&target, None)
        .map_err(|error| error.localizedDescription().to_string())
}
