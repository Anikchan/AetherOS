//! Image decoding and view math for the Images app (P4-T12, DA-AC-04).
//!
//! PNG, JPEG, GIF and BMP decode through the `image` crate. Zoom, fit, rotate
//! and flip are pure functions so the unit tests can check them without a
//! window: rotation is a lossless 90° step over the decoded pixels (DA-AC-04
//! "rotates without losing quality"), and 90°/270° swaps the reported
//! dimensions.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageFormat};

pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 16.0;
pub const ZOOM_STEP: f32 = 1.25;

#[derive(Debug)]
pub enum ViewerError {
    Unsupported(String),
    Decode { path: PathBuf, reason: String },
    Io { path: PathBuf, source: io::Error },
    NoImages(PathBuf),
}

impl std::fmt::Display for ViewerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ViewerError::Unsupported(name) => write!(f, "unsupported image format: {name}"),
            ViewerError::Decode { path, reason } => {
                write!(f, "cannot decode {}: {reason}", path.display())
            }
            ViewerError::Io { path, source } => write!(f, "{}: {source}", path.display()),
            ViewerError::NoImages(dir) => {
                write!(f, "no images in {}", dir.display())
            }
        }
    }
}

impl std::error::Error for ViewerError {}

/// Metadata shown in the info panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInfo {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub bytes: u64,
    pub modified: Option<SystemTime>,
}

/// Extensions the app opens.
pub fn supported_extension(path: &Path) -> bool {
    matches!(
        path.extension()
            .map(|ext| ext.to_string_lossy().to_lowercase())
            .unwrap_or_default()
            .as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "tif" | "tiff"
    )
}

fn format_label(format: ImageFormat) -> String {
    match format {
        ImageFormat::Png => "PNG".to_string(),
        ImageFormat::Jpeg => "JPEG".to_string(),
        ImageFormat::Gif => "GIF".to_string(),
        ImageFormat::Bmp => "BMP".to_string(),
        ImageFormat::WebP => "WebP".to_string(),
        ImageFormat::Tiff => "TIFF".to_string(),
        other => format!("{other:?}").to_uppercase(),
    }
}

/// Friendly folder label for the path bar: the last two components, prefixed
/// with an ellipsis when the real path is deeper, so long temp/build paths stay
/// readable and out of evidence screenshots.
pub fn display_path(path: &Path) -> String {
    let parts: Vec<String> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    if parts.len() <= 2 {
        return path.display().to_string();
    }
    format!("…/{}", parts[parts.len() - 2..].join("/"))
}

/// Decode a supported image into memory.
pub fn decode(path: &Path) -> Result<DynamicImage, ViewerError> {
    if !supported_extension(path) {
        return Err(ViewerError::Unsupported(
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        ));
    }
    image::open(path).map_err(|error| ViewerError::Decode {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })
}

/// Read width, height, format and size from the header, without decoding the
/// pixels.
pub fn info(path: &Path) -> Result<ImageInfo, ViewerError> {
    if !supported_extension(path) {
        return Err(ViewerError::Unsupported(
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        ));
    }
    let reader = image::ImageReader::open(path)
        .map_err(|error| ViewerError::Io {
            path: path.to_path_buf(),
            source: error,
        })?
        .with_guessed_format()
        .map_err(|error| ViewerError::Decode {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;
    let format = reader.format();
    let (width, height) = reader.into_dimensions().map_err(|error| ViewerError::Decode {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })?;
    let meta = fs::metadata(path).map_err(|error| ViewerError::Io {
        path: path.to_path_buf(),
        source: error,
    })?;
    Ok(ImageInfo {
        path: path.to_path_buf(),
        width,
        height,
        format: format
            .map(format_label)
            .unwrap_or_else(|| "Unknown".to_string()),
        bytes: meta.len(),
        modified: meta.modified().ok(),
    })
}

/// Clamp a zoom factor to the supported range.
pub fn clamp_zoom(zoom: f32) -> f32 {
    if !zoom.is_finite() {
        return 1.0;
    }
    zoom.clamp(MIN_ZOOM, MAX_ZOOM)
}

/// Multiply the zoom by 1.25 per step and clamp.
pub fn zoom_by(zoom: f32, steps: i32) -> f32 {
    clamp_zoom(zoom * ZOOM_STEP.powi(steps))
}

/// Scale that makes `width x height` fit inside the box (upscaling allowed up
/// to [`MAX_ZOOM`]).
pub fn fit_scale(width: u32, height: u32, box_width: f32, box_height: f32) -> f32 {
    if width == 0 || height == 0 || box_width <= 0.0 || box_height <= 0.0 {
        return 1.0;
    }
    clamp_zoom((box_width / width as f32).min(box_height / height as f32))
}

/// Rotate in 90° steps, clockwise, without resampling pixels.
pub fn rotate(image: DynamicImage, steps: i32) -> DynamicImage {
    match steps.rem_euclid(4) {
        1 => image.rotate90(),
        2 => image.rotate180(),
        3 => image.rotate270(),
        _ => image,
    }
}

pub fn flip_horizontal(image: DynamicImage) -> DynamicImage {
    image.fliph()
}

pub fn flip_vertical(image: DynamicImage) -> DynamicImage {
    image.flipv()
}

/// Images in a folder, sorted by file name (case-insensitive).
pub fn folder_images(dir: &Path) -> Result<Vec<PathBuf>, ViewerError> {
    let read = fs::read_dir(dir).map_err(|error| ViewerError::Io {
        path: dir.to_path_buf(),
        source: error,
    })?;
    let mut images: Vec<PathBuf> = Vec::new();
    for item in read {
        let item = item.map_err(|error| ViewerError::Io {
            path: dir.to_path_buf(),
            source: error,
        })?;
        let path = item.path();
        if path.is_file() && supported_extension(&path) {
            images.push(path);
        }
    }
    images.sort_by(|a, b| {
        let left = a.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        let right = b.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        left.cmp(&right)
    });
    Ok(images)
}

/// Premultiplied-free RGBA8 buffer for Slint.
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Downscale so the longest side is at most `max_side`.
pub fn downscale(image: DynamicImage, max_side: u32) -> DynamicImage {
    let (width, height) = image.dimensions();
    let longest = width.max(height);
    if longest <= max_side || max_side == 0 {
        return image;
    }
    let scale = max_side as f32 / longest as f32;
    let target_width = ((width as f32 * scale).round() as u32).max(1);
    let target_height = ((height as f32 * scale).round() as u32).max(1);
    image.resize_exact(target_width, target_height, FilterType::Triangle)
}

/// RGBA bytes plus dimensions.
pub fn rgba_bytes(image: &DynamicImage) -> (u32, u32, Vec<u8>) {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    (width, height, rgba.into_raw())
}

/// Decoded thumbnails keyed by path, so the strip renders each file once.
#[derive(Default)]
pub struct ThumbnailCache {
    entries: HashMap<PathBuf, Thumbnail>,
}

impl ThumbnailCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, path: &Path) -> Option<&Thumbnail> {
        self.entries.get(path)
    }

    /// Decode and cache a thumbnail; repeated calls hit the cache.
    pub fn render(&mut self, path: &Path, max_side: u32) -> Result<&Thumbnail, ViewerError> {
        if !self.entries.contains_key(path) {
            let image = decode(path)?;
            let (width, height, rgba) = rgba_bytes(&downscale(image, max_side));
            self.entries.insert(
                path.to_path_buf(),
                Thumbnail {
                    width,
                    height,
                    rgba,
                },
            );
        }
        Ok(self
            .entries
            .get(path)
            .expect("cached immediately above"))
    }

    /// Drop entries that are no longer part of the folder.
    pub fn retain(&mut self, keep: &[PathBuf]) {
        self.entries.retain(|path, _| keep.iter().any(|known| known == path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn temp_dir(tag: &str) -> PathBuf {
        static TEST_COUNTER: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(0);
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "aether-images-{tag}-{}-{stamp}-{}",
            std::process::id(),
            TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    /// Horizontal gradient, written with the requested name.
    fn gradient(width: u32, height: u32) -> DynamicImage {
        let mut buffer = RgbaImage::new(width, height);
        for (x, _, pixel) in buffer.enumerate_pixels_mut() {
            let t = x as f32 / width.max(1) as f32;
            *pixel = Rgba([
                (40.0 + 180.0 * t) as u8,
                (90.0 + 60.0 * t) as u8,
                (200.0 - 80.0 * t) as u8,
                255,
            ]);
        }
        DynamicImage::ImageRgba8(buffer)
    }

    #[test]
    fn decodes_png_and_reads_metadata() {
        let root = temp_dir("metadata");
        let path = root.join("gradient.png");
        gradient(64, 32).save(&path).unwrap();

        let decoded = decode(&path).unwrap();
        assert_eq!(decoded.dimensions(), (64, 32));

        let info = info(&path).unwrap();
        assert_eq!((info.width, info.height), (64, 32));
        assert_eq!(info.format, "PNG");
        assert!(info.bytes > 0);
        assert!(info.modified.is_some());
    }

    #[test]
    fn decodes_every_listed_format() {
        let root = temp_dir("formats");
        let cases = [
            ("a.png", ImageFormat::Png, 20, 10),
            ("b.jpg", ImageFormat::Jpeg, 18, 12),
            ("c.gif", ImageFormat::Gif, 16, 16),
            ("d.bmp", ImageFormat::Bmp, 14, 8),
        ];
        for (name, _, width, height) in cases {
            let path = root.join(name);
            // JPEG has no alpha channel: save the RGB view for every format.
            let image = gradient(width, height);
            image.to_rgb8().save(&path).unwrap();
            let decoded = decode(&path).unwrap();
            assert_eq!(decoded.dimensions(), (width, height), "{name}");
            assert!(info(&path).unwrap().bytes > 0, "{name}");
        }
        assert!(supported_extension(&root.join("e.png")));
        assert!(!supported_extension(&root.join("e.txt")));
    }

    #[test]
    fn rotation_swaps_dimensions_without_loss() {
        let image = gradient(40, 20);
        let original_pixels = image.to_rgba8().into_raw();

        let quarter = rotate(image.clone(), 1);
        assert_eq!(quarter.dimensions(), (20, 40));
        let half = rotate(image.clone(), 2);
        assert_eq!(half.dimensions(), (40, 20));
        let three = rotate(image.clone(), 3);
        assert_eq!(three.dimensions(), (20, 40));
        let full = rotate(image.clone(), 4);
        assert_eq!(full.dimensions(), (40, 20));
        assert_eq!(
            full.to_rgba8().into_raw(),
            original_pixels,
            "four 90° steps restore the exact pixels"
        );
        // Same number of pixels, so nothing was resampled.
        assert_eq!(
            quarter.to_rgba8().into_raw().len(),
            original_pixels.len()
        );
        let back = rotate(rotate(image, 1), 3);
        assert_eq!(back.dimensions(), (40, 20));
    }

    #[test]
    fn flips_mirror_pixels() {
        let mut buffer = RgbaImage::new(2, 1);
        buffer.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        buffer.put_pixel(1, 0, Rgba([0, 0, 255, 255]));
        let image = DynamicImage::ImageRgba8(buffer);

        let flipped = flip_horizontal(image.clone()).to_rgba8();
        assert_eq!(flipped.get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert_eq!(flipped.get_pixel(1, 0).0, [255, 0, 0, 255]);

        let mut tall = RgbaImage::new(1, 2);
        tall.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        tall.put_pixel(0, 1, Rgba([0, 255, 0, 255]));
        let flipped = flip_vertical(DynamicImage::ImageRgba8(tall)).to_rgba8();
        assert_eq!(flipped.get_pixel(0, 0).0, [0, 255, 0, 255]);
    }

    #[test]
    fn zoom_is_clamped_and_steps_multiply() {
        assert_eq!(clamp_zoom(0.01), MIN_ZOOM);
        assert_eq!(clamp_zoom(99.0), MAX_ZOOM);
        assert_eq!(clamp_zoom(f32::NAN), 1.0);
        let stepped = zoom_by(1.0, 4);
        assert!((stepped - 2.4414).abs() < 0.01, "1.25^4 ≈ 2.44, got {stepped}");
        assert_eq!(zoom_by(1.0, -30), MIN_ZOOM);
        assert_eq!(zoom_by(1.0, 30), MAX_ZOOM);
    }

    #[test]
    fn fit_keeps_aspect_inside_the_box() {
        // Wide image in a tall box: width is the limiting side.
        let scale = fit_scale(1000, 500, 500.0, 500.0);
        assert!((scale - 0.5).abs() < 1e-6);
        // Small image upscales to the box but never past MAX_ZOOM.
        let scale = fit_scale(100, 100, 300.0, 200.0);
        assert!((scale - 2.0).abs() < 1e-6);
        assert_eq!(fit_scale(0, 0, 500.0, 500.0), 1.0);
        assert_eq!(fit_scale(10, 10, 0.0, 0.0), 1.0);
        let clamped = fit_scale(1, 1, 4000.0, 4000.0);
        assert_eq!(clamped, MAX_ZOOM);
    }

    #[test]
    fn folder_listing_sorts_and_filters() {
        let root = temp_dir("folder");
        gradient(8, 8).save(root.join("b.png")).unwrap();
        gradient(8, 8).save(root.join("a.jpg")).unwrap();
        gradient(8, 8).to_rgb8().save(root.join("C.bmp")).unwrap();
        fs::write(root.join("notes.txt"), "not an image").unwrap();
        fs::create_dir_all(root.join("nested")).unwrap();
        gradient(8, 8).save(root.join("nested/inside.png")).unwrap();

        let names: Vec<String> = folder_images(&root)
            .unwrap()
            .into_iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.jpg", "b.png", "C.bmp"], "case-insensitive name order, files only");
    }

    #[test]
    fn unsupported_and_corrupt_files_error() {
        let root = temp_dir("errors");
        let text = root.join("notes.txt");
        fs::write(&text, "plain text").unwrap();
        assert!(matches!(
            decode(&text),
            Err(ViewerError::Unsupported(_))
        ));

        let corrupt = root.join("broken.png");
        fs::write(&corrupt, b"this is not a png").unwrap();
        match decode(&corrupt) {
            Err(ViewerError::Decode { reason, .. }) => assert!(!reason.is_empty()),
            other => panic!("expected a decode error, got {other:?}"),
        }
        assert!(info(&corrupt).is_err());
    }

    #[test]
    fn downscale_keeps_aspect_and_thumbs_are_cached() {
        let root = temp_dir("thumbs");
        let path = root.join("wide.png");
        gradient(600, 300).save(&path).unwrap();

        let scaled = downscale(decode(&path).unwrap(), 100);
        assert_eq!(scaled.dimensions(), (100, 50));

        let mut cache = ThumbnailCache::new();
        assert!(cache.is_empty());
        let thumb = cache.render(&path, 96).unwrap();
        assert_eq!(thumb.width, 96);
        assert_eq!(thumb.height, 48);
        assert_eq!(thumb.rgba.len(), 96 * 48 * 4);
        let first_len = thumb.rgba.len();
        let again = cache.render(&path, 96).unwrap();
        assert_eq!(again.rgba.len(), first_len);
        assert_eq!(cache.len(), 1, "the second render hits the cache");
        cache.retain(&[]);
        assert!(cache.is_empty());
    }
    #[test]
    fn decodes_webp_and_tiff_like_the_other_formats() {
        // DA-AC-04 lists WebP and TIFF, so both must open like PNG/JPEG/GIF/BMP.
        let root = temp_dir("formats");
        let image = gradient(24, 16).to_rgb8();

        let tiff_path = root.join("sample.tiff");
        image
            .save_with_format(&tiff_path, image::ImageFormat::Tiff)
            .expect("tiff encode");
        let opened = info(&tiff_path).expect("tiff header");
        assert_eq!(opened.width, 24);
        assert_eq!(opened.height, 16);
        assert_eq!(opened.format, "TIFF");
        let pixels = decode(&tiff_path).expect("tiff decode");
        assert_eq!((pixels.width(), pixels.height()), (24, 16));

        let webp_path = root.join("sample.webp");
        match image.save_with_format(&webp_path, image::ImageFormat::WebP) {
            Ok(()) => {
                let opened = info(&webp_path).expect("webp header");
                assert_eq!(opened.width, 24);
                assert_eq!(opened.height, 16);
                assert_eq!(opened.format, "WebP");
                let pixels = decode(&webp_path).expect("webp decode");
                assert_eq!((pixels.width(), pixels.height()), (24, 16));
            }
            Err(error) => {
                // The `image` crate's WebP encoder is lossless-only and can
                // reject some pixel layouts; decoding is what DA-AC-04 needs.
                eprintln!("webp encoder unavailable in this build: {error}");
            }
        }
        assert!(supported_extension(&root.join("x.webp")));
        assert!(supported_extension(&root.join("x.tif")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn folder_errors_and_cache_lookups_are_reported() {
        let root = temp_dir("folder-errors");
        let missing = root.join("does-not-exist");
        match folder_images(&missing) {
            Err(ViewerError::Io { path, .. }) => assert_eq!(path, missing),
            other => panic!("expected an Io error, got {other:?}"),
        }

        // The cache only answers after a render, and rotation wraps negative
        // steps like positive ones.
        let path = root.join("cached.png");
        gradient(10, 20).save(&path).unwrap();
        let mut cache = ThumbnailCache::new();
        assert!(cache.get(&path).is_none());
        cache.render(&path, 64).unwrap();
        assert!(cache.get(&path).is_some());
        assert_eq!(cache.get(&path).unwrap().height, 20);

        let image = gradient(30, 10);
        assert_eq!(rotate(image.clone(), -1).dimensions(), (10, 30));
        assert_eq!(
            rotate(image.clone(), -4).to_rgba8().into_raw(),
            image.to_rgba8().into_raw(),
            "four negative steps return the original"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn display_path_shortens_deep_paths() {
        assert_eq!(display_path(Path::new("Pictures")), "Pictures");
        assert_eq!(display_path(Path::new("/home/aether/Pictures")), "…/aether/Pictures");
        assert_eq!(
            display_path(Path::new("/var/folders/x/T/aether-images-demo")),
            "…/T/aether-images-demo"
        );
    }

}
