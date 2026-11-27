//! AetherOS shared design system (P4, S3).
//!
//! Compiles `design.slint` (tokens from design D1, original icon geometry and
//! the widget kit used by the shell and the default apps) and adds two Rust
//! helpers:
//!
//! * [`render`] — a headless frame renderer. It installs a small Slint
//!   `Platform` over the software renderer, so any component can be drawn to a
//!   PNG without a display server. This is how P4 evidence screenshots are
//!   produced on the macOS dev host and in CI.
//! * [`theme`] — token/tier helpers shared by the shell and the apps.
//!
//! Panels use the glass tiers from design D1 section 4; the compositor owns the
//! backdrop blur (P3-T02) and the shell degrades to tint + edge highlight when
//! the blur extension is absent (finding from spike S1).

slint::include_modules!();

pub mod theme;


/// Headless rendering helpers. Kept in its own module so tests that use the
/// Slint testing backend never install two platforms in one process.
pub mod render {
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;

    use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor};
    use slint::platform::{Platform, WindowAdapter};
    use slint::{ComponentHandle, PhysicalSize, Rgb8Pixel};

    struct HeadlessPlatform {
        last_window: RefCell<Option<Rc<MinimalSoftwareWindow>>>,
    }

    impl HeadlessPlatform {
        fn new() -> Rc<Self> {
            Rc::new(Self {
                last_window: RefCell::new(None),
            })
        }
    }

    /// `set_platform` needs an owned value, while the renderer keeps a handle
    /// for the window it created. This wrapper keeps one `Rc` alive by owning
    /// it and hands out a second one on demand.
    struct SharedPlatform(Rc<HeadlessPlatform>);

    impl Platform for SharedPlatform {
        fn create_window_adapter(
            &self,
        ) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            self.0.create_window_adapter()
        }

        fn duration_since_start(&self) -> std::time::Duration {
            self.0.duration_since_start()
        }
    }

    impl Platform for HeadlessPlatform {
        fn create_window_adapter(
            &self,
        ) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            let window = MinimalSoftwareWindow::new(
                slint::platform::software_renderer::RepaintBufferType::NewBuffer,
            );
            *self.last_window.borrow_mut() = Some(window.clone());
            Ok(window)
        }

        fn duration_since_start(&self) -> std::time::Duration {
            std::time::Duration::from_millis(0)
        }
    }

    struct Renderer {
        platform: Rc<HeadlessPlatform>,
    }

    thread_local! {
        static RENDERER: RefCell<Option<Renderer>> = const { RefCell::new(None) };
    }

    fn with_renderer<T>(f: impl FnOnce(&Renderer) -> T) -> Result<T, String> {
        RENDERER.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_none() {
                let platform = HeadlessPlatform::new();
                slint::platform::set_platform(Box::new(SharedPlatform(platform.clone())))
                    .map_err(|e| format!("cannot install headless platform: {e}"))?;
                *slot = Some(Renderer { platform });
            }
            Ok(f(slot.as_ref().expect("renderer initialised above")))
        })
    }

    /// Install the headless platform. Call this before creating components if
    /// the caller wants to own construction (the render helpers do it lazily
    /// otherwise). Subsequent calls are no-ops.
    pub fn init() -> Result<(), String> {
        with_renderer(|_| ()).map(|_| ())
    }

    /// What a single frame render produced.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Frame {
        pub width: u32,
        pub height: u32,
    }

    /// Render one component instance to an RGB buffer at the requested size.
    ///
    /// The component is shown, laid out at `width` x `height` logical pixels
    /// (scale factor 1.0, which is what the design screenshots use) and drawn
    /// once. Animations are advanced a full tick first so entrance animations
    /// do not leave the frame at opacity 0.
    pub fn render_rgb(
        ui: &impl ComponentHandle,
        width: u32,
        height: u32,
    ) -> Result<Vec<Rgb8Pixel>, String> {
        let buffer = with_renderer(|renderer| {
            ui.show().map_err(|e| format!("show failed: {e}"))?;
            slint::platform::update_timers_and_animations();
            let window = renderer
                .platform
                .last_window
                .borrow()
                .clone()
                .ok_or_else(|| "the component did not create a window".to_string())?;
            window.set_size(PhysicalSize::new(width, height));
            slint::platform::update_timers_and_animations();

            let mut buffer = vec![Rgb8Pixel::new(0, 0, 0); (width * height) as usize];
            window.draw_if_needed(|software_renderer| {
                software_renderer.render(&mut buffer, width as usize);
            });
            Ok::<_, String>(buffer)
        })??;
        Ok(buffer)
    }

    /// One painted rectangle of a frame, in physical pixels.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DirtyRect {
        pub x: i32,
        pub y: i32,
        pub width: i32,
        pub height: i32,
    }

    impl DirtyRect {
        pub fn intersects(&self, other: &DirtyRect) -> bool {
            self.x < other.x + other.width
                && other.x < self.x + self.width
                && self.y < other.y + other.height
                && other.y < self.y + self.height
        }
    }

    /// Paint the component into a caller-owned premultiplied ARGB buffer.
    ///
    /// Returns the rectangles that were painted, or `None` when Slint had
    /// nothing to redraw since the last call, so a host can coalesce its
    /// commits to the frames Slint actually repainted. Pixels Slint does not
    /// paint stay transparent (`alpha = 0`), which is what lets a layer-shell
    /// host crop one canvas into several surfaces.
    ///
    /// The component must have a transparent background (the shell's
    /// `layer-host` mode does that) for the untouched pixels to stay clear.
    pub fn render_argb_into(
        ui: &impl ComponentHandle,
        buffer: &mut [PremultipliedRgbaColor],
        width: u32,
        height: u32,
    ) -> Result<Option<Vec<DirtyRect>>, String> {
        let len = (width as usize) * (height as usize);
        if buffer.len() < len {
            return Err(format!(
                "ARGB buffer is {} pixels, the frame needs {len}",
                buffer.len()
            ));
        }
        with_renderer(|renderer| {
            ui.show().map_err(|e| format!("show failed: {e}"))?;
            slint::platform::update_timers_and_animations();
            let window = renderer
                .platform
                .last_window
                .borrow()
                .clone()
                .ok_or_else(|| "the component did not create a window".to_string())?;
            if window.size() != PhysicalSize::new(width, height) {
                window.set_size(PhysicalSize::new(width, height));
                slint::platform::update_timers_and_animations();
            }
            let mut painted: Option<Vec<DirtyRect>> = None;
            let drawn = window.draw_if_needed(|software_renderer| {
                let region = software_renderer.render(&mut buffer[..len], width as usize);
                painted = Some(
                    region
                        .iter()
                        .map(|(position, size)| DirtyRect {
                            x: position.x as i32,
                            y: position.y as i32,
                            width: size.width as i32,
                            height: size.height as i32,
                        })
                        .collect(),
                );
            });
            Ok::<_, String>(if drawn { painted } else { None })
        })?
    }

    /// Render a component and write it as a PNG.
    pub fn render_png(
        ui: &impl ComponentHandle,
        width: u32,
        height: u32,
        path: impl AsRef<Path>,
    ) -> Result<Frame, String> {
        let pixels = render_rgb(ui, width, height)?;
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        let file = std::fs::File::create(path)
            .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("png header: {e}"))?;
        let raw: Vec<u8> = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        writer
            .write_image_data(&raw)
            .map_err(|e| format!("png data: {e}"))?;
        Ok(Frame { width, height })
    }

    /// Average luminance (0..=1) of a rendered frame: a cheap sanity check for
    /// "the frame is not blank".
    pub fn mean_luminance(pixels: &[Rgb8Pixel]) -> f64 {
        if pixels.is_empty() {
            return 0.0;
        }
        let total: u64 = pixels
            .iter()
            .map(|p| (p.r as u64 * 299 + p.g as u64 * 587 + p.b as u64 * 114) / 1000)
            .sum();
        total as f64 / pixels.len() as f64 / 255.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn design_system_types_are_exported() {
        // Compiling these bounds proves the .slint surface produced the types
        // the shell and the apps consume. Token *values* are rendered by
        // `examples/gallery.rs` because instantiating a component needs a
        // platform, which a unit-test binary must not install globally.
        assert_copy::<GlassTier>();
        assert_copy::<IconKind>();
    }

    fn assert_copy<T: Copy>() {}
}
