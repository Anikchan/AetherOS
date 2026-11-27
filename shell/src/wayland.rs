//! The real layer-shell host: a Wayland client that creates the shell surfaces
//! and paints the Slint scene into them (P3-T02 → P4).
//!
//! [`crate::host`] is the contract (what each surface asks for, and how the
//! compositor's markers are read back). [`crate::layers`] is the table.
//! [`crate::paint`] renders the composed Slint scene. This module is the
//! socket:
//!
//! 1. connect to `$WAYLAND_DISPLAY`, bind `wl_compositor`, `wl_shm`,
//!    `zwlr_layer_shell_v1`, every `wl_output` (for its scale) and `wl_seat`
//!    when the compositor has one;
//! 2. create every planned surface in plan order with its layer, anchors, size,
//!    exclusive zone, top margin and keyboard mode, commit once so the
//!    compositor sends its configure, and ack every configure;
//! 3. paint the Slint scene at the output's logical size, copy each surface's
//!    rectangle into that surface's `wl_shm` buffer at the output's buffer
//!    scale, damage it and commit — only for the surfaces Slint actually
//!    repainted, at most one commit per surface per frame;
//! 4. unmap the surfaces whose panel is closed (launcher, notifications,
//!    control centre) instead of leaving stale pixels mapped;
//! 5. feed `wl_keyboard` presses to `crate::keys` and repaint when the shell
//!    state changed.
//!
//! `aether-shell --connect` runs it; `shell/tests/compositor_handshake.rs`
//! boots the real compositor headless and asserts the result end to end.

use std::fs;
use std::os::fd::AsFd;
use std::os::unix::fs::FileExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use slint::platform::software_renderer::PremultipliedRgbaColor;

use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_keyboard, wl_output, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

use crate::app::ShellController;
use crate::host::{self, HostRequest, KEYBOARD_EXCLUSIVE, KEYBOARD_ON_DEMAND};
use crate::keys::{self, KeyInput};
use crate::model::Overlay;
use crate::paint::LayerPainter;

/// A committed SHM buffer, its file and pool.
struct ShmBuffer {
    file: fs::File,
    _pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
    size: (u32, u32),
}

impl ShmBuffer {
    /// Overwrite the pixels of the mapped shared memory.
    fn write(&self, bytes: &[u8]) -> Result<(), String> {
        self.file
            .write_all_at(bytes, 0)
            .map_err(|error| format!("cannot write the SHM buffer: {error}"))
    }
}

/// One layer surface.
struct Slot {
    request: HostRequest,
    wl_surface: wl_surface::WlSurface,
    layer_surface: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    /// `(serial, width, height)` of every configure, in arrival order.
    configures: Vec<(u32, i32, i32)>,
    acked: usize,
    /// Latest configure that carried a real size.
    size: Option<(i32, i32)>,
    /// Buffer scale of the output this surface entered (1 when unknown).
    scale: i32,
    buffer: Option<ShmBuffer>,
    mapped: bool,
    /// Frames this surface was painted in.
    paints: u64,
    /// Milliseconds of the last paint of this surface.
    last_paint_ms: f64,
}

impl Slot {
    fn buffer_size(&self) -> (u32, u32) {
        self.buffer
            .as_ref()
            .map(|buffer| buffer.size)
            .unwrap_or((0, 0))
    }
}

/// One `wl_output`, for its scale.
struct OutputSlot {
    proxy: wl_output::WlOutput,
    name: String,
    scale: i32,
}

/// Client-side protocol state.
#[derive(Default)]
struct HostState {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    /// `(interface, registry name, advertised version)` of every global.
    globals: Vec<(String, u32, u32)>,
    outputs: Vec<OutputSlot>,
    slots: Vec<Slot>,
    /// Wire keycodes and the modifier mask in force, as `wl_keyboard` sends them.
    keys: Vec<(u32, u32)>,
    modifiers: u32,
    keyboard: bool,
}

/// What the host created for one surface.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceReport {
    pub name: &'static str,
    pub namespace: &'static str,
    pub layer_name: &'static str,
    pub anchors: u32,
    pub exclusive_zone: i32,
    pub margin_top: i32,
    pub keyboard_interactivity: u32,
    /// Configured logical size.
    pub size: Option<(i32, i32)>,
    /// Compositor placement on the output (logical pixels).
    pub placement: (i32, i32),
    pub scale: i32,
    pub configures: usize,
    /// Physical size of the buffer attached (0×0 when never painted).
    pub buffer: (u32, u32),
    pub mapped: bool,
    pub paints: u64,
    pub last_paint_ms: f64,
}

impl SurfaceReport {
    fn problem(&self) -> Option<String> {
        if self.size.is_none() {
            return Some(format!(
                "{} was created but never configured (no sized zwlr_layer_surface_v1.configure)",
                self.name
            ));
        }
        if self.mapped && self.buffer == (0, 0) {
            return Some(format!("{} is mapped without a buffer", self.name));
        }
        None
    }
}

/// The result of one `--connect` run.
#[derive(Debug, Clone, PartialEq)]
pub struct HostReport {
    pub display: String,
    pub outputs: Vec<String>,
    pub output: (i32, i32),
    pub scale: i32,
    pub keyboard: bool,
    pub surfaces: Vec<SurfaceReport>,
}

impl HostReport {
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.outputs.is_empty() {
            problems.push("the compositor advertises no wl_output".to_string());
        }
        if self.output.0 <= 0 || self.output.1 <= 0 {
            problems.push("the shell could not determine the output size".to_string());
        }
        problems.extend(self.surfaces.iter().filter_map(SurfaceReport::problem));
        problems
    }

    pub fn is_ok(&self) -> bool {
        !self.surfaces.is_empty() && self.problems().is_empty()
    }

    /// One marker per surface plus the verdict, for `--connect` and the harness.
    pub fn summary(&self) -> String {
        let mut out = format!(
            "AETHER_SHELL_LAYER_HOST: display={} outputs={} output={}x{} scale={} seat={}\n",
            self.display,
            self.outputs.len(),
            self.output.0,
            self.output.1,
            self.scale,
            if self.keyboard { "keyboard" } else { "none" }
        );
        for surface in &self.surfaces {
            out.push_str(&format!(
                "AETHER_SHELL_LAYER_CREATED: name={} namespace={} layer={} anchors={} size={}x{} exclusive_zone={} margin_top={} keyboard={}\n",
                surface.name,
                surface.namespace,
                surface.layer_name,
                surface.anchors,
                surface.size.map(|size| size.0).unwrap_or(0),
                surface.size.map(|size| size.1).unwrap_or(0),
                surface.exclusive_zone,
                surface.margin_top,
                surface.keyboard_interactivity
            ));
            out.push_str(&format!(
                "AETHER_SHELL_LAYER_CONFIGURE: name={} namespace={} configured={} serials={} placement={},{} scale={}\n",
                surface.name,
                surface.namespace,
                surface
                    .size
                    .map(|size| format!("{}x{}", size.0, size.1))
                    .unwrap_or_else(|| "none".to_string()),
                surface.configures,
                surface.placement.0,
                surface.placement.1,
                surface.scale
            ));
            out.push_str(&format!(
                "AETHER_SHELL_LAYER_PAINT: name={} mapped={} buffer={}x{} paints={} last_ms={:.3}\n",
                surface.name,
                surface.mapped,
                surface.buffer.0,
                surface.buffer.1,
                surface.paints,
                surface.last_paint_ms
            ));
        }
        let configured = self
            .surfaces
            .iter()
            .filter(|surface| surface.size.is_some())
            .count();
        let mapped = self
            .surfaces
            .iter()
            .filter(|surface| surface.mapped)
            .count();
        let painted = self
            .surfaces
            .iter()
            .filter(|surface| surface.buffer != (0, 0))
            .count();
        out.push_str(&format!(
            "AETHER_SHELL_LAYER_HOST_READY: surfaces={} configured={} mapped={} painted={}\n",
            self.surfaces.len(),
            configured,
            mapped,
            painted
        ));
        let problems = self.problems();
        if problems.is_empty() {
            out.push_str("AETHER_SHELL_LAYER_HOST_OK\n");
        } else {
            for problem in &problems {
                out.push_str(&format!("AETHER_SHELL_LAYER_HOST_PROBLEM: {problem}\n"));
            }
            out.push_str("AETHER_SHELL_LAYER_HOST_FAILED\n");
        }
        out
    }
}

/// What one `run` did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunStats {
    /// Frames Slint repainted.
    pub paints: u64,
    /// Surface commits issued (attach + damage + commit).
    pub commits: u64,
    /// Surfaces unmapped because their panel closed.
    pub unmaps: u64,
    /// Key presses routed to the shell.
    pub keys: u64,
}

impl RunStats {
    pub fn summary_line(&self) -> String {
        format!(
            "AETHER_SHELL_LAYER_RUN: paints={} commits={} unmaps={} keys={}",
            self.paints, self.commits, self.unmaps, self.keys
        )
    }
}

/// A live connection to the compositor.
pub struct LayerHost {
    conn: Connection,
    queue: EventQueue<HostState>,
    state: HostState,
    runtime_dir: PathBuf,
    display: String,
    painter: Option<LayerPainter>,
    /// Output the surfaces live on: `(logical size, buffer scale)`.
    output: ((i32, i32), i32),
    /// Scratch buffer for one surface's pixels.
    scratch: Vec<u8>,
    /// Where per-surface PNG evidence is written.
    frames_out: Option<PathBuf>,
    /// Commits issued (attach + damage + commit) since connect.
    commits: u64,
    /// Surfaces unmapped because their panel closed.
    unmaps: u64,
}

impl LayerHost {
    /// Connect to `$WAYLAND_DISPLAY` inside `$XDG_RUNTIME_DIR` and bind the
    /// globals the shell needs. Errors are harness-readable strings.
    pub fn connect() -> Result<LayerHost, String> {
        let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .map_err(|_| {
                "XDG_RUNTIME_DIR is not set: no Wayland socket to connect to".to_string()
            })?;
        let conn = Connection::connect_to_env()
            .map_err(|error| format!("cannot connect to {display}: {error}"))?;
        let mut queue = conn.new_event_queue::<HostState>();
        let qh = queue.handle();
        let registry = conn.display().get_registry(&qh, ());
        let mut state = HostState::default();
        // Enumerate globals through our own registry: `wl_output` and `wl_seat`
        // are multi-instance globals, so each one is bound individually (the
        // shortcut `GlobalList::bind` only binds the first and cannot follow
        // hotplug).
        queue
            .roundtrip(&mut state)
            .map_err(|error| format!("wl_registry failed: {error:?}"))?;
        let version_of = |interface: &str| {
            state
                .globals
                .iter()
                .find(|(name, _, _)| name == interface)
                .map(|(_, _, version)| *version)
        };
        let compositor_version = version_of("wl_compositor")
            .ok_or("the compositor has no wl_compositor")?
            .min(6);
        state.compositor = Some(registry.bind::<wl_compositor::WlCompositor, _, _>(
            global_name(&state, "wl_compositor"),
            compositor_version,
            &qh,
            (),
        ));
        let shm_version = version_of("wl_shm")
            .ok_or("the compositor has no wl_shm")?
            .min(1);
        state.shm = Some(registry.bind::<wl_shm::WlShm, _, _>(
            global_name(&state, "wl_shm"),
            shm_version,
            &qh,
            (),
        ));
        let layer_version = version_of("zwlr_layer_shell_v1")
            .ok_or("the compositor advertises no zwlr_layer_shell_v1")?
            .min(4);
        state.layer_shell = Some(
            registry.bind::<zwlr_layer_shell_v1::ZwlrLayerShellV1, _, _>(
                global_name(&state, "zwlr_layer_shell_v1"),
                layer_version,
                &qh,
                (),
            ),
        );
        // The DRM backend creates a seat; the headless backend has no input
        // devices, so the keyboard path is reported as absent instead of
        // failing the handshake.
        if let Some((_, _, version)) = state.globals.iter().find(|(name, _, _)| name == "wl_seat") {
            let version = (*version).min(7);
            let seat_name = global_name(&state, "wl_seat");
            let seat = registry.bind::<wl_seat::WlSeat, _, _>(seat_name, version, &qh, ());
            seat.get_keyboard(&qh, ());
            state.keyboard = true;
        }
        // One more roundtrip so the outputs' name/scale events and the seat
        // capabilities are in before the first surface is created.
        queue
            .roundtrip(&mut state)
            .map_err(|error| format!("initial roundtrip failed: {error:?}"))?;

        Ok(LayerHost {
            conn,
            queue,
            state,
            runtime_dir,
            display,
            painter: None,
            output: ((0, 0), 1),
            scratch: Vec::new(),
            frames_out: None,
            commits: 0,
            unmaps: 0,
        })
    }

    /// Write one PNG per surface after every painted frame (evidence for the
    /// integration test and the on-device harness).
    pub fn set_frames_out(&mut self, dir: Option<PathBuf>) {
        self.frames_out = dir;
    }

    /// Create every planned layer surface in plan order and learn its
    /// configure. `text_scale` is the accessibility scale the panel sizes and
    /// the menu-bar margin follow.
    pub fn create_surfaces(&mut self, text_scale: f32) -> Result<(), String> {
        let compositor = self.state.compositor.clone().expect("bound in connect");
        let layer_shell = self.state.layer_shell.clone().expect("bound in connect");
        let qh = self.queue.handle();

        for (index, request) in host::plan_at(text_scale).into_iter().enumerate() {
            let wl_surface = compositor.create_surface(&qh, index);
            let layer_surface = layer_shell.get_layer_surface(
                &wl_surface,
                None,
                match request.layer {
                    0 => zwlr_layer_shell_v1::Layer::Background,
                    1 => zwlr_layer_shell_v1::Layer::Bottom,
                    2 => zwlr_layer_shell_v1::Layer::Top,
                    _ => zwlr_layer_shell_v1::Layer::Overlay,
                },
                request.namespace.to_string(),
                &qh,
                index,
            );
            layer_surface.set_anchor(zwlr_layer_surface_v1::Anchor::from_bits_truncate(
                request.anchors,
            ));
            layer_surface.set_size(request.width.max(0) as u32, request.height.max(0) as u32);
            layer_surface.set_margin(request.margin_top.max(0), 0, 0, 0);
            layer_surface.set_exclusive_zone(request.exclusive_zone);
            layer_surface.set_keyboard_interactivity(match request.keyboard_interactivity {
                KEYBOARD_EXCLUSIVE => zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive,
                KEYBOARD_ON_DEMAND => zwlr_layer_surface_v1::KeyboardInteractivity::OnDemand,
                _ => zwlr_layer_surface_v1::KeyboardInteractivity::None,
            });
            // The output is resolved by the compositor; the scale follows
            // `wl_surface.enter` and is applied before the first attach.
            wl_surface.set_buffer_scale(1);
            wl_surface.commit();
            self.state.slots.push(Slot {
                request,
                wl_surface,
                layer_surface,
                configures: Vec::new(),
                acked: 0,
                size: None,
                scale: 1,
                buffer: None,
                mapped: false,
                paints: 0,
                last_paint_ms: 0.0,
            });
        }

        // The first commit makes the compositor send a sized configure after
        // the initial unsized one from `new_layer_surface`.
        self.roundtrip()?;
        self.roundtrip()?;
        self.ack_configures();

        let missing = self
            .state
            .slots
            .iter()
            .filter(|slot| slot.size.is_none())
            .map(|slot| slot.request.name)
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(format!(
                "the compositor never configured: {}",
                missing.join(", ")
            ));
        }
        self.output = self.resolve_output();
        Ok(())
    }

    /// Output rect and buffer scale the surfaces live on.
    ///
    /// The menu bar is anchored left+right, so its configured width is the
    /// output's logical width; the right-hand panels are anchored top+bottom
    /// below the menu bar margin, so their configured height plus that margin is
    /// the output's logical height. The scale comes from `wl_output.scale` via
    /// the output each surface entered.
    fn resolve_output(&self) -> ((i32, i32), i32) {
        let mut width = 0;
        let mut height = 0;
        let mut scale = 1;
        for slot in &self.state.slots {
            let Some(size) = slot.size else { continue };
            match slot.request.name {
                "menubar" => {
                    width = size.0;
                    scale = slot.scale.max(1);
                }
                "notifications" | "control-center" => {
                    height = height.max(size.1 + slot.request.margin_top);
                }
                _ => {}
            }
        }
        if width == 0 || height == 0 {
            if let Some(slot) = self.state.slots.first() {
                let size = slot
                    .size
                    .unwrap_or((slot.request.width, slot.request.height));
                return ((size.0.max(1), size.1.max(1)), slot.scale.max(1));
            }
        }
        ((width.max(1), height.max(1)), scale.max(1))
    }

    /// Paint and commit one frame. Returns `false` when nothing changed.
    pub fn present(&mut self, controller: &ShellController) -> Result<bool, String> {
        let (output, scale) = self.output;
        let first = self.painter.is_none();
        if first {
            let painter = LayerPainter::new(controller.ui(), output, scale)?;
            self.painter = Some(painter);
        }
        self.ack_configures();
        // Repaint only when the controller touched the Slint tree (property
        // change, key, timer, animation tick) — the same switch for settings,
        // focus, hover and input.
        let painted = {
            let painter = self.painter.as_mut().ok_or("the painter was not created")?;
            painter.paint(controller.ui())?
        };
        let mut changed = painted || first;
        let mut work: Vec<(usize, (u32, u32), bool)> = Vec::new();
        {
            let painter = self.painter.as_ref().ok_or("the painter was not created")?;
            let overlay = controller.state().overlay;
            for (index, slot) in self.state.slots.iter().enumerate() {
                let Some(size) = slot.size else { continue };
                let slot_scale = slot.scale.max(1);
                let want = ((size.0 * slot_scale) as u32, (size.1 * slot_scale) as u32);
                if !panel_open(slot.request.name, overlay) {
                    if slot.mapped {
                        work.push((index, want, true));
                    }
                    continue;
                }
                if slot.mapped && slot.buffer_size() == want && !first {
                    // Nothing painted this frame and the surface is already
                    // correct: do not touch it (one commit per frame maximum).
                    if !painted || !painter.touched(&slot.request) {
                        continue;
                    }
                }
                work.push((index, want, false));
            }
        }
        for (index, want, unmap) in work {
            if unmap {
                let slot = &mut self.state.slots[index];
                slot.wl_surface.attach(None, 0, 0);
                slot.wl_surface.commit();
                slot.mapped = false;
                self.unmaps += 1;
                changed = true;
            } else {
                if self.paint_slot(index, want)? {
                    changed = true;
                }
            }
        }
        self.conn.flush().ok();
        Ok(changed)
    }

    /// Render one surface's rectangle into its SHM buffer and commit it.
    fn paint_slot(&mut self, index: usize, want: (u32, u32)) -> Result<bool, String> {
        let shm = self.state.shm.clone().ok_or("no wl_shm")?;
        let qh = self.queue.handle();
        let request = self.state.slots[index].request.clone();
        let scale = self.state.slots[index].scale.max(1);

        // (Re)create the buffer when its size changed (output or text scale).
        if self.state.slots[index].buffer_size() != want {
            let path = self.runtime_dir.join(format!(
                "aether-shell-shm-{}-{}x{}.raw",
                request.name, want.0, want.1
            ));
            let buffer = create_buffer(
                &shm,
                &qh,
                &path,
                want.0 as i32,
                want.1 as i32,
                PremultipliedRgbaColor::default(),
            )?;
            self.state.slots[index].buffer = Some(buffer);
        }

        let started = Instant::now();
        let mut scratch = std::mem::take(&mut self.scratch);
        let (width, height) = {
            let painter = self.painter.as_ref().ok_or("the painter was not created")?;
            painter.write_surface(&request, scale, &mut scratch)?
        };
        if let Some(buffer) = self.state.slots[index].buffer.as_ref() {
            buffer.write(&scratch)?;
        }
        let paint_ms = started.elapsed().as_secs_f64() * 1000.0;

        let slot = &mut self.state.slots[index];
        slot.wl_surface.set_buffer_scale(scale);
        if let Some(buffer) = slot.buffer.as_ref() {
            slot.wl_surface.attach(Some(&buffer.buffer), 0, 0);
            slot.wl_surface
                .damage_buffer(0, 0, width as i32, height as i32);
        }
        slot.wl_surface.commit();
        slot.mapped = true;
        slot.paints += 1;
        slot.last_paint_ms = paint_ms;
        self.commits += 1;

        if let Some(dir) = self.frames_out.clone() {
            dump_png(&dir, &slot.request.name, width, height, &scratch)?;
        }
        self.scratch = scratch;
        Ok(true)
    }

    /// Drive the shell for `duration`: dispatch events, route keys through
    /// `keys::dispatch`, repaint when the shell changed.
    pub fn run(
        &mut self,
        controller: &ShellController,
        duration: Duration,
        mut on_key: impl FnMut(&keys::KeyEvent),
    ) -> Result<RunStats, String> {
        let deadline = Instant::now() + duration;
        let mut stats = RunStats::default();
        // The menu-bar clock is a one-minute timer, exactly like the binary's
        // timer in the interactive host: it dirties the scene at most once a
        // minute, never once per frame.
        let mut last_tick = Instant::now();
        // First frame: map the panels that are open (menu bar and dock at
        // startup) and paint them.
        if self.present(controller)? {
            stats.paints += 1;
        }
        loop {
            self.queue
                .dispatch_pending(&mut self.state)
                .map_err(|error| format!("Wayland dispatch failed: {error:?}"))?;
            for input in self.take_keys() {
                let event = keys::dispatch(controller, input);
                stats.keys += 1;
                on_key(&event);
            }
            if self.present(controller)? {
                stats.paints += 1;
            }
            let now = Instant::now();
            if now.duration_since(last_tick) >= Duration::from_secs(1) {
                controller.tick(&chrono::Local::now());
                last_tick = now;
            }
            if now >= deadline {
                break;
            }
            let interval = frame_interval(controller);
            std::thread::sleep(interval.min(deadline.saturating_duration_since(now)));
        }
        let _ = self.roundtrip();
        stats.commits = self.commits;
        stats.unmaps = self.unmaps;
        Ok(stats)
    }

    /// Milliseconds the last Slint paint took (0 before the first frame).
    pub fn last_paint_ms(&self) -> f64 {
        self.painter
            .as_ref()
            .map(LayerPainter::last_paint_ms)
            .unwrap_or(0.0)
    }

    /// The current report.
    pub fn report(&self) -> HostReport {
        let (output, scale) = self.output;
        HostReport {
            display: self.display.clone(),
            outputs: self
                .state
                .outputs
                .iter()
                .map(|output| output.name.clone())
                .collect(),
            output,
            scale,
            keyboard: self.state.keyboard,
            surfaces: self
                .state
                .slots
                .iter()
                .map(|slot| {
                    let scale = slot.scale.max(1);
                    SurfaceReport {
                        name: slot.request.name,
                        namespace: slot.request.namespace,
                        layer_name: slot.request.layer_name,
                        anchors: slot.request.anchors,
                        exclusive_zone: slot.request.exclusive_zone,
                        margin_top: slot.request.margin_top,
                        keyboard_interactivity: slot.request.keyboard_interactivity,
                        size: slot.size,
                        placement: slot.request.placement(output),
                        scale,
                        configures: slot.configures.len(),
                        buffer: slot.buffer_size(),
                        mapped: slot.mapped,
                        paints: slot.paints,
                        last_paint_ms: slot.last_paint_ms,
                    }
                })
                .collect(),
        }
    }

    /// Ack every configure received so far (in arrival order).
    fn ack_configures(&mut self) {
        for index in 0..self.state.slots.len() {
            let pending =
                self.state.slots[index].configures[self.state.slots[index].acked..].to_vec();
            for (serial, width, height) in pending {
                let slot = &mut self.state.slots[index];
                slot.layer_surface.ack_configure(serial);
                slot.acked += 1;
                if width > 0 && height > 0 {
                    slot.size = Some((width, height));
                }
            }
        }
    }

    fn roundtrip(&mut self) -> Result<(), String> {
        self.queue
            .roundtrip(&mut self.state)
            .map(|_| ())
            .map_err(|error| format!("Wayland roundtrip failed: {error:?}"))
    }

    /// Key presses the compositor delivered, as evdev + modifier state.
    pub fn take_keys(&mut self) -> Vec<KeyInput> {
        std::mem::take(&mut self.state.keys)
            .into_iter()
            .map(|(key, modifiers)| KeyInput::from_wire(key, modifiers))
            .collect()
    }

    /// True when a Wayland socket exists for the current environment.
    pub fn has_socket() -> bool {
        let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") else {
            return false;
        };
        let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
        PathBuf::from(dir).join(display).exists()
    }
}

/// Whether a panel is on screen for the current overlay state.
pub fn panel_open(name: &str, overlay: Overlay) -> bool {
    match name {
        "menubar" | "dock" => true,
        "launcher" => overlay == Overlay::Launcher,
        "notifications" => overlay == Overlay::Notifications,
        "control-center" => overlay == Overlay::ControlCenter,
        // Settings, overview and the lock screen are full-screen overlays with
        // no layer surface yet; they are not painted by the layer host.
        _ => false,
    }
}

/// How often the host ticks the shell: one frame per 16 ms normally, and a
/// fifth of that rate when the user asked for reduced motion (animations are
/// collapsed, so there is nothing to interpolate at 60 Hz).
pub fn frame_interval(controller: &ShellController) -> Duration {
    if controller.a11y().reduce_motion {
        Duration::from_millis(100)
    } else {
        Duration::from_millis(16)
    }
}

/// Create and fill a buffer with one colour. A real file (not `memfd_create`)
/// keeps the host portable to the macOS dev host.
fn create_buffer(
    shm: &wl_shm::WlShm,
    qh: &QueueHandle<HostState>,
    path: &std::path::Path,
    width: i32,
    height: i32,
    colour: PremultipliedRgbaColor,
) -> Result<ShmBuffer, String> {
    use std::io::Write;

    let width = width.max(1) as u32;
    let height = height.max(1) as u32;
    let len = (width as usize) * (height as usize) * 4;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    let mut pixels = Vec::with_capacity(len);
    for _ in 0..(width as usize * height as usize) {
        pixels.extend_from_slice(&[colour.blue, colour.green, colour.red, colour.alpha]);
    }
    file.write_all(&pixels)
        .map_err(|error| format!("cannot fill {}: {error}", path.display()))?;
    file.flush()
        .map_err(|error| format!("cannot flush {}: {error}", path.display()))?;

    let pool = shm.create_pool(file.as_fd(), len as i32, qh, ());
    let buffer = pool.create_buffer(
        0,
        width as i32,
        height as i32,
        (width * 4) as i32,
        wl_shm::Format::Argb8888,
        qh,
        (),
    );
    Ok(ShmBuffer {
        file,
        _pool: pool,
        buffer,
        size: (width, height),
    })
}

/// Write one surface's ARGB pixels as a PNG (`layer-<name>.png`).
fn dump_png(
    dir: &std::path::Path,
    name: &str,
    width: u32,
    height: u32,
    argb: &[u8],
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Ok(());
    }
    fs::create_dir_all(dir).map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    let mut rgb = Vec::with_capacity((width as usize) * (height as usize) * 3);
    for pixel in argb.chunks_exact(4) {
        rgb.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
    }
    let path = dir.join(format!("layer-{name}.png"));
    let file = fs::File::create(&path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|error| format!("png header: {error}"))?;
    writer
        .write_image_data(&rgb)
        .map_err(|error| format!("png data: {error}"))?;
    Ok(())
}

impl Dispatch<wl_registry::WlRegistry, ()> for HostState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            state.globals.push((interface.clone(), name, version));
            if interface == "wl_output" {
                let index = state.outputs.len();
                // `wl_output` version 2 carries `name` and `scale`.
                let proxy: wl_output::WlOutput = registry.bind(name, version.min(4), qh, index);
                state.outputs.push(OutputSlot {
                    proxy,
                    name: String::new(),
                    scale: 1,
                });
            }
        }
    }
}

/// Registry name of a global the shell already recorded.
fn global_name(state: &HostState, interface: &str) -> u32 {
    state
        .globals
        .iter()
        .find(|(name, _, _)| name == interface)
        .map(|(_, name, _)| *name)
        .unwrap_or(0)
}

impl Dispatch<wl_output::WlOutput, usize> for HostState {
    fn event(
        state: &mut Self,
        _proxy: &wl_output::WlOutput,
        event: wl_output::Event,
        index: &usize,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let Some(output) = state.outputs.get_mut(*index) else {
            return;
        };
        match event {
            wl_output::Event::Name { name } => output.name = name,
            wl_output::Event::Scale { factor } => output.scale = factor.max(1),
            _ => {}
        }
    }
}

impl Dispatch<wl_surface::WlSurface, usize> for HostState {
    fn event(
        state: &mut Self,
        _proxy: &wl_surface::WlSurface,
        event: wl_surface::Event,
        index: &usize,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_surface::Event::Enter { output } = event {
            if let Some(entered) = state
                .outputs
                .iter()
                .find(|candidate| candidate.proxy.id() == output.id())
            {
                if let Some(slot) = state.slots.get_mut(*index) {
                    slot.scale = entered.scale.max(1);
                }
            }
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, usize> for HostState {
    fn event(
        state: &mut Self,
        _proxy: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        index: &usize,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let zwlr_layer_surface_v1::Event::Configure {
            serial,
            width,
            height,
        } = event
        {
            if let Some(slot) = state.slots.get_mut(*index) {
                slot.configures.push((serial, width as i32, height as i32));
            }
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for HostState {
    fn event(
        state: &mut Self,
        _proxy: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Key {
                key,
                state: key_state,
                ..
            } => {
                // Key repeat and releases are not shell actions.
                if matches!(key_state, WEnum::Value(wl_keyboard::KeyState::Pressed)) {
                    state.keys.push((key, state.modifiers));
                }
            }
            wl_keyboard::Event::Modifiers { mods_depressed, .. } => {
                state.modifiers = mods_depressed;
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for HostState {
    fn event(
        state: &mut Self,
        _proxy: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities } = event {
            if let WEnum::Value(capabilities) = capabilities {
                state.keyboard = capabilities.contains(wl_seat::Capability::Keyboard);
            }
        }
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for HostState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_compositor::WlCompositor,
        _event: <wl_compositor::WlCompositor as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm::WlShm, ()> for HostState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_shm::WlShm,
        _event: <wl_shm::WlShm as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for HostState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_shm_pool::WlShmPool,
        _event: <wl_shm_pool::WlShmPool as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for HostState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_buffer::WlBuffer,
        _event: <wl_buffer::WlBuffer as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_layer_shell_v1::ZwlrLayerShellV1, ()> for HostState {
    fn event(
        _state: &mut Self,
        _proxy: &zwlr_layer_shell_v1::ZwlrLayerShellV1,
        _event: <zwlr_layer_shell_v1::ZwlrLayerShellV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_open_panels_are_mapped() {
        // A fresh session shows the menu bar and the dock; the three overlay
        // panels are unmapped until their overlay is open.
        assert!(panel_open("menubar", Overlay::None));
        assert!(panel_open("dock", Overlay::None));
        assert!(!panel_open("launcher", Overlay::None));
        assert!(!panel_open("notifications", Overlay::None));
        assert!(!panel_open("control-center", Overlay::None));

        assert!(panel_open("launcher", Overlay::Launcher));
        assert!(!panel_open("notifications", Overlay::Launcher));
        assert!(panel_open("notifications", Overlay::Notifications));
        assert!(panel_open("control-center", Overlay::ControlCenter));
        // The menu bar and the dock stay up under every overlay.
        for overlay in [
            Overlay::Launcher,
            Overlay::ControlCenter,
            Overlay::Notifications,
            Overlay::Overview,
            Overlay::Settings,
            Overlay::Lock,
        ] {
            assert!(panel_open("menubar", overlay), "{overlay:?}");
            assert!(panel_open("dock", overlay), "{overlay:?}");
        }
        // Full-screen overlays have no layer surface yet.
        assert!(!panel_open("settings", Overlay::Settings));
    }
}
