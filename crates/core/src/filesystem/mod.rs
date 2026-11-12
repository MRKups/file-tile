//! Directory adapters. Native reads use `std::fs`; no engine stack is imported.

mod locations;
pub use locations::LocationWorkers;
mod metadata;
mod preview;
mod webp_header;
pub use preview::PreviewWorkers;
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
