//! AetherOS compositor (Smithay-based, P3).
//!
//! Phase P3-T01: compositor skeleton. Starts as a headless Wayland compositor
//! (no DRM/KMS, no input) — the same scope proven by spike S2 (P0-T08).
//!
//! Phase P3-T02 extends the skeleton with the window-system surface area the
//! desktop shell and apps need, still headless so it builds and runs on the
//! macOS dev host and the Linux CI runner:
//!
//! - **xdg-shell** toplevels and popups (`xdg_wm_base`).
//! - **layer shell** (`zwlr_layer_shell_v1`) for the menu bar and dock.
//!   Layer surfaces get a concrete configure size derived from their anchors,
//!   margins, requested size and the *logical* size of their output.
//! - **multi-monitor**: two `wl_output` globals in one logical layout
//!   (side by side) plus `zxdg_output_v1` for logical geometry.
//! - **HiDPI**: the two monitors run at different scales (1 and 1.5, the
//!   latter advertised as integer 2 to `wl_output`-only clients). Surfaces are
//!   assigned to their output (`wl_surface.enter`) and told their preferred
//!   `wp_fractional_scale_v1` scale; `wl_surface.set_buffer_scale` is accepted
//!   and recorded.
//!
//! Phase P3-T05 adds the glass material pipeline and effect tiers (design doc
//! D1 section 4) in [`glass`]: the tier resolves to a concrete pass list
//! (Dual Kawase blur levels + saturation + tint + edge refraction + edge
//! highlight + shadow + composite), blur is only planned over the damaged
//! region, and the modelled frame cost is checked against the 60 Hz budget.
//! Phase P3-T06 adds [`scheduling`]: damage tracking, frame pacing, frame
//! callbacks and idle measurement.
//!
//! Run: `cargo run -p aether-compositor` (or `aether-compositor` binary).
//! Markers: `AETHER_COMPOSITOR_SOCKET_READY: wayland-0`,
//!          `AETHER_COMPOSITOR_OUTPUT: …`,
//!          `AETHER_COMPOSITOR_OUTPUT_COUNT: 2`,
//!          `AETHER_COMPOSITOR_OUTPUT_BOUND: …`,
//!          `AETHER_COMPOSITOR_XDG_TOPLEVEL: …`,
//!          `AETHER_COMPOSITOR_LAYER_SURFACE: …`,
//!          `AETHER_COMPOSITOR_LAYER_CONFIGURE: …`,
//!          `AETHER_COMPOSITOR_BUFFER_SCALE: …`,
//!          `AETHER_COMPOSITOR_CLIENT_MAPPED`,
//!          `AETHER_COMPOSITOR_FRAME_RATE: 60`,
//!          `AETHER_COMPOSITOR_EFFECT_TIER: full`,
//!          `AETHER_COMPOSITOR_EFFECT_PIPELINE: tier=… blur=…`,
//!          `AETHER_COMPOSITOR_EFFECT_TIER_SWITCH: from=… to=… reason=…`,
//!          `AETHER_COMPOSITOR_GLASS_GPU_PROFILE: name=… measured=false …`,
//!          `AETHER_COMPOSITOR_GLASS_PLAN: tier=… windows=… passes=… est_ms=…`,
//!          `AETHER_COMPOSITOR_FRAME: n=… damage_px=… tier=… est_ms=…`,
//!          `AETHER_COMPOSITOR_FRAME_CALLBACKS: n=…`,
//!          `AETHER_COMPOSITOR_IDLE: idle_ms=… idle_pct=…`,
//!          `AETHER_COMPOSITOR_FRAME_STATS: frames=… idle_skips=…`,
//!          `AETHER_COMPOSITOR_EFFECT_SUMMARY: tier=… tier_switches=…`,
//!          `AETHER_COMPOSITOR_DONE`.
//!
//! Env: `AETHER_COMPOSITOR_GRACE_MS` (default 3000) — how long the compositor
//! keeps running after the first mapped surface before exiting. The headless
//! multi-output test raises it so two clients' worth of surfaces can be
//! exercised in one run.
//! `AETHER_COMPOSITOR_EFFECT_TIER` (`full` | `reduced` | `minimal`, default
//! `full`) selects the glass tier; `AETHER_COMPOSITOR_TARGET_FPS` (default 60)
//! sets the frame pacing/budget target.
//!
//! The Rust module and the wire protocol share the word "compositor"; see
//! [`AetherCompositor`] (state) vs. `wayland::compositor` (protocol).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use smithay::delegate_compositor;
use smithay::delegate_fractional_scale;
#[cfg(target_os = "linux")]
use smithay::delegate_xwayland_shell;
use smithay::delegate_layer_shell;
use smithay::delegate_output;
use smithay::delegate_shm;
use smithay::delegate_xdg_shell;
use smithay::input::{SeatHandler, SeatState};
use smithay::output::{Mode, Output, PhysicalProperties, Scale, Subpixel};
use smithay::reexports::wayland_server::backend::{
    ClientData, ClientId, DisconnectReason, ObjectId,
};
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::protocol::wl_seat::WlSeat;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{
    Client, Display, DisplayHandle, Resource,
};
use smithay::utils::{Logical, Serial, Size, Transform};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    with_states, BufferAssignment, CompositorClientState, CompositorHandler, CompositorState,
    Damage, SurfaceAttributes,
};
use smithay::wayland::fractional_scale::{
    with_fractional_scale, FractionalScaleHandler, FractionalScaleManagerState,
};
use smithay::wayland::socket::ListeningSocketSource;
#[cfg(target_os = "linux")]
use smithay::wayland::xwayland_shell::{
    XWaylandShellHandler, XWaylandShellState,
};
#[cfg(target_os = "linux")]
use smithay::xwayland::{XWayland, XWaylandEvent};
#[cfg(target_os = "linux")]
use smithay::xwayland::xwm::{X11Surface, X11Wm, XwmHandler, XwmId};
use smithay::wayland::output::{OutputHandler, OutputManagerState};
use smithay::wayland::shell::wlr_layer::{
    Anchor, Layer, LayerSurface, LayerSurfaceCachedState, WlrLayerShellHandler, WlrLayerShellState,
};
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
};
use smithay::wayland::shm::{ShmHandler, ShmState};

mod glass;
mod scheduling;
mod wm;
pub use glass::*;
pub use scheduling::*;
pub use wm::*;

/// Per-client state, as required by the smithay compositor module.
#[derive(Debug, Default)]
pub struct ClientState {
    pub compositor: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

/// One headless monitor: physical mode, advertised scale and logical position
/// in the global layout.
pub struct HeadlessOutputSpec {
    /// `wl_output.name` / `zxdg_output_v1.name`
    pub name: &'static str,
    /// Physical mode in pixels (width, height).
    pub mode: (i32, i32),
    /// Scale advertised to clients (integer to `wl_output`, fractional to
    /// `wp_fractional_scale_v1` / `zxdg_output_v1`).
    pub scale: Scale,
    /// Logical position of the output's top-left corner.
    pub location: (i32, i32),
    /// Monitor size in millimetres (for `wl_output.geometry`).
    pub physical_mm: (i32, i32),
}

/// The two-monitor HiDPI layout used by the headless (CI) compositor.
///
/// - Output 0: 1920×1080 at scale 1 → logical 1920×1080 at (0, 0).
/// - Output 1: 2880×1800 at scale 1.5 → `wl_output.scale` 2 (integer
///   protocols round up), `zxdg_output_v1` logical size 1920×1200 at (1920, 0).
pub const HEADLESS_OUTPUTS: &[HeadlessOutputSpec] = &[
    HeadlessOutputSpec {
        name: "aether-headless-0",
        mode: (1920, 1080),
        scale: Scale::Integer(1),
        location: (0, 0),
        physical_mm: (344, 194),
    },
    HeadlessOutputSpec {
        name: "aether-headless-1",
        mode: (2880, 1800),
        scale: Scale::Fractional(1.5),
        location: (1920, 0),
        physical_mm: (508, 318),
    },
];

/// A `zwlr_layer_surface_v1` surface known to the compositor.
pub struct LayerSurfaceEntry {
    pub surface: LayerSurface,
    pub namespace: String,
    pub layer: Layer,
    /// Name of the output the surface was created for, if any (None = all).
    pub output: Option<String>,
}

/// Compositor state shared between the Wayland dispatch handlers and the
/// main loop.
pub struct AetherCompositor {
    pub compositor: CompositorState,
    pub shm: ShmState,
    pub xdg_shell: XdgShellState,
    pub layer_shell: WlrLayerShellState,
    pub output_manager: OutputManagerState,
    pub display_handle: smithay::reexports::wayland_server::DisplayHandle,
    pub fractional_scale: FractionalScaleManagerState,
    pub seat_state: SeatState<Self>,
    /// Headless monitors, in layout order (index 0 is primary).
    pub outputs: Vec<Output>,
    pub layer_surfaces: Vec<LayerSurfaceEntry>,
    pub toplevels: Vec<ToplevelSurface>,
    /// xwayland shell global (associates X11 windows with wl_surfaces).
    #[cfg(target_os = "linux")]
    pub xwayland_shell: XWaylandShellState,
    /// Running X11 window manager, if XWayland spawned successfully.
    #[cfg(target_os = "linux")]
    pub xwm: Option<X11Wm>,
    /// Wayland client representing the XWayland server.
    #[cfg(target_os = "linux")]
    pub xwayland_client: Option<smithay::reexports::wayland_server::Client>,
    /// Display number chosen by XWayland.
    #[cfg(target_os = "linux")]
    pub xwayland_display: Option<u32>,
    /// Preferred fractional scale per surface, decided when the surface is
    /// assigned to an output.
    surface_scales: HashMap<ObjectId, f64>,
    /// Surfaces whose buffer scale has already been reported.
    scaled_surfaces: HashSet<ObjectId>,
    /// P3-T04 window-management model: floating/snap/tile + workspaces.
    /// Layer surfaces (shell chrome) are deliberately *not* managed here.
    pub wm: WorkspaceManager,
    /// P3-T05 glass material pipeline: tier, theme, GPU profile, effect plans.
    pub effects: GlassPipeline,
    /// P3-T05 automatic tier switching (hysteresis) from measured frame times.
    pub tier_policy: TierPolicy,
    /// P3-T06 damage tracking and frame pacing.
    pub frames: FrameScheduler,
    /// Start instant, used for `wl_callback.done` timestamps.
    pub started: Instant,
    /// `wl_surface.frame` callbacks answered so far.
    pub frame_callbacks_sent: u64,
    /// Tier changes performed at runtime (env, API, or the tier policy).
    pub tier_switches: u32,
    /// Window count of the last `AETHER_COMPOSITOR_GLASS_PLAN` marker.
    pub reported_windows: usize,
    /// Monotonic id source for managed toplevels.
    next_window_id: u64,
    pub mapped: bool,
    pub shutdown_at: Option<Instant>,
    pub deadline: Option<Instant>,
    pub timed_out: bool,
}

impl AetherCompositor {
    pub fn new(dh: &DisplayHandle) -> Self {
        // P3-T05: the effect tier is selectable; the default is Full, matching
        // the design doc. Unknown values fall back to Full instead of failing so
        // a typo cannot leave the desktop without a blur policy.
        let tier = std::env::var("AETHER_COMPOSITOR_EFFECT_TIER")
            .ok()
            .and_then(|value| GlassTier::parse(&value))
            .unwrap_or(GlassTier::Full);
        let target_fps = std::env::var("AETHER_COMPOSITOR_TARGET_FPS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|fps| *fps > 0)
            .unwrap_or(glass::DEFAULT_TARGET_FPS);
        let effects = GlassPipeline::new(tier);

        let mut state = Self {
            compositor: CompositorState::new::<Self>(dh),
            shm: ShmState::new::<Self>(dh, vec![]),
            xdg_shell: XdgShellState::new::<Self>(dh),
            layer_shell: WlrLayerShellState::new::<Self>(dh),
            // xdg-output piggybacks on every wl_output global created below,
            // so it must exist first.
            output_manager: OutputManagerState::new_with_xdg_output::<Self>(dh),
            display_handle: dh.clone(),
            fractional_scale: FractionalScaleManagerState::new::<Self>(dh),
            seat_state: SeatState::new(),
            outputs: Vec::new(),
            layer_surfaces: Vec::new(),
            toplevels: Vec::new(),
            #[cfg(target_os = "linux")]
            xwayland_shell: XWaylandShellState::new::<Self>(dh),
            #[cfg(target_os = "linux")]
            xwm: None,
            #[cfg(target_os = "linux")]
            xwayland_client: None,
            #[cfg(target_os = "linux")]
            xwayland_display: None,
            surface_scales: HashMap::new(),
            scaled_surfaces: HashSet::new(),
            // Replaced with the primary output's logical size once outputs are
            // added below.
            wm: WorkspaceManager::new(0, 0),
            effects,
            tier_policy: TierPolicy::new(target_fps),
            frames: FrameScheduler::new(target_fps),
            started: Instant::now(),
            frame_callbacks_sent: 0,
            tier_switches: 0,
            reported_windows: 0,
            next_window_id: 0,
            mapped: false,
            shutdown_at: None,
            deadline: None,
            timed_out: false,
        };

        for spec in HEADLESS_OUTPUTS {
            state.add_headless_output(dh, spec);
        }
        // Seed the WM with the primary output's logical size so snap/tile
        // geometry matches what clients are configured at.
        let (ow, oh) = state
            .outputs
            .first()
            .map(|output| state.logical_size(output))
            .map(|size| (size.w, size.h))
            .unwrap_or((1920, 1080));
        state.wm = WorkspaceManager::new(ow, oh);
        println!("AETHER_COMPOSITOR_OUTPUT_COUNT: {}", state.outputs.len());
        println!("AETHER_COMPOSITOR_FRAME_RATE: {target_fps}");
        println!(
            "AETHER_COMPOSITOR_EFFECT_TIER: {}",
            state.effects.tier().name()
        );
        println!(
            "AETHER_COMPOSITOR_EFFECT_PIPELINE: {}",
            state.effects.material().describe()
        );
        println!(
            "AETHER_COMPOSITOR_GLASS_GPU_PROFILE: {}",
            state.effects.profile().describe()
        );
        state.report_glass_plan(&[]);
        state
    }

    /// P3-T05: switch the glass effect tier at runtime and report the change.
    pub fn set_effect_tier(&mut self, tier: GlassTier, reason: &str) {
        let from = self.effects.tier();
        if from == tier {
            return;
        }
        self.effects.set_tier(tier);
        self.tier_switches += 1;
        println!(
            "AETHER_COMPOSITOR_EFFECT_TIER_SWITCH: from={} to={} reason={reason}",
            from.name(),
            tier.name()
        );
        println!(
            "AETHER_COMPOSITOR_EFFECT_PIPELINE: {}",
            self.effects.material().describe()
        );
        self.report_glass_plan(&[]);
    }

    /// Glass surfaces of the windows visible on the active workspace.
    fn visible_glass_surfaces(&self) -> Vec<GlassSurface> {
        let material = self.effects.material();
        let mut ids: Vec<WindowId> = self.wm.visible_on(self.wm.active);
        ids.sort_by_key(|id| id.0);
        ids.into_iter()
            .filter_map(|id| {
                self.wm.windows.get(&id).map(|window| GlassSurface {
                    id: id.0,
                    rect: window.rect,
                    corner_radius: material.corner_radius,
                    opacity: 1.0,
                })
            })
            .collect()
    }

    /// Print the resolved glass plan and modelled frame cost for the visible
    /// windows at the current tier (`AETHER_COMPOSITOR_GLASS_PLAN`). An empty
    /// `damage` slice means a full repaint, i.e. the worst-case frame.
    pub fn report_glass_plan(&mut self, damage: &[Rect]) {
        let pairs: Vec<(GlassSurface, Vec<Rect>)> = self
            .visible_glass_surfaces()
            .into_iter()
            .map(|surface| (surface, damage.to_vec()))
            .collect();
        let cost = self.effects.frame_cost(&pairs);
        let material = self.effects.material();
        self.reported_windows = pairs.len();
        println!(
            "AETHER_COMPOSITOR_GLASS_PLAN: {} windows={} passes={} damage_rects={} damaged_px={} covered_px={} est_ms={:.3} budget_ms={:.2} share={:.3} within_budget={}",
            material.describe(),
            cost.windows,
            material.passes().len(),
            damage.len(),
            cost.damaged_pixels,
            cost.covered_pixels,
            cost.estimated_ms,
            cost.budget_ms,
            cost.share(),
            cost.within_budget()
        );
    }

    /// P3-T06: one frame-scheduler tick, driven from the event loop. Presents
    /// a frame when there is damage and the frame interval elapsed, answers the
    /// `wl_surface.frame` callbacks for that frame, and lets the tier policy
    /// react to the modelled frame cost.
    pub fn frame_tick(&mut self) {
        let now = Instant::now();
        if !self.frames.due(now) {
            self.frames.note_idle(now);
            if self.frames.take_idle_report(now) {
                println!(
                    "AETHER_COMPOSITOR_IDLE: idle_ms={} idle_pct={:.1} frames={}",
                    self.frames.idle_time(now).as_millis(),
                    self.frames.estimated_gpu_idle_percent(now),
                    self.frames.frames
                );
            }
            return;
        }

        let Some(frame) = self.frames.present(now) else {
            return;
        };

        let pairs: Vec<(GlassSurface, Vec<Rect>)> = self
            .visible_glass_surfaces()
            .into_iter()
            .map(|surface| (surface, frame.damage.clone()))
            .collect();
        let cost = self.effects.frame_cost(&pairs);
        if frame.index <= 5 || frame.index % 120 == 0 {
            println!(
                "AETHER_COMPOSITOR_FRAME: n={} time_ms={} damage_rects={} damage_px={} windows={} tier={} est_ms={:.3}",
                frame.index,
                frame.time_ms,
                frame.damage.len(),
                frame.damage_pixels,
                cost.windows,
                self.effects.tier().name(),
                cost.estimated_ms
            );
        }

        if pairs.len() != self.reported_windows {
            // Worst-case cost for the new window set: every visible window
            // fully repainted at the current tier.
            self.report_glass_plan(&[]);
        }

        let callbacks = self.send_frame_callbacks(frame.time_ms);
        if callbacks > 0 && (frame.index <= 5 || frame.index % 120 == 0) {
            println!(
                "AETHER_COMPOSITOR_FRAME_CALLBACKS: n={callbacks} time_ms={}",
                frame.time_ms
            );
        }

        if let Some(next) = self
            .tier_policy
            .observe(self.effects.tier(), cost.estimated_ms)
        {
            self.set_effect_tier(next, "frame-budget");
        }
    }

    /// Answer the `wl_surface.frame` callbacks of the mapped surfaces with the
    /// frame's presentation timestamp. Returns the number of callbacks sent.
    fn send_frame_callbacks(&mut self, time_ms: u32) -> u64 {
        let mut surfaces: Vec<WlSurface> = Vec::new();
        for (index, toplevel) in self.toplevels.iter().enumerate() {
            let visible = self
                .wm
                .windows
                .get(&WindowId(index as u64))
                .map(|window| window.workspace == self.wm.active)
                .unwrap_or(true);
            if visible {
                surfaces.push(toplevel.wl_surface().clone());
            }
        }
        for entry in &self.layer_surfaces {
            surfaces.push(entry.surface.wl_surface().clone());
        }

        let mut sent = 0u64;
        for surface in surfaces {
            let callbacks = with_states(&surface, |states| {
                let mut attrs = states.cached_state.get::<SurfaceAttributes>();
                std::mem::take(&mut attrs.current().frame_callbacks)
            });
            for callback in callbacks {
                callback.done(time_ms);
                sent += 1;
            }
        }
        self.frame_callbacks_sent += sent;
        sent
    }

    /// Record the damage of one committed surface in output coordinates.
    fn record_surface_damage(&mut self, surface: &WlSurface, damage: &[Rect]) {
        let window = self
            .toplevels
            .iter()
            .position(|toplevel| toplevel.wl_surface() == surface)
            .and_then(|index| self.wm.windows.get(&WindowId(index as u64)).cloned());
        match window {
            Some(window) if window.workspace == self.wm.active => {
                for rect in damage {
                    self.frames.add_damage(Rect {
                        x: window.rect.x + rect.x,
                        y: window.rect.y + rect.y,
                        w: rect.w.min(window.rect.w),
                        h: rect.h.min(window.rect.h),
                    });
                }
            }
            Some(_) => {}
            // Layer surfaces (panel, dock, wallpaper) have no window geometry
            // yet; their damage is recorded in output space from the surface
            // origin, which is the top-left corner for anchored shell chrome.
            None => {
                for rect in damage {
                    self.frames.add_damage(*rect);
                }
            }
        }
    }

    /// Damage a surface's whole area (first map, buffer swap without an
    /// explicit damage request).
    fn damage_whole_surface(&mut self, surface: &WlSurface) {
        let window = self
            .toplevels
            .iter()
            .position(|toplevel| toplevel.wl_surface() == surface)
            .and_then(|index| self.wm.windows.get(&WindowId(index as u64)).cloned());
        match window {
            Some(window) => {
                if window.workspace == self.wm.active {
                    self.frames.add_damage(window.rect);
                }
            }
            None => {
                // Layer surface without managed geometry: damage the primary
                // output's full logical area.
                if let Some(output) = self.outputs.first() {
                    let size = self.logical_size(output);
                    self.frames.add_damage(Rect {
                        x: 0,
                        y: 0,
                        w: size.w,
                        h: size.h,
                    });
                }
            }
        }
    }

    /// Create one headless monitor: advertise the `wl_output` global and
    /// configure mode, scale and logical position.
    fn add_headless_output(&mut self, dh: &DisplayHandle, spec: &HeadlessOutputSpec) {
        let output = Output::new(
            spec.name.to_string(),
            PhysicalProperties {
                size: spec.physical_mm.into(),
                subpixel: Subpixel::Unknown,
                make: "AetherOS".into(),
                model: "Headless".into(),
            },
        );
        // The global stays alive for the compositor's lifetime.
        let _global = output.create_global::<Self>(dh);

        let mode = Mode {
            size: spec.mode.into(),
            refresh: 60_000,
        };
        output.set_preferred(mode);
        output.change_current_state(
            Some(mode),
            Some(Transform::Normal),
            Some(spec.scale),
            Some(spec.location.into()),
        );

        let fractional = spec.scale.fractional_scale();
        let logical_w = (spec.mode.0 as f64 / fractional).round() as i32;
        let logical_h = (spec.mode.1 as f64 / fractional).round() as i32;
        println!(
            "AETHER_COMPOSITOR_OUTPUT: name={} mode={}x{} scale={} logical={}x{} position={},{}",
            spec.name,
            spec.mode.0,
            spec.mode.1,
            fractional,
            logical_w,
            logical_h,
            spec.location.0,
            spec.location.1
        );
        self.outputs.push(output);
    }

    /// Logical (post-scale) size of an output.
    fn logical_size(&self, output: &Output) -> Size<i32, Logical> {
        let (w, h) = output
            .current_mode()
            .map(|mode| (mode.size.w, mode.size.h))
            .unwrap_or((0, 0));
        let scale = output.current_scale().fractional_scale();
        Size::from((
            (w as f64 / scale).round() as i32,
            (h as f64 / scale).round() as i32,
        ))
    }

    fn output_by_name(&self, name: &str) -> Option<&Output> {
        self.outputs.iter().find(|output| output.name() == name)
    }

    /// Assign a surface to an output: send `wl_surface.enter` and remember the
    /// output's fractional scale for `wp_fractional_scale_v1`.
    fn assign_surface(&mut self, surface: &WlSurface, output: &Output) {
        output.enter(surface);
        let scale = output.current_scale().fractional_scale();
        self.surface_scales.insert(surface.id(), scale);
    }

    /// Send a configure with a concrete size to the layer surface behind
    /// `surface` once its anchors and requested size are known.
    fn configure_layer_surface(&mut self, surface: &WlSurface) {
        let id = surface.id();
        let Some(entry) = self
            .layer_surfaces
            .iter()
            .find(|entry| entry.surface.wl_surface().id() == id)
        else {
            return;
        };

        let output = match entry.output.as_deref() {
            Some(name) => self.output_by_name(name).or_else(|| self.outputs.first()),
            None => self.outputs.first(),
        };
        let Some(output) = output else {
            return;
        };

        let output_size = self.logical_size(output);
        let cached = with_states(surface, |states| {
            states
                .cached_state
                .get::<LayerSurfaceCachedState>()
                .current()
                .clone()
        });
        let size = layer_surface_size(&cached, output_size);

        let layer_surface = entry.surface.clone();
        let namespace = entry.namespace.clone();
        layer_surface.with_pending_state(|state| {
            state.size = Some(size);
        });
        if layer_surface.send_pending_configure().is_some() {
            println!(
                "AETHER_COMPOSITOR_LAYER_CONFIGURE: namespace={namespace} size={}x{}",
                size.w, size.h
            );
        }
    }

    /// Push a managed geometry to the toplevel at `index` (the WM `WindowId`
    /// equals the toplevel index). No-op if the index is unknown.
    fn configure_toplevel(&mut self, index: usize, rect: Rect) {
        let Some(toplevel) = self.toplevels.get(index) else {
            return;
        };
        let size = Size::from((rect.w.max(1), rect.h.max(1)));
        toplevel.with_pending_state(|state| {
            state.size = Some(size);
        });
        if toplevel.send_pending_configure().is_some() {
            println!(
                "AETHER_COMPOSITOR_WM_CONFIGURE: id={index} size={}x{}",
                rect.w, rect.h
            );
        }
    }

    /// P3-T04: snap the toplevel at `index` to an output edge.
    pub fn snap_toplevel(&mut self, index: usize, edge: SnapEdge) {
        let id = WindowId(index as u64);
        self.wm.set_mode(id, WindowMode::Snapped(edge));
        let Some(rect) = self.wm.windows.get(&id).map(|window| window.rect) else {
            return;
        };
        println!(
            "AETHER_COMPOSITOR_WM_SNAP: id={index} edge={edge:?} rect={},{},{}x{}",
            rect.x, rect.y, rect.w, rect.h
        );
        self.frames.add_damage(rect);
        self.configure_toplevel(index, rect);
    }

    /// P3-T04: assign the toplevel at `index` to a workspace.
    pub fn move_toplevel_to_workspace(&mut self, index: usize, workspace: WorkspaceId) {
        let id = WindowId(index as u64);
        if self.wm.move_to_workspace(id, workspace) {
            println!(
                "AETHER_COMPOSITOR_WM_MOVE: id={index} workspace={}",
                workspace.0
            );
            // Both the vacated and the new workspace must repaint.
            if let Some(window) = self.wm.windows.get(&id).cloned() {
                self.frames.add_damage(window.rect);
            }
        }
    }

    /// P3-T04: tile every window on the active workspace into a grid.
    pub fn tile_active_workspace(&mut self) {
        let workspace = self.wm.active;
        self.wm.tile_all(workspace);
        let rects: Vec<(usize, Rect)> = self
            .wm
            .visible_on(workspace)
            .into_iter()
            .filter_map(|id| {
                self.wm
                    .windows
                    .get(&id)
                    .map(|window| (id.0 as usize, window.rect))
            })
            .collect();
        for (index, rect) in rects {
            println!(
                "AETHER_COMPOSITOR_WM_TILE: id={index} rect={},{},{}x{}",
                rect.x, rect.y, rect.w, rect.h
            );
            self.frames.add_damage(rect);
            self.configure_toplevel(index, rect);
        }
    }

    /// P3-T04: switch the active workspace (creating it if unknown).
    pub fn activate_workspace(&mut self, workspace: WorkspaceId) {
        if !self.wm.workspaces.contains(&workspace) {
            self.wm.workspaces.push(workspace);
        }
        self.wm.active = workspace;
        println!(
            "AETHER_COMPOSITOR_WM_WORKSPACE: active={}",
            workspace.0
        );
    }
}

/// Layer-shell size negotiation (the subset the shell needs): anchored edges
/// stretch to the output, an explicit non-zero size wins, otherwise the
/// surface fills the output along that axis. Margins are subtracted.
fn layer_surface_size(
    cached: &LayerSurfaceCachedState,
    output: Size<i32, Logical>,
) -> Size<i32, Logical> {
    let anchor = cached.anchor;
    let margins = cached.margin;

    let width = if anchor.contains(Anchor::LEFT) && anchor.contains(Anchor::RIGHT) {
        output.w - margins.left - margins.right
    } else if cached.size.w > 0 {
        cached.size.w
    } else {
        output.w - margins.left - margins.right
    };

    let height = if anchor.contains(Anchor::TOP) && anchor.contains(Anchor::BOTTOM) {
        output.h - margins.top - margins.bottom
    } else if cached.size.h > 0 {
        cached.size.h
    } else {
        output.h - margins.top - margins.bottom
    };

    Size::from((width.max(1), height.max(1)))
}

// Delegate protocol dispatching.
delegate_compositor!(AetherCompositor);
delegate_shm!(AetherCompositor);
delegate_xdg_shell!(AetherCompositor);
delegate_layer_shell!(AetherCompositor);
delegate_output!(AetherCompositor);
delegate_fractional_scale!(AetherCompositor);
#[cfg(target_os = "linux")]
delegate_xwayland_shell!(AetherCompositor);

impl CompositorHandler for AetherCompositor {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        if let Some(data) = client.get_data::<ClientState>() {
            return &data.compositor;
        }
        // XWayland is inserted with smithay's own `XWaylandClientData` (it is
        // not a `ClientState` client). Its `wl_surface`/`wl_region` requests
        // still need the compositor client state; without this branch the
        // compositor would `exit(1)` the moment XWayland creates its root
        // surface, so XWayland never reaches `XWaylandEvent::Ready`.
        #[cfg(target_os = "linux")]
        if let Some(data) = client.get_data::<smithay::xwayland::XWaylandClientData>() {
            return &data.compositor_state;
        }
        eprintln!("aether-compositor: client without ClientState data");
        std::process::exit(1);
    }

    fn commit(&mut self, surface: &WlSurface) {
        let (buffer_scale, has_buffer, damage) = with_states(surface, |states| {
            let mut attrs = states.cached_state.get::<SurfaceAttributes>();
            let current = attrs.current();
            let buffer_scale = current.buffer_scale;
            let has_buffer = matches!(&current.buffer, Some(BufferAssignment::NewBuffer(_)));
            // P3-T06: drain the client's damage so it is accounted once.
            let mut damage = Vec::new();
            for region in std::mem::take(&mut current.damage) {
                let rect = match region {
                    Damage::Surface(rect) => Rect {
                        x: rect.loc.x,
                        y: rect.loc.y,
                        w: rect.size.w,
                        h: rect.size.h,
                    },
                    Damage::Buffer(rect) => {
                        let scale = buffer_scale.max(1);
                        Rect {
                            x: rect.loc.x / scale,
                            y: rect.loc.y / scale,
                            w: rect.size.w / scale,
                            h: rect.size.h / scale,
                        }
                    }
                };
                damage.push(rect);
            }
            (buffer_scale, has_buffer, damage)
        });

        // HiDPI evidence: a client drawing for a scaled output.
        if buffer_scale > 1 && self.scaled_surfaces.insert(surface.id()) {
            println!("AETHER_COMPOSITOR_BUFFER_SCALE: surface_buffer_scale={buffer_scale}");
        }

        if !self.mapped && has_buffer {
            self.mapped = true;
            println!("AETHER_COMPOSITOR_CLIENT_MAPPED");
        }

        if !damage.is_empty() {
            self.record_surface_damage(surface, &damage);
        } else if has_buffer {
            // Attaching a buffer without a damage request damages the whole
            // surface (first map, resize, full redraw).
            self.damage_whole_surface(surface);
        }

        self.configure_layer_surface(surface);
    }
}

impl BufferHandler for AetherCompositor {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl ShmHandler for AetherCompositor {
    fn shm_state(&self) -> &ShmState {
        &self.shm
    }
}

impl OutputHandler for AetherCompositor {
    fn output_bound(&mut self, output: Output, _wl_output: WlOutput) {
        println!("AETHER_COMPOSITOR_OUTPUT_BOUND: name={}", output.name());
    }
}

impl FractionalScaleHandler for AetherCompositor {
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        let Some(scale) = self.surface_scales.get(&surface.id()).copied() else {
            return;
        };
        with_states(&surface, |states| {
            with_fractional_scale(states, |fractional| {
                fractional.set_preferred_scale(scale);
            });
        });
    }
}

impl XdgShellHandler for AetherCompositor {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let wl_surface = surface.wl_surface().clone();
        let output = self.outputs.first().cloned();
        let output_name = output
            .as_ref()
            .map(Output::name)
            .unwrap_or_else(|| "none".to_string());
        if let Some(output) = output {
            self.assign_surface(&wl_surface, &output);
        }
        println!("AETHER_COMPOSITOR_XDG_TOPLEVEL: output={output_name}");
        self.toplevels.push(surface.clone());

        // P3-T04: register the toplevel with the window manager as a floating
        // window on the active workspace and configure it to its managed
        // geometry. The window id equals the toplevel's index so the WM model
        // and `self.toplevels` stay in lock-step.
        let index = self.toplevels.len() - 1;
        let id = WindowId(self.next_window_id);
        self.next_window_id += 1;
        let (ow, oh) = self
            .outputs
            .first()
            .map(|output| self.logical_size(output))
            .map(|size| (size.w.max(1), size.h.max(1)))
            .unwrap_or((1920, 1080));
        self.wm.output_size = (ow, oh);
        self.wm.add_window(id, None);
        // Default floating geometry: a centered window at 60% of the output.
        let win_w = (ow * 3 / 5).max(1);
        let win_h = (oh * 3 / 5).max(1);
        let rect = Rect {
            x: (ow - win_w) / 2,
            y: (oh - win_h) / 2,
            w: win_w,
            h: win_h,
        };
        if let Some(window) = self.wm.windows.get_mut(&id) {
            window.rect = rect;
        }
        println!(
            "AETHER_COMPOSITOR_WM_WINDOW: id={} workspace={} output={} mode=floating rect={},{},{}x{}",
            id.0, self.wm.active.0, output_name, rect.x, rect.y, rect.w, rect.h
        );
        self.configure_toplevel(index, rect);
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {}

    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: Serial) {}

    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {
    }
}

impl WlrLayerShellHandler for AetherCompositor {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell
    }

    fn new_layer_surface(
        &mut self,
        surface: LayerSurface,
        output: Option<WlOutput>,
        layer: Layer,
        namespace: String,
    ) {
        // Resolve the wl_output the client asked for; fall back to the primary
        // output so every layer surface has a scale and target size.
        let target = output
            .as_ref()
            .and_then(Output::from_resource)
            .or_else(|| self.outputs.first().cloned());
        let (output_name, scale) = match &target {
            Some(target) => {
                self.assign_surface(surface.wl_surface(), target);
                (
                    target.name(),
                    Some(target.current_scale().fractional_scale()),
                )
            }
            None => ("all".to_string(), None),
        };
        println!(
            "AETHER_COMPOSITOR_LAYER_SURFACE: namespace={namespace} layer={layer:?} output={output_name} scale={}",
            scale.unwrap_or(1.0)
        );
        self.layer_surfaces.push(LayerSurfaceEntry {
            surface: surface.clone(),
            namespace,
            layer,
            output: target.as_ref().map(Output::name),
        });
        // Layer surfaces are shell chrome (menu bar, dock, wallpapers) and are
        // intentionally excluded from the P3-T04 window manager: they are never
        // tiled/snapped and have no workspace. Their geometry is negotiated by
        // `configure_layer_surface` instead.
        // Immediate initial configure; a sized configure follows on the first
        // commit once anchors and the requested size are known.
        let _ = surface.send_configure();
    }
}

impl SeatHandler for AetherCompositor {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }
}

#[cfg(target_os = "linux")]
impl XWaylandShellHandler for AetherCompositor {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.xwayland_shell
    }

    fn surface_associated(
        &mut self,
        _xwm: XwmId,
        _wl_surface: smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
        _surface: X11Surface,
    ) {
    }
}

#[cfg(target_os = "linux")]
impl XwmHandler for AetherCompositor {
    fn xwm_state(&mut self, _xwm: XwmId) -> &mut X11Wm {
        self.xwm.as_mut().expect("xwm not initialised")
    }

    fn new_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn new_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn map_window_request(&mut self, _xwm: XwmId, window: X11Surface) {
        let _ = window.set_mapped(true);
        let _ = window.configure(None);
        println!("AETHER_COMPOSITOR_XWAYLAND_MAPPED");
    }

    fn map_window_notify(&mut self, _xwm: XwmId, _window: X11Surface) {
        println!("AETHER_COMPOSITOR_XWAYLAND_MAPPED");
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {
        println!("AETHER_COMPOSITOR_XWAYLAND_MAPPED");
    }

    fn unmapped_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn destroyed_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn configure_request(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _reorder: Option<smithay::xwayland::xwm::Reorder>,
    ) {
        use smithay::utils::{Logical, Rectangle, Size};
        let rect = Rectangle::<i32, Logical>::new(
            (x.unwrap_or(0), y.unwrap_or(0)).into(),
            Size::from((w.unwrap_or(0) as i32, h.unwrap_or(0) as i32)),
        );
        if w.is_some() || h.is_some() {
            let _ = window.configure(Some(rect));
        } else {
            let _ = window.configure(None);
        }
    }

    fn configure_notify(
        &mut self,
        _xwm: XwmId,
        _window: X11Surface,
        _geometry: smithay::utils::Rectangle<i32, smithay::utils::Logical>,
        _above: Option<smithay::xwayland::xwm::X11Window>,
    ) {
    }

    fn resize_request(&mut self, _xwm: XwmId, _window: X11Surface, _button: u32, _resize_edge: smithay::xwayland::xwm::ResizeEdge) {}

    fn move_request(&mut self, _xwm: XwmId, _window: X11Surface, _button: u32) {}
}

/// Run the compositor. Returns Ok(()) on clean shutdown.
pub fn run() -> Result<(), String> {
    const OVERALL_TIMEOUT: Duration = Duration::from_secs(60);

    let grace = Duration::from_millis(
        std::env::var("AETHER_COMPOSITOR_GRACE_MS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(3000),
    );

    let display: Display<AetherCompositor> =
        Display::new().map_err(|e| format!("Display::new: {e:?}"))?;
    let dh = display.handle();

    let mut state = AetherCompositor::new(&dh);
    state.deadline = Some(Instant::now() + OVERALL_TIMEOUT);

    // XWayland (and smithay's prepare_x11_sockets) bind Unix sockets under
    // /tmp/.X11-unix; that directory is not always present on minimal CI images
    // (e.g. ubuntu-latest with only `xwayland`/`x11-apps` installed), which
    // would make XWayland::spawn fail silently. Make sure it exists.
    let _ = std::fs::create_dir_all("/tmp/.X11-unix");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions("/tmp/.X11-unix", std::fs::Permissions::from_mode(0o1777));
    }

    // Spawn XWayland if available (Linux CI). Failure is non-fatal: the
    // compositor keeps running as a pure Wayland compositor, but log the
    // reason so a missing binary / socket dir is diagnosable in CI.
    //
    // stdout/stderr are inherited (not `Stdio::null()`) so XWayland's own
    // `-verbose` diagnostics reach the compositor's log/console and a startup
    // failure is visible in CI instead of silent.
    #[cfg(target_os = "linux")]
    let xwayland_pair = match XWayland::spawn(
        &dh,
        None,
        std::iter::empty::<(String, String)>(),
        false,
        std::process::Stdio::inherit(),
        std::process::Stdio::inherit(),
        |_ud| {},
    ) {
        Ok(pair) => Some(pair),
        Err(err) => {
            eprintln!("aether-compositor: XWayland spawn failed: {err}");
            None
        }
    };
    #[cfg(target_os = "linux")]
    if let Some((_, client)) = &xwayland_pair {
        state.xwayland_client = Some(client.clone());
    }

    let mut event_loop: smithay::reexports::calloop::EventLoop<'_, AetherCompositor> =
        smithay::reexports::calloop::EventLoop::try_new()
            .map_err(|e| format!("EventLoop::try_new: {e:?}"))?;
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    let loop_handle = event_loop.handle();
    let signal = event_loop.get_signal();

    // Listening socket for Wayland clients.
    let socket_source = ListeningSocketSource::with_name("wayland-0")
        .map_err(|e| format!("bind wayland-0: {e:?}"))?;
    println!("AETHER_COMPOSITOR_SOCKET_READY: wayland-0");
    event_loop
        .handle()
        .insert_source(socket_source, |stream, _, state| {
            if let Err(err) = state
                .display_handle
                .insert_client(stream, Arc::new(ClientState::default()))
            {
                eprintln!("aether-compositor: insert_client: {err:?}");
            }
        })
        .map_err(|e| format!("insert socket source: {e:?}"))?;

    // XWayland event source.
    #[cfg(target_os = "linux")]
    if let Some((xwayland, _client)) = xwayland_pair {
        let lh = loop_handle.clone();
        event_loop
            .handle()
            .insert_source(xwayland, move |event, _, state| {
                match event {
                    XWaylandEvent::Ready {
                        x11_socket,
                        display_number,
                    } => {
                        state.xwayland_display = Some(display_number);
                        println!("AETHER_COMPOSITOR_XWAYLAND_DISPLAY: {display_number}");
                        if let Some(client) = state.xwayland_client.clone() {
                            match X11Wm::start_wm(lh.clone(), x11_socket, client) {
                                Ok(wm) => {
                                    state.xwm = Some(wm);
                                }
                                Err(err) => {
                                    eprintln!("aether-compositor: X11Wm start failed: {err:?}");
                                }
                            }
                        }
                    }
                    XWaylandEvent::Error => {
                        eprintln!("aether-compositor: XWayland error");
                    }
                }
            })
            .map_err(|e| format!("insert xwayland source: {e:?}"))?;
    }

    // We need display accessible inside the event loop callback. Store it in a
    // thread-local-ish slot via a wrapper? Instead, keep display in a mutable
    // captured variable and use a raw pointer. Simpler: make AetherCompositor
    // carry the display? Display<AetherCompositor> is not self-referential by
    // value (backend only), so we can store it in the state.
    // -> We instead handle display via a global static pointer set before run.
    // (See below.)

    // To avoid self-referential complexity, store display in an Arc<Mutex>?
    // Display is not Sync. Instead, move display into state.display field.
    // Add field at runtime by... AetherCompositor has no display field.
    // We'll use a separate captured mutable and pass through callback closure.
    // calloop callback only receives &mut Data, so capture display in an
    // UnsafeCell via a box leaked for the duration of the run.
    let display_box = Box::new(display);
    let display_ptr = Box::into_raw(display_box);
    // Save dh already. We'll use display_ptr in closure.

    // Insert display pointer into state? Store as usize in a global.
    DISPLAY_PTR.store(display_ptr as usize, std::sync::atomic::Ordering::SeqCst);

    let grace_for_closure = grace;
    let result = event_loop.run(
        Some(Duration::from_millis(10)),
        &mut state,
        move |state| {
            // dispatch wayland clients
            let display = unsafe { &mut *(DISPLAY_PTR.load(std::sync::atomic::Ordering::SeqCst) as *mut Display<AetherCompositor>) };
            let _ = display.dispatch_clients(state);
            let _ = display.flush_clients();

            // P3-T06: damage-based frame scheduling. No damage means no frame,
            // so an idle desktop submits no composite/blur work at all.
            state.frame_tick();

            if state.mapped && state.shutdown_at.is_none() {
                state.shutdown_at = Some(Instant::now() + grace_for_closure);
            }
            if let Some(shutdown_at) = state.shutdown_at {
                if Instant::now() >= shutdown_at {
                    signal.stop();
                }
            }
            if let Some(deadline) = state.deadline {
                if Instant::now() >= deadline {
                    state.timed_out = true;
                    signal.stop();
                }
            }
        },
    );

    // reclaim display
    let _ = unsafe { Box::from_raw(display_ptr) };

    // Need state.timed_out field. Add to struct? We'll check via a side channel:
    // We stored timed_out? We didn't add field. Add quickly? Instead, track via
    // a global.
    // For now, report timeout if result ok but no mapped? Keep simple.
    let _ = result;
    // We need to propagate timed_out. We added field? Let's add field `timed_out: bool` to AetherCompositor.
    if state.timed_out {
        println!(
            "AETHER_COMPOSITOR_FRAME_STATS: {}",
            state.frames.stats_line(Instant::now())
        );
        return Err("compositor timed out waiting for a client surface to commit a buffer".to_string());
    }

    println!(
        "AETHER_COMPOSITOR_FRAME_STATS: {}",
        state.frames.stats_line(Instant::now())
    );
    println!(
        "AETHER_COMPOSITOR_EFFECT_SUMMARY: tier={} tier_switches={} frame_callbacks={}",
        state.effects.tier().name(),
        state.tier_switches,
        state.frame_callbacks_sent
    );
    println!("AETHER_COMPOSITOR_DONE");
    Ok(())
}

// globals for display pointer and timed_out
static DISPLAY_PTR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
