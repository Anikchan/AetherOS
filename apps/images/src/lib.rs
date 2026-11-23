//! AetherOS Images (P4-T12, DA-AC-04).
//!
//! [`viewer`] decodes PNG/JPEG/GIF/BMP and owns the zoom, fit, rotate, flip,
//! folder-order and thumbnail-cache rules. This module maps them to Slint and
//! keeps the decode/transform pipeline in Rust.

slint::include_modules!();

pub mod viewer;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use image::{DynamicImage, GenericImageView};
use slint::{ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};

use viewer::{ImageInfo, Thumbnail, ThumbnailCache, ViewerError};

/// Fallback canvas size used before the window reports its real size.
const FALLBACK_CANVAS: (f32, f32) = (860.0, 560.0);
const THUMBNAIL_SIDE: u32 = 96;

/// Everything the window shows.
pub struct ImageState {
    pub folder: PathBuf,
    pub files: Vec<PathBuf>,
    pub index: usize,
    pub image: Option<DynamicImage>,
    pub info: Option<ImageInfo>,
    pub zoom: f32,
    /// True while the zoom follows the canvas size (Fit mode).
    pub fit_mode: bool,
    pub rotation: u8,
    pub flipped_h: bool,
    pub flipped_v: bool,
    pub thumbs: ThumbnailCache,
    pub status: String,
    pub error: String,
}

impl ImageState {
    fn current_path(&self) -> Option<&Path> {
        self.files.get(self.index).map(PathBuf::as_path)
    }

    fn dimensions(&self) -> (u32, u32) {
        self.image
            .as_ref()
            .map(|image| image.dimensions())
            .unwrap_or((0, 0))
    }
}

fn image_from_rgba(width: u32, height: u32, rgba: &[u8]) -> slint::Image {
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
    let pixels = buffer.make_mut_slice();
    for (pixel, chunk) in pixels.iter_mut().zip(rgba.chunks_exact(4)) {
        *pixel = Rgba8Pixel::new(chunk[0], chunk[1], chunk[2], chunk[3]);
    }
    slint::Image::from_rgba8(buffer)
}

fn slint_image(image: &DynamicImage) -> slint::Image {
    let (width, height, rgba) = viewer::rgba_bytes(image);
    image_from_rgba(width, height, &rgba)
}

fn thumbnail_image(thumb: &Thumbnail) -> slint::Image {
    image_from_rgba(thumb.width, thumb.height, &thumb.rgba)
}

fn canvas_box(ui: &ImagesApp) -> (f32, f32) {
    let width = ui.get_canvas_width();
    let height = ui.get_canvas_height();
    if width > 1.0 && height > 1.0 {
        (width, height)
    } else {
        FALLBACK_CANVAS
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn format_time(time: Option<std::time::SystemTime>) -> String {
    let Some(time) = time else {
        return "—".to_string();
    };
    let seconds = time
        .duration_since(std::time::UNIX_EPOCH)
        .map(|delta| delta.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rest / 3600,
        (rest % 3600) / 60
    )
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Recompute the zoom for Fit mode from the current canvas.
fn apply_fit(ui: &ImagesApp, state: &mut ImageState) {
    let (width, height) = state.dimensions();
    let (box_width, box_height) = canvas_box(ui);
    state.zoom = viewer::fit_scale(width, height, box_width, box_height);
    state.fit_mode = true;
}

/// Decode the file at `index`, reset the transforms and fit it to the canvas.
fn load_current(ui: &ImagesApp, state: &mut ImageState) {
    let Some(path) = state.current_path().map(Path::to_path_buf) else {
        state.image = None;
        state.info = None;
        state.status = "No images".to_string();
        return;
    };
    match viewer::decode(&path) {
        Ok(image) => {
            let info = viewer::info(&path).ok();
            state.image = Some(image);
            state.info = info;
            state.rotation = 0;
            state.flipped_h = false;
            state.flipped_v = false;
            apply_fit(ui, state);
            state.error.clear();
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            state.status = format!(
                "{name} · {} of {} · {}",
                state.index + 1,
                state.files.len(),
                state
                    .info
                    .as_ref()
                    .map(|info| info.format.clone())
                    .unwrap_or_else(|| "unknown".to_string())
            );
        }
        Err(error) => {
            state.image = None;
            state.info = None;
            state.error = error.to_string();
            state.status = "Could not decode the image".to_string();
        }
    }
}

fn build_thumbs(state: &mut ImageState) -> Vec<ImageThumb> {
    let files = state.files.clone();
    let current = state.index;
    files
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let picture = match state.thumbs.render(path, THUMBNAIL_SIDE) {
                Ok(thumb) => thumbnail_image(thumb),
                Err(_) => slint::Image::default(),
            };
            ImageThumb {
                name: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
                    .into(),
                picture,
                selected: index == current,
            }
        })
        .collect()
}

fn refresh(ui: &ImagesApp, state: &mut ImageState) {
    let thumbs = build_thumbs(state);
    let picture = state
        .image
        .as_ref()
        .map(slint_image)
        .unwrap_or_default();
    let (width, height) = state.dimensions();
    ui.set_picture(picture);
    ui.set_has_image(state.image.is_some());
    ui.set_image_width(width as i32);
    ui.set_image_height(height as i32);
    ui.set_zoom(state.zoom);
    ui.set_file_name(
        state
            .current_path()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
            .into(),
    );
    ui.set_dimensions(if state.image.is_some() { format!("{width} × {height}").into() } else { "—".into() });
    ui.set_format_label(
        state
            .info
            .as_ref()
            .map(|info| info.format.clone())
            .unwrap_or_else(|| "—".to_string())
            .into(),
    );
    ui.set_file_size(
        state
            .info
            .as_ref()
            .map(|info| format_bytes(info.bytes))
            .unwrap_or_else(|| "—".to_string())
            .into(),
    );
    ui.set_modified(
        format_time(state.info.as_ref().and_then(|info| info.modified)).into(),
    );
    ui.set_folder(state.folder.display().to_string().into());
    ui.set_thumbs(ModelRc::new(VecModel::from(thumbs)));
    ui.set_current_index(state.index as i32);
    ui.set_total_images(state.files.len() as i32);
    ui.set_flipped_h(state.flipped_h);
    ui.set_flipped_v(state.flipped_v);
    ui.set_status(state.status.clone().into());
    ui.set_error(state.error.clone().into());
}

// --- actions -------------------------------------------------------------

fn action_open(ui: &ImagesApp, state: &mut ImageState, folder: &str) {
    let trimmed = folder.trim().trim_matches('"');
    if trimmed.is_empty() {
        state.error = "Enter a folder path first".to_string();
        refresh(ui, state);
        return;
    }
    let folder = PathBuf::from(trimmed);
    match viewer::folder_images(&folder) {
        Ok(files) => {
            if files.is_empty() {
                state.error = ViewerError::NoImages(folder.clone()).to_string();
                state.status = "No images in that folder".to_string();
                refresh(ui, state);
                return;
            }
            state.thumbs.retain(&files);
            state.files = files;
            state.index = 0;
            state.folder = folder;
            ui.set_folder_path(state.folder.display().to_string().into());
            load_current(ui, state);
        }
        Err(error) => {
            state.error = error.to_string();
            state.status = "Could not open the folder".to_string();
        }
    }
    refresh(ui, state);
}

fn action_step(ui: &ImagesApp, state: &mut ImageState, delta: i32) {
    if state.files.is_empty() {
        return;
    }
    let len = state.files.len() as i32;
    let next = (state.index as i32 + delta).rem_euclid(len);
    state.index = next as usize;
    load_current(ui, state);
    refresh(ui, state);
}

fn action_zoom(ui: &ImagesApp, state: &mut ImageState, steps: i32) {
    if state.image.is_none() {
        return;
    }
    state.fit_mode = false;
    state.zoom = viewer::zoom_by(state.zoom, steps);
    state.status = format!("Zoom {}%", (state.zoom * 100.0).round() as i32);
    refresh(ui, state);
}

fn action_actual_size(ui: &ImagesApp, state: &mut ImageState) {
    if state.image.is_none() {
        return;
    }
    state.fit_mode = false;
    state.zoom = 1.0;
    state.status = "Actual size · 100%".to_string();
    refresh(ui, state);
}

fn action_rotate(ui: &ImagesApp, state: &mut ImageState) {
    let Some(image) = state.image.take() else {
        return;
    };
    state.image = Some(viewer::rotate(image, 1));
    state.rotation = (state.rotation + 1) % 4;
    let (width, height) = state.dimensions();
    state.status = format!("Rotated 90° clockwise · {width} × {height}");
    refresh(ui, state);
}

fn action_flip(ui: &ImagesApp, state: &mut ImageState, horizontal: bool) {
    let Some(image) = state.image.take() else {
        return;
    };
    if horizontal {
        state.image = Some(viewer::flip_horizontal(image));
        state.flipped_h = !state.flipped_h;
        state.status = format!("Flipped horizontally · {}", if state.flipped_h { "on" } else { "off" });
    } else {
        state.image = Some(viewer::flip_vertical(image));
        state.flipped_v = !state.flipped_v;
        state.status = format!("Flipped vertically · {}", if state.flipped_v { "on" } else { "off" });
    }
    refresh(ui, state);
}

fn action_canvas_resized(ui: &ImagesApp, state: &mut ImageState) {
    if state.image.is_some() && state.fit_mode {
        apply_fit(ui, state);
        ui.set_zoom(state.zoom);
    }
}

/// Owns the Images window and its state.
pub struct ImagesController {
    ui: ImagesApp,
    state: Rc<RefCell<ImageState>>,
}

impl ImagesController {
    pub fn new() -> Result<Self, String> {
        let ui = ImagesApp::new().map_err(|error| error.to_string())?;
        let state = ImageState {
            folder: PathBuf::new(),
            files: Vec::new(),
            index: 0,
            image: None,
            info: None,
            zoom: 1.0,
            fit_mode: true,
            rotation: 0,
            flipped_h: false,
            flipped_v: false,
            thumbs: ThumbnailCache::new(),
            status: "Open an image to begin".to_string(),
            error: String::new(),
        };
        let state = Rc::new(RefCell::new(state));
        let controller = Self { ui, state };
        controller.wire();
        refresh(&controller.ui, &mut controller.state.borrow_mut());
        Ok(controller)
    }

    /// Generate a small album in a temporary folder so the rendered frame
    /// shows a real decode, real thumbnails and a fitted image.
    pub fn demo() -> Result<Self, String> {
        let root = std::env::temp_dir().join("aether-images-demo");
        let _ = std::fs::remove_dir_all(&root);
        build_demo_album(&root)?;
        let controller = Self::new()?;
        controller.open_folder(&root.display().to_string());
        {
            let mut state = controller.state.borrow_mut();
            state.status = format!(
                "{} images in {} · sorted by name",
                state.files.len(),
                root.display()
            );
        }
        refresh(&controller.ui, &mut controller.state.borrow_mut());
        Ok(controller)
    }

    pub fn ui(&self) -> &ImagesApp {
        &self.ui
    }

    pub fn state(&self) -> std::cell::Ref<'_, ImageState> {
        self.state.borrow()
    }

    // --- public actions (also used by the headless interaction test) ------

    pub fn open_folder(&self, folder: &str) {
        let mut state = self.state.borrow_mut();
        action_open(&self.ui, &mut state, folder);
    }

    pub fn previous(&self) {
        let mut state = self.state.borrow_mut();
        action_step(&self.ui, &mut state, -1);
    }

    pub fn next(&self) {
        let mut state = self.state.borrow_mut();
        action_step(&self.ui, &mut state, 1);
    }

    pub fn zoom_in(&self) {
        let mut state = self.state.borrow_mut();
        action_zoom(&self.ui, &mut state, 1);
    }

    pub fn zoom_out(&self) {
        let mut state = self.state.borrow_mut();
        action_zoom(&self.ui, &mut state, -1);
    }

    pub fn fit(&self) {
        let mut state = self.state.borrow_mut();
        apply_fit(&self.ui, &mut state);
        state.status = format!("Fit to window · {}%", (state.zoom * 100.0).round() as i32);
        refresh(&self.ui, &mut state);
    }

    pub fn actual_size(&self) {
        let mut state = self.state.borrow_mut();
        action_actual_size(&self.ui, &mut state);
    }

    pub fn rotate_cw(&self) {
        let mut state = self.state.borrow_mut();
        action_rotate(&self.ui, &mut state);
    }

    pub fn flip_h(&self) {
        let mut state = self.state.borrow_mut();
        action_flip(&self.ui, &mut state, true);
    }

    pub fn flip_v(&self) {
        let mut state = self.state.borrow_mut();
        action_flip(&self.ui, &mut state, false);
    }

    pub fn select_thumb(&self, index: i32) {
        let mut state = self.state.borrow_mut();
        if state.files.is_empty() {
            return;
        }
        let index = index.max(0) as usize % state.files.len();
        state.index = index;
        load_current(&self.ui, &mut state);
        refresh(&self.ui, &mut state);
    }

    fn wire(&self) {
        macro_rules! bind {
            ($setter:ident, |$($arg:ident),*| ($ui:ident, $st:ident) => $body:block) => {{
                let weak = self.ui.as_weak();
                let state = self.state.clone();
                self.ui.$setter(move |$($arg),*| {
                    if let Some($ui) = weak.upgrade() {
                        let mut $st = state.borrow_mut();
                        $body
                    }
                });
            }};
            ($setter:ident, ($ui:ident, $st:ident) => $body:block) => {{
                let weak = self.ui.as_weak();
                let state = self.state.clone();
                self.ui.$setter(move || {
                    if let Some($ui) = weak.upgrade() {
                        let mut $st = state.borrow_mut();
                        $body
                    }
                });
            }};
        }

        bind!(on_previous, (ui, state) => {
            action_step(&ui, &mut state, -1);
        });
        bind!(on_next, (ui, state) => {
            action_step(&ui, &mut state, 1);
        });
        bind!(on_zoom_in, (ui, state) => {
            action_zoom(&ui, &mut state, 1);
        });
        bind!(on_zoom_out, (ui, state) => {
            action_zoom(&ui, &mut state, -1);
        });
        bind!(on_fit, (ui, state) => {
            apply_fit(&ui, &mut state);
            state.status = format!("Fit to window · {}%", (state.zoom * 100.0).round() as i32);
            refresh(&ui, &mut state);
        });
        bind!(on_actual_size, (ui, state) => {
            action_actual_size(&ui, &mut state);
        });
        bind!(on_rotate_cw, (ui, state) => {
            action_rotate(&ui, &mut state);
        });
        bind!(on_flip_h, (ui, state) => {
            action_flip(&ui, &mut state, true);
        });
        bind!(on_flip_v, (ui, state) => {
            action_flip(&ui, &mut state, false);
        });
        bind!(on_select_thumb, |index| (ui, state) => {
            if !state.files.is_empty() {
                let index = index.max(0) as usize % state.files.len();
                state.index = index;
                load_current(&ui, &mut state);
                refresh(&ui, &mut state);
            }
        });
        bind!(on_open_folder, |folder| (ui, state) => {
            action_open(&ui, &mut state, &folder);
        });
        bind!(on_canvas_resized, (ui, state) => {
            action_canvas_resized(&ui, &mut state);
        });
    }
}

/// Write the demo album: smooth gradients with a sun disc, one format each.
fn build_demo_album(root: &Path) -> Result<(), String> {
    use std::fs;

    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let images = [
        ("Aurora.png", 1600, 1000, 0.0_f32),
        ("Coast.jpg", 1400, 900, 0.35),
        ("Lumen.png", 1200, 800, 0.7),
        ("Orbit.png", 900, 900, 1.0),
        ("Meadow.bmp", 800, 600, 1.4),
    ];
    for (name, width, height, shift) in images {
        let image = demo_image(width, height, shift);
        if name.ends_with(".jpg") {
            image
                .to_rgb8()
                .save(root.join(name))
                .map_err(|error| error.to_string())?;
        } else {
            image
                .save(root.join(name))
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// A smooth sunrise gradient with a bright sun: synthetic, no user files.
fn demo_image(width: u32, height: u32, shift: f32) -> DynamicImage {
    let mut buffer = image::RgbaImage::new(width, height);
    let sun_x = 0.66 + 0.06 * shift;
    let sun_y = 0.58;
    for (x, y, pixel) in buffer.enumerate_pixels_mut() {
        let fx = x as f32 / width.max(1) as f32;
        let fy = y as f32 / height.max(1) as f32;
        let distance = ((fx - sun_x).powi(2) * 1.0 + (fy - sun_y).powi(2) * 1.5).sqrt();
        let sun = (1.0 - (distance / 0.18).min(1.0)).powf(2.2);
        let horizon = fy.powf(1.6);
        let red = (28.0 + 170.0 * horizon + 190.0 * sun).min(255.0);
        let green = (34.0 + 92.0 * horizon + 150.0 * sun).min(255.0);
        let blue = (86.0 + 40.0 * (1.0 - horizon) + 70.0 * sun - 30.0 * shift).clamp(0.0, 255.0);
        let halo = ((fx - 0.5) * 0.25).abs();
        *pixel = image::Rgba([
            (red - 12.0 * halo).clamp(0.0, 255.0) as u8,
            (green - 8.0 * halo).clamp(0.0, 255.0) as u8,
            (blue + 20.0 * halo).clamp(0.0, 255.0) as u8,
            255,
        ]);
    }
    DynamicImage::ImageRgba8(buffer)
}
