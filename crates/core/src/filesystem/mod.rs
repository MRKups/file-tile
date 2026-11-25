//! Directory adapters. Native reads use `std::fs`; no engine stack is imported.

mod locations;
pub use locations::LocationWorkers;
mod metadata;
mod preview;
mod webp_header;
pub use preview::PreviewWorkers;
mod search;
pub use search::SearchWorkers;
mod opening;
pub use opening::OpenWorkers;
mod native;
mod work;
pub use metadata::MetadataWorkers;
pub(crate) mod tests;
mod watch;
mod worker;

pub use native::NativeDirectory;
pub use tests::ControlledDirectory;
pub use watch::DirectoryWatcher;
pub use worker::DirectoryWorkers;
pub mod file_operations;
pub mod folder_usage;
pub mod sorting;

pub(crate) fn is_dataless(metadata: &std::fs::Metadata) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        (metadata.st_flags() & 0x4000_0000) != 0
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = metadata;
        false
    }
}
