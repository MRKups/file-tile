//! Bounded image-read data retained for thumbnail consumers, not file activation.
use crate::domain::ViewId;
use std::path::{Path, PathBuf};
use std::sync::Arc;
pub const IMAGE_LIMIT: u64 = 20 * 1024 * 1024;
pub const PIXEL_LIMIT: u64 = 16_000_000;
pub const DIMENSION_LIMIT: u32 = 16_384;

pub fn image_candidate(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["png", "jpg", "jpeg", "webp"]
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRequest {
    pub view: ViewId,
    pub generation: u64,
    pub path: PathBuf,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewData {
    Image {
        width: u32,
        height: u32,
        rgba: Arc<[u8]>,
    },
}
impl PreviewData {
    pub fn valid(&self) -> bool {
        match self {
            Self::Image {
                width,
                height,
                rgba,
            } => {
                *width > 0
                    && *height > 0
                    && *width <= DIMENSION_LIMIT
                    && *height <= DIMENSION_LIMIT
                    && u64::from(*width) * u64::from(*height) <= PIXEL_LIMIT
                    && rgba.len() as u64 == u64::from(*width) * u64::from(*height) * 4
            }
        }
    }
}
#[derive(Debug, Clone)]
pub struct PreviewCompletion {
    pub request: PreviewRequest,
    pub result: Result<PreviewData, String>,
}
