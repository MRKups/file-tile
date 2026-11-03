//! One synchronous native copy. No custom recursion, overwrite, or source cleanup.
use objc2_foundation::NSFileManager;
use std::path::Path;

pub fn copy(source: &Path, destination: &Path) -> Result<(), String> {
    objc2::rc::autoreleasepool(|_| copy_with_pool(source, destination))
}

fn copy_with_pool(source: &Path, destination: &Path) -> Result<(), String> {
    let source = super::file_urls::encode_url(
        source,
        "Cannot encode the copy source URL. Use Finder for this entry.",
    )?;
    let destination = super::file_urls::encode_url(
        destination,
        "Cannot encode the copy destination URL. Use Finder for this entry.",
    )?;
    // A fresh manager has no delegate that can skip items and still report success.
    NSFileManager::new().copyItemAtURL_toURL_error(&source, &destination)
        .map_err(|error| format!("Native copy failed: {}. Inspect the destination before retrying; partial output may remain.", error.localizedDescription()))
}
