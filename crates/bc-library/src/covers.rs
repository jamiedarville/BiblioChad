use std::path::Path;

use crate::{LibraryError, Result};

/// Longest edge of stored cover thumbnails, in pixels.
const COVER_MAX: u32 = 600;

/// Decode an image (JPEG/PNG/GIF/WebP), shrink it to a cover thumbnail and
/// write it as JPEG to `dest`. Rejects absurd dimensions before decoding.
pub fn save_cover_thumbnail(bytes: &[u8], dest: &Path) -> Result<()> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| LibraryError::Image(e.to_string()))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(12_000);
    limits.max_image_height = Some(12_000);
    limits.max_alloc = Some(256 * 1024 * 1024);
    let mut reader = reader;
    reader.limits(limits);
    let img = reader
        .decode()
        .map_err(|e| LibraryError::Image(e.to_string()))?;
    let img = if img.width() > COVER_MAX || img.height() > COVER_MAX {
        img.thumbnail(COVER_MAX, COVER_MAX)
    } else {
        img
    };
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension("tmp");
    img.into_rgb8()
        .save_with_format(&tmp, image::ImageFormat::Jpeg)
        .map_err(|e| LibraryError::Image(e.to_string()))?;
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_thumbnail() {
        let img = image::RgbImage::from_pixel(1200, 1800, image::Rgb([200, 30, 30]));
        let mut png = std::io::Cursor::new(Vec::new());
        img.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("covers/1.jpg");
        save_cover_thumbnail(png.get_ref(), &dest).unwrap();
        let out = image::open(&dest).unwrap();
        assert_eq!((out.width(), out.height()), (400, 600));
        assert!(save_cover_thumbnail(b"not an image", &dest).is_err());
    }
}
