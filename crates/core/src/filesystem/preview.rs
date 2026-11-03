//! Bounded image-thumbnail reads. File opening has a separate worker.
use super::work::{WorkFuture, Workers};
use crate::application::contracts::directory::Cancellation;
use crate::application::contracts::preview::{
    DIMENSION_LIMIT, IMAGE_LIMIT, PIXEL_LIMIT, PreviewCompletion, PreviewData, PreviewRequest,
    image_candidate,
};
use image::ImageDecoder;
use std::future::Future;
use std::io::{Cursor, Read};
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
pub struct PreviewWorkers {
    workers: Workers<Result<PreviewData, String>>,
}
impl PreviewWorkers {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            workers: Workers::new("preview", 2, 8)?,
        })
    }
    pub fn read(&self, request: PreviewRequest) -> PreviewFuture {
        let path = request.path.clone();
        let future = self
            .workers
            .submit(move |cancellation| read(&path, cancellation));
        PreviewFuture { request, future }
    }
}
pub struct PreviewFuture {
    request: PreviewRequest,
    future: WorkFuture<Result<PreviewData, String>>,
}
impl PreviewFuture {
    pub fn cancellation(&self) -> Cancellation {
        self.future.cancellation()
    }
}
impl Future for PreviewFuture {
    type Output = PreviewCompletion;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        Pin::new(&mut this.future)
            .poll(cx)
            .map(|result| PreviewCompletion {
                request: this.request.clone(),
                result: result
                    .map_err(|error| {
                        format!(
                            "Cannot load thumbnail: file worker {error:?}. Reload the folder to retry."
                        )
                    })
                    .and_then(|value| value)
                    .and_then(|data| {
                        if data.valid() {
                            Ok(data)
                        } else {
                            Err(
                                "Cannot decode image: invalid pixel data. Use explicit Open."
                                    .into(),
                            )
                        }
                    }),
            })
    }
}
pub(super) fn read(path: &Path, cancellation: &Cancellation) -> Result<PreviewData, String> {
    if !image_candidate(path) {
        return Err(
            "Cannot thumbnail this file: only PNG/JPEG/WebP images are supported. Use Open.".into(),
        );
    }
    if cancellation.is_cancelled() {
        return Err("Thumbnail cancelled.".into());
    }
    let info = std::fs::metadata(path).map_err(|error| {
        format!("Cannot read thumbnail: {error}. Reload when the file is available.")
    })?;
    if !info.is_file() {
        return Err("Cannot thumbnail this entry type. Only regular files are supported.".into());
    }
    if info.len() > IMAGE_LIMIT {
        return Err("Cannot load thumbnail: image exceeds 20 MiB. Use Open.".into());
    }
    if cancellation.is_cancelled() {
        return Err("Thumbnail cancelled.".into());
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x00000004);
    } // SDK sys/fcntl.h: O_NONBLOCK avoids a raced FIFO open.
    let mut file = options
        .open(path)
        .map_err(|error| format!("Cannot read thumbnail: {error}. Reload the folder to retry."))?;
    let before = file.metadata().map_err(|error| {
        format!("Cannot inspect thumbnail: {error}. Reload the folder to retry.")
    })?;
    if !before.is_file() {
        return Err("Cannot thumbnail this entry type. Only regular files are supported.".into());
    }
    if before.len() > IMAGE_LIMIT {
        return Err("Cannot load thumbnail: image exceeds 20 MiB. Use Open.".into());
    }
    if !unchanged(&info, &before) {
        return Err(
            "Cannot thumbnail an image replaced or changed before reading. Reload the folder to retry.".into(),
        );
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(before.len() as usize + 1)
        .map_err(|_| {
            "Cannot allocate thumbnail input. Close other apps and reload the folder.".to_owned()
        })?;
    let mut input = (&mut file).take(IMAGE_LIMIT + 1);
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        if cancellation.is_cancelled() {
            return Err("Thumbnail cancelled.".into());
        }
        let count = input.read(&mut chunk).map_err(|error| {
            format!("Cannot read thumbnail: {error}. Reload the folder to retry.")
        })?;
        if count == 0 {
            break;
        }
        bytes.try_reserve_exact(count).map_err(|_| {
            "Cannot allocate thumbnail input. Close other apps and reload the folder.".to_owned()
        })?;
        bytes.extend_from_slice(&chunk[..count]);
    }
    if cancellation.is_cancelled() {
        return Err("Thumbnail cancelled.".into());
    }
    let after = file.metadata().map_err(|error| {
        format!("Cannot inspect thumbnail: {error}. Reload the folder to retry.")
    })?;
    let named = std::fs::metadata(path).map_err(|error| {
        format!("Cannot recheck thumbnail: {error}. Reload the folder to retry.")
    })?;
    if !unchanged(&before, &after) || !unchanged(&before, &named) {
        return Err(
            "Cannot thumbnail a file changed during reading. Reload the folder to retry.".into(),
        );
    }
    if bytes.len() as u64 > IMAGE_LIMIT {
        return Err("Cannot load thumbnail: the 20 MiB limit was exceeded. Use Open.".into());
    }
    match image::guess_format(&bytes) {
        Ok(
            format
            @ (image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP),
        ) => decode(&bytes, format, cancellation),
        _ => Err("Cannot thumbnail image: its contents are not PNG/JPEG/WebP. Use Open.".into()),
    }
}
fn decode(
    bytes: &[u8],
    format: image::ImageFormat,
    cancellation: &Cancellation,
) -> Result<PreviewData, String> {
    if cancellation.is_cancelled() {
        return Err("Thumbnail cancelled.".into());
    }
    if format == image::ImageFormat::WebP {
        super::webp_header::validate(bytes)?;
    }
    // Reject oversized PNG rasters before read_info can process ancillary chunks.
    // Signature and IHDR shape are checked here; the decoder verifies CRC/content.
    if format == image::ImageFormat::Png {
        if bytes.len() < 33
            || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
            || bytes[8..12] != [0, 0, 0, 13]
            || &bytes[12..16] != b"IHDR"
        {
            return Err("Cannot load thumbnail: invalid PNG header. Use Open.".into());
        }
        let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        dimensions(width, height)?;
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    // Decoder allocation accounting is best-effort, not a process-memory cap.
    // The byte/pixel limits and output shape are checked separately.
    limits.max_alloc = Some(128 * 1024 * 1024);
    limits.max_image_width = Some(DIMENSION_LIMIT);
    limits.max_image_height = Some(DIMENSION_LIMIT);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|error| format!("Cannot decode thumbnail: {error}. Use Open."))?;
    let (width, height) = decoder.dimensions();
    dimensions(width, height)?;
    let orientation = decoder
        .orientation()
        .map_err(|error| format!("Cannot inspect thumbnail orientation: {error}. Use Open."))?;
    if cancellation.is_cancelled() {
        return Err("Thumbnail cancelled.".into());
    }
    let mut image = image::DynamicImage::from_decoder(decoder)
        .map_err(|error| format!("Cannot decode thumbnail: {error}. Use Open."))?;
    if cancellation.is_cancelled() {
        return Err("Thumbnail cancelled.".into());
    }
    image.apply_orientation(orientation);
    dimensions(image.width(), image.height())?;
    if cancellation.is_cancelled() {
        return Err("Thumbnail cancelled.".into());
    }
    let image = if image.width() > 256 || image.height() > 256 {
        image.thumbnail(256, 256)
    } else {
        image
    };
    Ok(PreviewData::Image {
        width: image.width(),
        height: image.height(),
        rgba: Arc::from(image.into_rgba8().into_raw()),
    })
}

pub(super) fn dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0
        || height == 0
        || width > DIMENSION_LIMIT
        || height > DIMENSION_LIMIT
        || u64::from(width) * u64::from(height) > PIXEL_LIMIT
    {
        return Err("Cannot load thumbnail: dimensions exceed the 16-megapixel or 16,384-pixel axis limit, or are zero. Use Open.".into());
    }
    Ok(())
}

fn unchanged(before: &std::fs::Metadata, after: &std::fs::Metadata) -> bool {
    let same = after.is_file()
        && before.len() == after.len()
        && before.modified().ok() == after.modified().ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        same && before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        same
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filesystem::tests::{Fixture, wait};

    fn encoded(format: image::ImageFormat) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            2,
            2,
            image::Rgb([30, 80, 120]),
        ))
        .write_to(&mut bytes, format)?;
        Ok(bytes.into_inner())
    }

    #[test]
    fn corrupt_and_truncated_images_do_not_poison_the_worker_or_change_files()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = Fixture::new()?;
        let workers = PreviewWorkers::new()?;
        let png = encoded(image::ImageFormat::Png)?;
        let jpeg = encoded(image::ImageFormat::Jpeg)?;
        let mut corrupt_png = png.clone();
        corrupt_png[29] ^= 0xff;
        let cases = [
            ("plain.png", b"not an image".to_vec()),
            ("archive.png", b"PK\x03\x04synthetic".to_vec()),
            ("pdf.png", b"%PDF-1.4 synthetic".to_vec()),
            ("crc.png", corrupt_png),
            ("header.png", png[..24].to_vec()),
            ("empty.jpg", vec![]),
            ("header.jpg", jpeg[..16].to_vec()),
            ("broken.jpg", vec![0xff, 0xd8, 0xff, 0xff, 0, 0]),
        ];
        for (index, (name, bytes)) in cases.into_iter().enumerate() {
            let path = fixture.path().join(name);
            std::fs::write(&path, &bytes)?;
            let outcome = wait(workers.read(PreviewRequest {
                view: crate::domain::ViewId(1),
                generation: index as u64,
                path: path.clone(),
            }));
            assert!(
                outcome.result.is_err(),
                "{name} must fail rather than become text/success"
            );
            assert_eq!(std::fs::read(&path)?, bytes);
        }
        let oversize = fixture.path().join("oversize.png");
        std::fs::File::create(&oversize)?.set_len(IMAGE_LIMIT + 1)?;
        let outcome = wait(workers.read(PreviewRequest {
            view: crate::domain::ViewId(1),
            generation: 99,
            path: oversize.clone(),
        }));
        assert_eq!(
            outcome.result.err().as_deref(),
            Some("Cannot load thumbnail: image exceeds 20 MiB. Use Open.")
        );
        assert_eq!(std::fs::metadata(oversize)?.len(), IMAGE_LIMIT + 1);
        for (index, (name, bytes)) in [("valid.png", png), ("valid.jpg", jpeg)]
            .into_iter()
            .enumerate()
        {
            let path = fixture.path().join(name);
            std::fs::write(&path, &bytes)?;
            let outcome = wait(workers.read(PreviewRequest {
                view: crate::domain::ViewId(1),
                generation: 100 + index as u64,
                path: path.clone(),
            }));
            assert!(matches!(
                outcome.result,
                Ok(PreviewData::Image {
                    width: 2,
                    height: 2,
                    rgba,
                }) if rgba.len() == 2 * 2 * 4
            ));
            assert_eq!(std::fs::read(path)?, bytes);
        }
        Ok(())
    }
}
