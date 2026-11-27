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

#[cfg(test)]
mod tests {
    use crate::host::plan;

    #[test]
    fn surfaces_are_placed_where_the_anchors_say() {
        let output = (1920, 1080);
        let plan = plan();
        let find = |name: &str| plan.iter().find(|request| request.name == name).unwrap();
        assert_eq!(find("menubar").placement(output), (0, 0));
        assert_eq!(find("dock").placement(output), (600, 1016));
        assert_eq!(find("launcher").placement(output), (640, 330));
        assert_eq!(find("notifications").placement(output), (1560, 28));
        assert_eq!(find("control-center").placement(output), (1580, 28));
    }
}
