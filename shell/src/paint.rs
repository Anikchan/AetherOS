//! Paints the Slint scene into layer-surface buffers (P3-T02 → P4).
//!
//! The shell draws one composed scene (`ShellRoot` with `layer-host: true`):
//! no wallpaper, no desktop preview, a transparent background, and every panel
//! sitting exactly where the compositor will place its layer surface. The host
//! renders that scene once per frame into a premultiplied ARGB canvas and copies
//! each surface's rectangle out of it, so one Slint tree feeds five
//! `wl_surface`s without duplicating the UI or the controller wiring.
//!
//! Rendering only happens when Slint is dirty ([`LayerPainter::paint`] returns
//! `None` otherwise) and the host commits only the surfaces whose canvas
//! rectangle intersects the region Slint actually repainted, which is what keeps
//! the loop off the CPU and the commits to one per surface per frame.

use slint::platform::software_renderer::PremultipliedRgbaColor;

use aether_ui::render;

use crate::host::HostRequest;
use crate::ShellRoot;

/// The composed scene plus the last frame it produced.
pub struct LayerPainter {
    /// Output logical size the canvas is laid out for.
    output: (i32, i32),
    /// Canvas size in logical pixels (the output size).
    size: (u32, u32),
    /// Premultiplied ARGB canvas, one pixel per canvas pixel.
    canvas: Vec<PremultipliedRgbaColor>,
    /// Rectangles Slint repainted in the last frame (physical pixels).
    dirty: Vec<render::DirtyRect>,
    /// Frames painted since start, for the host log.
    frames: u64,
    /// Nanoseconds spent inside Slint for the last frame.
    last_paint_ns: u64,
}

impl LayerPainter {
    /// Prepare the shell window for the layer host and paint the first frame.
    ///
    /// The Slint platform must already be installed (`aether_ui::render::init`,
    /// which `--connect` calls). `scale` is the buffer scale of the output the
    /// surfaces land on; the canvas is logical, so a scaled output upscales when
    /// the pixels are copied into the surface buffer.
    pub fn new(ui: &ShellRoot, output: (i32, i32), scale: i32) -> Result<LayerPainter, String> {
        if output.0 <= 0 || output.1 <= 0 {
            return Err(format!(
                "the compositor reported no output size ({}x{})",
                output.0, output.1
            ));
        }
        let _ = scale;
        ui.set_layer_host(true);
        let mut painter = LayerPainter {
            output,
            size: (output.0 as u32, output.1 as u32),
            canvas: vec![
                PremultipliedRgbaColor {
                    red: 0,
                    green: 0,
                    blue: 0,
                    alpha: 0,
                };
                (output.0 as usize) * (output.1 as usize)
            ],
            dirty: Vec::new(),
            frames: 0,
            last_paint_ns: 0,
        };
        painter.paint(ui)?;
        Ok(painter)
    }

    /// Advance timers and animations, then repaint only if Slint is dirty.
    ///
    /// Returns `true` when a new frame was painted. `motion-*` durations and
    /// `reduce-motion` reach Slint through the tokens the controller syncs, so
    /// the animations themselves respect the accessibility switch; the host
    /// only chooses how often it ticks.
    pub fn paint(&mut self, ui: &ShellRoot) -> Result<bool, String> {
        let started = std::time::Instant::now();
        let painted = render::render_argb_into(ui, &mut self.canvas, self.size.0, self.size.1)?;
        match painted {
            Some(rects) => {
                self.dirty = rects;
                self.frames += 1;
                self.last_paint_ns = started.elapsed().as_nanos() as u64;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn output(&self) -> (i32, i32) {
        self.output
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// The last painted canvas (premultiplied ARGB), for tests and evidence.
    pub fn pixels(&self) -> &[PremultipliedRgbaColor] {
        &self.canvas
    }

    /// Canvas size in logical pixels.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Milliseconds the last Slint paint took.
    pub fn last_paint_ms(&self) -> f64 {
        self.last_paint_ns as f64 / 1_000_000.0
    }

    /// Whether the last frame touched any part of `request`'s surface.
    pub fn touched(&self, request: &HostRequest) -> bool {
        let (x, y) = request.placement(self.output);
        let (width, height) = request.negotiate(self.output);
        let rect = render::DirtyRect {
            x,
            y,
            width,
            height,
        };
        self.dirty.iter().any(|dirty| dirty.intersects(&rect))
    }

    /// Copy one surface's rectangle out of the canvas into `out` as `Argb8888`
    /// bytes (`B, G, R, A` little-endian, premultiplied), scaled by `scale`.
    ///
    /// The scene is laid out in logical pixels, so `scale > 1` upscales with
    /// nearest-neighbour sampling: correct placement and full coverage, but text
    /// stays logical-resolution. Crisp HiDPI needs a renderer that lays out at
    /// the device scale, which the software renderer's window adapter cannot be
    /// told to do (see the D6).
    pub fn write_surface(
        &self,
        request: &HostRequest,
        scale: i32,
        out: &mut Vec<u8>,
    ) -> Result<(u32, u32), String> {
        let scale = scale.max(1) as usize;
        let (x, y) = request.placement(self.output);
        let (width, height) = request.negotiate(self.output);
        let buffer_width = (width as usize) * scale;
        let buffer_height = (height as usize) * scale;
        let stride = buffer_width * 4;
        out.clear();
        out.resize(stride * buffer_height, 0);

        let canvas_width = self.size.0 as usize;
        let canvas_height = self.size.1 as usize;
        for row in 0..buffer_height {
            let source_y = y + (row / scale) as i32;
            if source_y < 0 || source_y as usize >= canvas_height {
                continue; // out of the canvas: transparent
            }
            let canvas_row = (source_y as usize) * canvas_width;
            let dest = &mut out[row * stride..(row + 1) * stride];
            if scale == 1 {
                let start = x;
                let end = x + buffer_width as i32;
                for column in start.max(0)..end.min(canvas_width as i32) {
                    let pixel = self.canvas[canvas_row + column as usize];
                    let offset = (column - start) as usize * 4;
                    dest[offset] = pixel.blue;
                    dest[offset + 1] = pixel.green;
                    dest[offset + 2] = pixel.red;
                    dest[offset + 3] = pixel.alpha;
                }
            } else {
                for column in 0..buffer_width {
                    let source_x = x + (column / scale) as i32;
                    if source_x < 0 || source_x as usize >= canvas_width {
                        continue;
                    }
                    let pixel = self.canvas[canvas_row + source_x as usize];
                    let offset = column * 4;
                    dest[offset] = pixel.blue;
                    dest[offset + 1] = pixel.green;
                    dest[offset + 2] = pixel.red;
                    dest[offset + 3] = pixel.alpha;
                }
            }
        }
        Ok((buffer_width as u32, buffer_height as u32))
    }

    /// The design wallpaper at the canvas size (× `scale`), as premultiplied
    /// ARGB bytes (`B, G, R, A` little-endian).
    ///
    /// [`Wallpaper`] in `shell/ui/surfaces.slint` is a 155° three-stop gradient
    /// (#0B1220 → #0E1116 → #101725) with two soft glows; the background layer
    /// surface needs it without the desktop preview (and without painting the
    /// wallpaper behind every panel, which would cover the windows under a
    /// floating dock). Painting it here keeps the chrome surfaces transparent
    /// outside their panels and gives the background surface one opaque frame.
    ///
    /// [`Wallpaper`]: ../ui/surfaces.slint
    pub fn write_wallpaper(&self, scale: i32, out: &mut Vec<u8>) -> (u32, u32) {
        let scale = scale.max(1);
        let width = self.size.0 * scale as u32;
        let height = self.size.1 * scale as u32;
        out.clear();
        out.resize((width as usize) * (height as usize) * 4, 0xff);

        // Row-invariant parts of the gradient: `t` is a linear function of the
        // pixel position, so the per-pixel work is two multiply-adds plus the
        // two glows (whose falloff needs no square root).
        let (w, h) = (width.max(1) as f32, height.max(1) as f32);
        let angle = 155f32.to_radians();
        let (dx, dy) = (angle.sin(), -angle.cos());
        let extent = dx.abs() * 0.5 + dy.abs() * 0.5;
        // `px`/`py` are already normalised to 0..1, so the axis projection
        // needs no size factor (dividing by the size made the gradient flat).
        let slope_x = dx / (2.0 * extent);
        let slope_y = dy / (2.0 * extent);
        let (rx1, ry1) = (0.23, 0.29);
        let (rx2, ry2) = (0.19, 0.24);
        let inv_w = 1.0 / w;
        let inv_h = 1.0 / h;

        for y in 0..height {
            let py = y as f32 * inv_h;
            let base_y = (py - 0.5) * slope_y + 0.5;
            let row = (y as usize) * (width as usize);
            for x in 0..width {
                let px = x as f32 * inv_w;
                let t = (base_y + (px - 0.5) * slope_x).clamp(0.0, 1.0);
                let mut colour = wallpaper_gradient(t);
                colour = glow(
                    colour,
                    ((px - 0.31) / rx1, (py - 0.41) / ry1),
                    (0x1C, 0x3D, 0x7A),
                    0.33,
                );
                colour = glow(
                    colour,
                    ((px - 0.77) / rx2, (py - 0.66) / ry2),
                    (0x6C, 0x4C, 0xE6),
                    0.20,
                );
                let offset = (row + x as usize) * 4;
                // Premultiplied and fully opaque, so the channel order is just
                // B, G, R with A = 255.
                out[offset] = colour.2;
                out[offset + 1] = colour.1;
                out[offset + 2] = colour.0;
                out[offset + 3] = 0xff;
            }
        }
        (width, height)
    }

    /// The composed canvas as `Rgbx8888` bytes (alpha forced opaque), for the
    /// evidence dump the integration test reads.
    pub fn canvas_rgbx(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.canvas.len() * 4);
        for pixel in &self.canvas {
            out.push(pixel.blue);
            out.push(pixel.green);
            out.push(pixel.red);
            out.push(0xff);
        }
        out
    }
}

/// One wallpaper pixel (RGB) at `(x, y)` of a `width × height` frame.
///
/// Mirrors `Wallpaper` in `shell/ui/surfaces.slint`: the 155° base gradient plus
/// its two glows, blended in the same order.
pub fn wallpaper_pixel(x: u32, y: u32, width: u32, height: u32) -> (u8, u8, u8) {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let (px, py) = (x as f32 / w, y as f32 / h);
    let angle = 155f32.to_radians();
    let (dx, dy) = (angle.sin(), -angle.cos());
    let extent = dx.abs() * 0.5 + dy.abs() * 0.5;
    let t = (((px - 0.5) * dx + (py - 0.5) * dy) / (2.0 * extent) + 0.5).clamp(0.0, 1.0);
    let colour = wallpaper_gradient(t);
    let colour = glow(
        colour,
        ((px - 0.31) / 0.23, (py - 0.41) / 0.29),
        (0x1C, 0x3D, 0x7A),
        0.33,
    );
    glow(
        colour,
        ((px - 0.77) / 0.19, (py - 0.66) / 0.24),
        (0x6C, 0x4C, 0xE6),
        0.20,
    )
}

/// The 155° three-stop gradient of `Wallpaper` at position `t` (0..1).
fn wallpaper_gradient(t: f32) -> (u8, u8, u8) {
    // #0B1220 0 %, #0E1116 45 %, #101725 100 %.
    const STOPS: [(f32, (u8, u8, u8)); 3] = [
        (0.0, (0x0B, 0x12, 0x20)),
        (0.45, (0x0E, 0x11, 0x16)),
        (1.0, (0x10, 0x17, 0x25)),
    ];
    if t <= STOPS[0].0 {
        return STOPS[0].1;
    }
    for pair in STOPS.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        if t <= to.0 {
            let local = (t - from.0) / (to.0 - from.0).max(f32::EPSILON);
            return mix(from.1, to.1, local);
        }
    }
    STOPS[2].1
}

fn mix(from: (u8, u8, u8), to: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let blend = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    (
        blend(from.0, to.0),
        blend(from.1, to.1),
        blend(from.2, to.2),
    )
}

/// An elliptical tint with `normalised` offsets from its centre, fading to
/// nothing at the unit circle (the `.slint` glows are blurred rectangles).
/// Squared-distance falloff: smooth, and no square root per pixel.
fn glow(
    base: (u8, u8, u8),
    normalised: (f32, f32),
    tint: (u8, u8, u8),
    strength: f32,
) -> (u8, u8, u8) {
    let d2 = normalised.0 * normalised.0 + normalised.1 * normalised.1;
    if d2 >= 1.0 {
        return base;
    }
    let fade = 1.0 - d2;
    mix(base, tint, strength * fade * fade)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::plan;

    #[test]
    fn the_wallpaper_is_opaque_and_is_not_the_panel_tint() {
        // The `.slint` `Wallpaper` and the chrome tint are different surfaces;
        // the background must never be the palette fallback (all zero) or the
        // menu-bar glass tint (#0C0E12 premultiplied).
        let (w, h) = (1920u32, 1080u32);
        let mut min = (255u8, 255u8, 255u8);
        let mut max = (0u8, 0u8, 0u8);
        for y in 0..h {
            for x in 0..w {
                let pixel = wallpaper_pixel(x, y, w, h);
                min = (min.0.min(pixel.0), min.1.min(pixel.1), min.2.min(pixel.2));
                max = (max.0.max(pixel.0), max.1.max(pixel.1), max.2.max(pixel.2));
                assert!(
                    pixel != (12, 14, 18),
                    "the wallpaper is not the menu-bar tint at {x},{y}"
                );
            }
        }
        assert!(min != max, "the wallpaper is a gradient, not a flat fill");
        assert!(
            min.0 < 0x20 && max.2 > 0x30,
            "it spans the design's dark blues"
        );
        // Top-left and bottom-right differ along the 155° axis.
        assert_ne!(
            wallpaper_pixel(0, 0, w, h),
            wallpaper_pixel(w - 1, h - 1, w, h)
        );
    }

    #[test]
    fn surfaces_are_placed_where_the_anchors_say() {
        let output = (1920, 1080);
        let plan = plan();
        let find = |name: &str| plan.iter().find(|request| request.name == name).unwrap();
        assert_eq!(find("background").placement(output), (0, 0));
        assert_eq!(find("background").negotiate(output), (1920, 1080));
        assert_eq!(find("menubar").placement(output), (0, 0));
        assert_eq!(find("dock").placement(output), (600, 1016));
        assert_eq!(find("launcher").placement(output), (640, 330));
        assert_eq!(find("notifications").placement(output), (1560, 28));
        assert_eq!(find("control-center").placement(output), (1580, 28));
    }
}
