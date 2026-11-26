//! P3-T09: real hardware backend — DRM/KMS scanout plus libinput input.
//!
//! Linux-only and compiled with the `drm` cargo feature. The headless backend
//! ([`super::BackendKind::Headless`]) stays the default so the crate builds and
//! tests on the macOS dev host and on CI; this module is type-checked for
//! `x86_64-unknown-linux-gnu` by `scripts/check-linux-target.sh` and runs only
//! on a machine with a DRM device and a seat (the Dell).
//!
//! Pipeline:
//!
//! - **Seat**: `libseat` (`seatd` or systemd-logind) opens `/dev/dri/card*` and
//!   the evdev nodes and reports session pause/resume.
//! - **Device**: `udev` enumerates the DRM nodes of the seat; the primary GPU is
//!   chosen with `udev::primary_gpu`.
//! - **Scanout**: the first connected connector with modes gets the KMS
//!   surface; a `GbmAllocator` + `GbmFramebufferExporter` back a
//!   `DrmCompositor`, which flips the primary plane from a GLES renderer
//!   (`EGLDisplay` on the same GBM device).
//! - **Rendering**: every due frame from the existing [`FrameScheduler`] walks
//!   the same window/layer-surface state the headless path tracks, builds
//!   `WaylandSurfaceRenderElement`s back-to-front, presents them through
//!   `DrmCompositor::render_frame`/`queue_frame`, and answers
//!   `wl_surface.frame` callbacks only after the page flip (`frame_submitted`).
//!   When [`DirectScanout`] reports one opaque full-screen window, only that
//!   element is submitted so the client buffer is flipped on the primary plane
//!   with no composition.
//! - **Input**: libinput events (keyboard, pointer motion/button/axis) are fed
//!   into the Wayland seat created here; pointer hits pick the topmost
//!   window/layer surface and a click moves keyboard focus to it.
//!
//! **Glass passes (P3-T05):** at every tier except `minimal` the backdrop under
//! each window and shell-chrome surface is captured, blurred with the
//! dual-Kawase chain and composited with the tint/edge/refraction shader
//! ([`crate::backend::glass_gles`]) before the surface content is drawn on top.
//! `minimal` runs the same composite program in `fill` mode (solid tinted
//! panel, no backdrop sample).
//!
//! Markers: `AETHER_COMPOSITOR_DRM_SESSION`, `_DRM_PRIMARY_GPU`, `_DRM_DEVICE`,
//! `_DRM_CONNECTOR`, `_DRM_RENDERER`, `_DRM_OUTPUT`, `_DRM_VBLANK`,
//! `_DRM_SESSION_EVENT`, `_DRM_HOTPLUG`, `_DRM_STATS`, `_SEAT`, `_INPUT`,
//! `_INPUT_KEY`, `_INPUT_BUTTON`.
//!
//! [`FrameScheduler`]: crate::FrameScheduler
//! [`DirectScanout`]: crate::DirectScanout

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice};
use smithay::backend::drm::compositor::{DrmCompositor, FrameFlags};
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::backend::drm::{
    DrmDevice, DrmDeviceFd, DrmDeviceNotifier, DrmEvent, DrmNode, NodeType,
};
use smithay::backend::egl::{EGLContext, EGLDisplay};
use smithay::backend::input::{
    Axis, ButtonState, Event as _, InputEvent, KeyState, KeyboardKeyEvent, PointerAxisEvent,
    PointerButtonEvent, PointerMotionEvent,
};
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::ImportDma;
use smithay::backend::renderer::Renderer;
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::{Event as SessionEvent, Session};
use smithay::backend::udev::{primary_gpu, UdevBackend, UdevEvent};
use smithay::input::keyboard::FilterResult;
use smithay::input::pointer::{AxisFrame, ButtonEvent, MotionEvent};
use smithay::input::Seat;
use smithay::output::{Mode, Output, PhysicalProperties, Scale, Subpixel};
use smithay::reexports::drm::control::{
    connector, crtc, Device as ControlDevice, Mode as DrmMode, ModeTypeFlags,
};
use smithay::reexports::input as libinput;
use smithay::reexports::rustix::fs::OFlags;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Display, DisplayHandle};
use smithay::utils::{DeviceFd, Logical, Physical, Point, Size, Transform};
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::wlr_layer::{Anchor, Layer, LayerSurfaceCachedState};
use smithay::wayland::socket::ListeningSocketSource;

use crate::backend::glass_gles::{Backdrop, DrmRenderElement, GlassPasses};
use crate::backend::layer_location;
use crate::glass_shader;
use crate::{
    AetherCompositor, ClientState, Frame, GlassSurface, LayerSurfaceEntry, Rect, WindowId,
};

/// Per-frame user data handed to KMS and returned by `frame_submitted`.
#[derive(Debug, Clone, Copy)]
pub struct FrameUserData {
    /// Frame index from the frame scheduler (`AETHER_COMPOSITOR_FRAME: n=`).
    pub index: u64,
    /// Presentation timestamp in milliseconds since compositor start.
    pub time_ms: u32,
}

type KmsCompositor = DrmCompositor<
    GbmAllocator<DrmDeviceFd>,
    GbmFramebufferExporter<DrmDeviceFd>,
    FrameUserData,
    DrmDeviceFd,
>;

/// Open DRM/GBM/GLES/scanout state for the primary output.
pub struct DrmBackend {
    /// Open DRM device (keeps the fd and the KMS resources alive).
    pub device: DrmDevice,
    /// GBM device the primary-plane swapchain allocates from.
    pub gbm: GbmDevice<DrmDeviceFd>,
    /// GLES renderer (EGL on the GBM device).
    pub renderer: GlesRenderer,
    /// KMS compositor owning the primary/overlay/cursor planes.
    pub compositor: KmsCompositor,
    /// The advertising `wl_output` for this connector.
    pub output: Output,
    /// Connector name (`eDP-1`, `HDMI-A-1`, …).
    pub connector: String,
    /// CRTC driving the connector.
    pub crtc: crtc::Handle,
    /// Current KMS mode.
    pub mode: Mode,
    /// VBlank notifier, moved into the event loop once at startup.
    pub notifier: Option<DrmDeviceNotifier>,
    /// Frame queued to KMS; its callbacks are answered on the next VBlank.
    pub pending: Option<Frame>,
    /// Completed page flips.
    pub vblanks: u64,
    /// Frames queued to KMS.
    pub frames_queued: u64,
    /// Frames presented straight from the client buffer (P3-T07).
    pub direct_frames: u64,
    /// P3-T05 compiled glass shader programs; `None` when compilation failed,
    /// in which case surfaces are composited without the glass passes.
    pub glass: Option<GlassPasses>,
}

/// Run the compositor on the seat's primary DRM device.
pub fn run() -> Result<(), String> {
    // 1. Session (libseat). Without a seat we cannot open DRM/evdev nodes.
    let (mut session, session_notifier) =
        LibSeatSession::new().map_err(|err| format!("libseat session: {err:?}"))?;
    let seat_name = session.seat();
    println!("AETHER_COMPOSITOR_DRM_SESSION: seat={seat_name} backend=libseat");

    // 2. udev: devices of this seat and the primary GPU among them.
    let udev_backend =
        UdevBackend::new(&seat_name).map_err(|err| format!("udev backend: {err}"))?;
    let devices: Vec<(u64, PathBuf)> = udev_backend
        .device_list()
        .map(|(id, path)| (id, path.to_path_buf()))
        .collect();
    println!("AETHER_COMPOSITOR_DRM_DEVICE_LIST: count={}", devices.len());
    let primary = primary_gpu(&seat_name)
        .map_err(|err| format!("primary gpu: {err}"))?
        .or_else(|| devices.first().map(|(_, path)| path.clone()))
        .ok_or_else(|| {
            format!(
                "no DRM device on seat {seat_name:?}; run with \
                 AETHER_COMPOSITOR_BACKEND=headless to start without a GPU"
            )
        })?;
    println!(
        "AETHER_COMPOSITOR_DRM_PRIMARY_GPU: path={}",
        primary.display()
    );

    // 3. Wayland display and compositor state (no synthetic outputs).
    let display: Display<AetherCompositor> =
        Display::new().map_err(|err| format!("Display::new: {err:?}"))?;
    let dh: DisplayHandle = display.handle();
    let mut state = AetherCompositor::new_for_drm(&dh);

    // 4. Bring up the KMS output and register it with the compositor.
    state.drm = Some(open_device(&mut session, &primary)?);
    let (output, mode, connector_name) = {
        let drm = state.drm.as_ref().expect("drm backend just created");
        (drm.output.clone(), drm.mode, drm.connector.clone())
    };
    state.register_output(&dh, output);
    println!(
        "AETHER_COMPOSITOR_DRM_OUTPUT: connector={connector_name} mode={}x{} refresh_mhz={}",
        mode.size.w, mode.size.h, mode.refresh
    );

    // 5. Seat: keyboard + pointer, so libinput events have a destination.
    let mut seat: Seat<AetherCompositor> = state.seat_state.new_wl_seat(&dh, "seat-0");
    seat.add_keyboard(Default::default(), 200, 25)
        .map_err(|err| format!("add_keyboard: {err:?}"))?;
    seat.add_pointer();
    state.seat = Some(seat);
    println!("AETHER_COMPOSITOR_SEAT: name=seat-0 keyboard=true pointer=true");

    // 6. libinput over the seat.
    let mut libinput_context = libinput::Libinput::new_with_udev::<
        LibinputSessionInterface<LibSeatSession>,
    >(session.clone().into());
    libinput_context
        .udev_assign_seat(&seat_name)
        .map_err(|_| format!("libinput udev_assign_seat({seat_name})"))?;
    println!("AETHER_COMPOSITOR_INPUT: backend=libinput seat={seat_name}");

    // 7. Event loop: Wayland socket, session, udev, KMS VBlanks, libinput.
    let mut event_loop: smithay::reexports::calloop::EventLoop<'_, AetherCompositor> =
        smithay::reexports::calloop::EventLoop::try_new()
            .map_err(|err| format!("EventLoop::try_new: {err:?}"))?;
    let signal = event_loop.get_signal();

    let socket_source = ListeningSocketSource::with_name("wayland-0")
        .map_err(|err| format!("bind wayland-0: {err:?}"))?;
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
        .map_err(|err| format!("insert socket source: {err:?}"))?;

    event_loop
        .handle()
        .insert_source(session_notifier, |event, _, state| match event {
            SessionEvent::PauseSession => {
                println!("AETHER_COMPOSITOR_DRM_SESSION_EVENT: state=paused");
                if let Some(drm) = state.drm.as_mut() {
                    drm.device.pause();
                }
            }
            SessionEvent::ActivateSession => {
                println!("AETHER_COMPOSITOR_DRM_SESSION_EVENT: state=active");
                if let Some(drm) = state.drm.as_mut() {
                    if let Err(err) = drm.device.activate(false) {
                        eprintln!("aether-compositor: drm activate: {err}");
                    }
                    // Buffers and the queued frame are invalid after a VT
                    // switch: drop the pending frame and rebuild the swapchain.
                    drm.pending = None;
                    drm.compositor.reset_buffers();
                }
                // Everything on screen is stale after a VT switch.
                state.damage_whole_output();
            }
        })
        .map_err(|err| format!("insert session source: {err:?}"))?;

    // Hotplug is logged only: reconfiguring outputs at runtime (second KMS
    // output, connector swap) is a documented residual of this task.
    event_loop
        .handle()
        .insert_source(udev_backend, |event, _, _state| match event {
            UdevEvent::Added { device_id, path } => println!(
                "AETHER_COMPOSITOR_DRM_HOTPLUG: added id={device_id} path={}",
                path.display()
            ),
            UdevEvent::Removed { device_id } => {
                println!("AETHER_COMPOSITOR_DRM_HOTPLUG: removed id={device_id}")
            }
            UdevEvent::Changed { device_id } => {
                println!("AETHER_COMPOSITOR_DRM_HOTPLUG: changed id={device_id}")
            }
        })
        .map_err(|err| format!("insert udev source: {err:?}"))?;

    // VBlank: the queued frame is on screen, so its callbacks may be answered.
    let (drm_notifier, drm_crtc) = {
        let drm = state.drm.as_mut().expect("drm backend just created");
        let notifier = drm
            .notifier
            .take()
            .ok_or_else(|| "drm notifier already moved".to_string())?;
        (notifier, drm.crtc)
    };
    event_loop
        .handle()
        .insert_source(drm_notifier, move |event, _, state| match event {
            DrmEvent::VBlank(crtc) if crtc == drm_crtc => on_vblank(state),
            DrmEvent::VBlank(_) => {}
            DrmEvent::Error(err) => eprintln!("aether-compositor: drm event error: {err}"),
        })
        .map_err(|err| format!("insert drm source: {err:?}"))?;

    let libinput_backend = LibinputInputBackend::new(libinput_context);
    event_loop
        .handle()
        .insert_source(libinput_backend, |event, _, state| {
            process_input_event(state, event)
        })
        .map_err(|err| format!("insert libinput source: {err:?}"))?;

    // XWayland: X11 applications on the real desktop (P3-T03). Failure is
    // non-fatal, the compositor stays a pure Wayland compositor.
    let loop_handle = event_loop.handle();
    let _ = std::fs::create_dir_all("/tmp/.X11-unix");
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions("/tmp/.X11-unix", std::fs::Permissions::from_mode(0o1777));
    }
    let xwayland_pair = match smithay::xwayland::XWayland::spawn(
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
    if let Some((_, client)) = &xwayland_pair {
        state.xwayland_client = Some(client.clone());
    }
    if let Some((xwayland, _client)) = xwayland_pair {
        let lh = loop_handle.clone();
        event_loop
            .handle()
            .insert_source(xwayland, move |event, _, state| match event {
                smithay::xwayland::XWaylandEvent::Ready {
                    x11_socket,
                    display_number,
                } => {
                    state.xwayland_display = Some(display_number);
                    println!("AETHER_COMPOSITOR_XWAYLAND_DISPLAY: {display_number}");
                    if let Some(client) = state.xwayland_client.clone() {
                        match smithay::xwayland::xwm::X11Wm::start_wm(
                            lh.clone(),
                            x11_socket,
                            client,
                        ) {
                            Ok(wm) => state.xwm = Some(wm),
                            Err(err) => {
                                eprintln!("aether-compositor: X11Wm start failed: {err:?}")
                            }
                        }
                    }
                }
                smithay::xwayland::XWaylandEvent::Error => {
                    eprintln!("aether-compositor: XWayland error")
                }
            })
            .map_err(|err| format!("insert xwayland source: {err:?}"))?;
    }

    // The display cannot live inside the state it dispatches (it would be
    // self-referential), so it is kept alive for the run through a raw pointer,
    // the same approach the headless backend uses.
    let display_ptr = Box::into_raw(Box::new(display));
    DISPLAY_PTR.store(display_ptr as usize, Ordering::SeqCst);
    state.deadline = Some(Instant::now() + Duration::from_secs(60 * 60 * 12));

    let result = event_loop.run(Some(Duration::from_millis(4)), &mut state, move |state| {
        let display =
            unsafe { &mut *(DISPLAY_PTR.load(Ordering::SeqCst) as *mut Display<AetherCompositor>) };
        let _ = display.dispatch_clients(state);
        let _ = display.flush_clients();

        // P3-T06/P3-T07: present when the scheduler says a frame is due; an idle
        // desktop submits nothing.
        if let Err(err) = present_if_due(state) {
            eprintln!("aether-compositor: present: {err}");
        }

        // P3-T08 health ticks are shared with the headless backend.
        state.health_tick(Instant::now());

        if let Some(deadline) = state.deadline {
            if Instant::now() >= deadline {
                signal.stop();
            }
        }
    });
    let _ = result;

    unsafe { drop(Box::from_raw(display_ptr)) };

    if let Some(drm) = state.drm.as_ref() {
        println!(
            "AETHER_COMPOSITOR_DRM_STATS: frames_queued={} vblanks={} direct_frames={}",
            drm.frames_queued, drm.vblanks, drm.direct_frames
        );
    }
    if let Some(glass) = state.drm.as_ref().and_then(|drm| drm.glass.as_ref()) {
        println!("AETHER_COMPOSITOR_GLASS_STATS: {}", glass.stats_line());
    }
    println!(
        "AETHER_COMPOSITOR_FRAME_STATS: {}",
        state.frames.stats_line(Instant::now())
    );
    println!(
        "AETHER_COMPOSITOR_SCANOUT_STATS: {}",
        state.scanout.stats_line()
    );
    println!("AETHER_COMPOSITOR_DONE");
    Ok(())
}

static DISPLAY_PTR: AtomicUsize = AtomicUsize::new(0);

/// Open the primary DRM device and bring up EGL/GLES plus the KMS compositor.
fn open_device(session: &mut LibSeatSession, path: &Path) -> Result<DrmBackend, String> {
    let file = session
        .open(
            path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
        )
        .map_err(|err| format!("open {}: {err:?}", path.display()))?;
    let drm_fd = DrmDeviceFd::new(DeviceFd::from(file));
    let (mut device, notifier) =
        DrmDevice::new(drm_fd.clone(), false).map_err(|err| format!("drm device: {err}"))?;
    let cursor = device.cursor_size();
    println!(
        "AETHER_COMPOSITOR_DRM_DEVICE: path={} atomic={} cursor={}x{}",
        path.display(),
        device.is_atomic(),
        cursor.w,
        cursor.h
    );

    // Connector + preferred mode + CRTC.
    let (connector_name, connector_handle, drm_mode, crtc) = pick_output(&device)?;
    let output_mode: Mode = drm_mode.into();
    println!(
        "AETHER_COMPOSITOR_DRM_CONNECTOR: name={connector_name} mode={}x{}@{}mhz crtc={crtc:?}",
        output_mode.size.w, output_mode.size.h, output_mode.refresh
    );

    let surface = device
        .create_surface(crtc, drm_mode, &[connector_handle])
        .map_err(|err| format!("create_surface: {err}"))?;

    // GBM + EGL + GLES on the same device.
    let gbm = GbmDevice::new(drm_fd.clone()).map_err(|err| format!("gbm device: {err}"))?;
    let egl_display =
        unsafe { EGLDisplay::new(gbm.clone()) }.map_err(|err| format!("EGLDisplay::new: {err}"))?;
    let egl_context =
        EGLContext::new(&egl_display).map_err(|err| format!("EGLContext::new: {err}"))?;
    let mut renderer = unsafe { GlesRenderer::new(egl_context) }
        .map_err(|err| format!("GlesRenderer::new: {err}"))?;
    let renderer_formats: Vec<_> = renderer.dmabuf_formats().iter().copied().collect();
    println!(
        "AETHER_COMPOSITOR_DRM_RENDERER: backend=gles render_formats={}",
        renderer_formats.len()
    );

    // P3-T05: compile the glass programs once, on this EGL context. Failure is
    // non-fatal: the compositor keeps running without the glass passes.
    let glass = match GlassPasses::new(&mut renderer) {
        Ok(glass) => {
            println!(
                "AETHER_COMPOSITOR_GLASS_SHADER: programs=3 backend=gles shaders=kawase-down,kawase-up,glass-composite"
            );
            Some(glass)
        }
        Err(err) => {
            eprintln!("aether-compositor: glass shaders unavailable: {err:?}");
            None
        }
    };

    // Advertising output: the connector's mode at scale 1.
    let output = Output::new(
        connector_name.clone(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "AetherOS".into(),
            model: connector_name.clone(),
        },
    );
    output.change_current_state(
        Some(output_mode),
        Some(Transform::Normal),
        Some(Scale::Integer(1)),
        Some((0, 0).into()),
    );
    output.set_preferred(output_mode);

    let allocator = GbmAllocator::new(
        gbm.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    // Direct scan-out of client buffers needs the device's render node.
    let import_node = DrmNode::from_path(path)
        .ok()
        .and_then(|node| node.node_with_type(NodeType::Render).and_then(Result::ok));
    let exporter = GbmFramebufferExporter::new(gbm.clone(), import_node);
    let compositor = DrmCompositor::new(
        &output,
        surface,
        None,
        allocator,
        exporter,
        [smithay::backend::allocator::Fourcc::Xrgb8888].into_iter(),
        renderer_formats,
        Size::from((cursor.w, cursor.h)),
        Some(gbm.clone()),
    )
    .map_err(|err| format!("DrmCompositor::new: {err:?}"))?;

    Ok(DrmBackend {
        device,
        gbm,
        renderer,
        compositor,
        output,
        connector: connector_name,
        crtc,
        mode: output_mode,
        notifier: Some(notifier),
        pending: None,
        vblanks: 0,
        frames_queued: 0,
        direct_frames: 0,
        glass,
    })
}

/// Pick the first connected connector that has modes, its preferred mode and a
/// CRTC.
fn pick_output(
    device: &DrmDevice,
) -> Result<(String, connector::Handle, DrmMode, crtc::Handle), String> {
    let resources = device
        .resource_handles()
        .map_err(|err| format!("resource_handles: {err}"))?;

    for handle in resources.connectors() {
        let info = match device.get_connector(*handle, false) {
            Ok(info) => info,
            Err(err) => {
                eprintln!("aether-compositor: connector {handle:?}: {err}");
                continue;
            }
        };
        if info.state() != connector::State::Connected || info.modes().is_empty() {
            continue;
        }
        let mode = info
            .modes()
            .iter()
            .find(|mode| mode.mode_type().contains(ModeTypeFlags::PREFERRED))
            .or_else(|| {
                info.modes()
                    .iter()
                    .max_by_key(|mode| mode.size().0 as u64 * mode.size().1 as u64)
            })
            .copied()
            .ok_or_else(|| "connected connector without modes".to_string())?;

        let crtc = info
            .current_encoder()
            .and_then(|encoder| device.get_encoder(encoder).ok())
            .and_then(|encoder| encoder.crtc())
            .or_else(|| resources.crtcs().first().copied())
            .ok_or_else(|| "no CRTC available for the connector".to_string())?;

        let name = format!("{}-{}", info.interface().as_str(), info.interface_id());
        return Ok((name, *handle, mode, crtc));
    }
    Err("no connected connector with modes (is the display switched on?)".to_string())
}

/// Render and queue one frame when the shared frame scheduler says one is due.
fn present_if_due(state: &mut AetherCompositor) -> Result<(), String> {
    // KMS still owns the previous frame: leave the damage queued so the next
    // tick retries instead of failing inside `queue_frame`.
    if state
        .drm
        .as_ref()
        .map(|drm| drm.pending.is_some())
        .unwrap_or(false)
    {
        return Ok(());
    }
    let Some(frame) = state.begin_frame() else {
        return Ok(());
    };
    let Some(mut drm) = state.drm.take() else {
        return Err("drm backend not initialised".to_string());
    };
    let outcome = drm.present(state, &frame);
    state.drm = Some(drm);

    match outcome {
        // Frame callbacks and markers wait for the page flip.
        Ok(true) => {
            if let Some(drm) = state.drm.as_mut() {
                drm.pending = Some(frame);
            }
        }
        // Empty frame: keep the output dirty so the next tick retries rather
        // than dropping the update.
        Ok(false) => state.damage_whole_output(),
        Err(err) => {
            eprintln!("aether-compositor: drm render: {err}");
            state.damage_whole_output();
        }
    }
    Ok(())
}

impl DrmBackend {
    /// Build the element list and hand it to the KMS compositor.
    ///
    /// `Ok(true)` = a frame was queued to KMS, `Ok(false)` = nothing visible
    /// changed (the compositor reported an empty frame).
    fn present(&mut self, state: &mut AetherCompositor, frame: &Frame) -> Result<bool, String> {
        let output_size = self.logical_size();
        let scale = self.output.current_scale().fractional_scale();
        let tier = state.effects.tier();

        // P3-T07: a single opaque full-screen window with no shell overlay is
        // scanned out from the client buffer; no glass passes are planned.
        let direct = state.scanout.is_direct();

        // P3-T05: capture the backdrop once per frame, over the union of the
        // glass surfaces. The Minimal tier needs no backdrop (fill mode).
        let backdrop = if direct || !glass_shader::tier_blurs(tier) {
            None
        } else {
            let background = collect_layers(
                &mut self.renderer,
                state,
                output_size,
                scale,
                BACKDROP_LAYERS,
            );
            let surfaces = glass_surfaces(state, output_size);
            match self.glass.as_mut() {
                Some(glass) => glass.backdrop(
                    &mut self.renderer,
                    &background,
                    &surfaces,
                    scale,
                    output_size,
                ),
                None => None,
            }
        };

        let elements = collect_elements(
            &mut self.renderer,
            self.glass.as_mut(),
            state,
            output_size,
            scale,
            &frame.damage,
            backdrop.as_ref(),
        );

        if frame.index <= 5 || frame.index % 120 == 0 {
            let frosted = elements
                .iter()
                .filter(|element| matches!(element, DrmRenderElement::Frost(_)))
                .count();
            if let Some(glass) = self.glass.as_ref() {
                println!(
                    "AETHER_COMPOSITOR_GLASS_SHADER: tier={} blur={} frosted={} frosted_total={} damage_px={} backdrop={}",
                    tier.name(),
                    if glass_shader::tier_blurs(tier) {
                        "dual-kawase"
                    } else {
                        "none"
                    },
                    frosted,
                    glass.surfaces,
                    frame.damage_pixels,
                    backdrop
                        .as_ref()
                        .map(|backdrop| format!(
                            "{}x{}",
                            backdrop.region.size.w, backdrop.region.size.h
                        ))
                        .unwrap_or_else(|| "none".to_string()),
                );
            }
        }

        if direct {
            self.direct_frames += 1;
            if frame.index <= 5 || frame.index % 120 == 0 {
                println!(
                    "AETHER_COMPOSITOR_SCANOUT_FRAME: n={} window={} damage_px={} passes=0",
                    frame.index,
                    state.scanout.direct_window().unwrap_or(0),
                    frame.damage_pixels
                );
            }
        }

        let clear = [0.0f32, 0.0, 0.0, 1.0];
        let rendered = self
            .compositor
            .render_frame(&mut self.renderer, &elements, clear, FrameFlags::DEFAULT)
            .map_err(|err| format!("render_frame: {err:?}"))?;
        if rendered.is_empty {
            return Ok(false);
        }
        // The result borrows the elements and the swapchain slot; drop it before
        // queueing so the buffer can be submitted.
        drop(rendered);

        self.compositor
            .queue_frame(FrameUserData {
                index: frame.index,
                time_ms: frame.time_ms,
            })
            .map_err(|err| format!("queue_frame: {err:?}"))?;
        self.frames_queued += 1;
        Ok(true)
    }

    fn logical_size(&self) -> (i32, i32) {
        let (w, h) = self
            .output
            .current_mode()
            .map(|mode| (mode.size.w, mode.size.h))
            .unwrap_or((self.mode.size.w, self.mode.size.h));
        let scale = self.output.current_scale().fractional_scale();
        (
            (w as f64 / scale).round() as i32,
            (h as f64 / scale).round() as i32,
        )
    }
}

/// Layer surfaces that form the backdrop of the glass surfaces.
const BACKDROP_LAYERS: [Layer; 2] = [Layer::Background, Layer::Bottom];
/// Layer surfaces drawn above the windows (shell chrome: panel, dock, …).
const CHROME_LAYERS: [Layer; 2] = [Layer::Top, Layer::Overlay];

/// Back-to-front render elements for the active workspace.
///
/// Windows and shell-chrome surfaces get a frosted backdrop element drawn
/// underneath their content (P3-T05); everything else is composited plainly.
fn collect_elements(
    renderer: &mut GlesRenderer,
    mut glass: Option<&mut GlassPasses>,
    state: &AetherCompositor,
    output_size: (i32, i32),
    scale: f64,
    damage: &[Rect],
    backdrop: Option<&Backdrop>,
) -> Vec<DrmRenderElement> {
    let mut elements = Vec::new();

    // 1. Background/bottom layer surfaces (wallpaper, backdrop).
    for element in collect_layers(renderer, state, output_size, scale, BACKDROP_LAYERS) {
        elements.push(DrmRenderElement::Surface(element));
    }

    // 2. Managed windows of the active workspace, frosted underneath.
    for (index, toplevel) in state.toplevels.iter().enumerate() {
        let Some(window) = state.wm.windows.get(&WindowId(index as u64)) else {
            continue;
        };
        if window.workspace != state.wm.active || !toplevel.alive() {
            continue;
        }
        if let Some(glass) = glass.as_mut() {
            push_frost(
                renderer,
                glass,
                state,
                window.rect,
                damage,
                scale,
                backdrop,
                &mut elements,
            );
        }
        if let Some(element) = surface_element(
            renderer,
            toplevel.wl_surface(),
            physical(window.rect.x, window.rect.y, scale),
            Kind::Unspecified,
        ) {
            elements.push(DrmRenderElement::Surface(element));
        }
    }

    // 3. Top/overlay layer surfaces (panel, dock, notifications, lock).
    for entry in state
        .layer_surfaces
        .iter()
        .filter(|entry| CHROME_LAYERS.contains(&entry.layer))
    {
        if let Some(glass) = glass.as_mut() {
            push_frost(
                renderer,
                glass,
                state,
                layer_rect(entry, output_size),
                damage,
                scale,
                backdrop,
                &mut elements,
            );
        }
        if let Some(element) = layer_element(renderer, entry, output_size, scale) {
            elements.push(DrmRenderElement::Surface(element));
        }
    }

    // 4. Software cursor from the client's cursor surface.
    if let smithay::input::pointer::CursorImageStatus::Surface(surface) = &state.cursor_status {
        let location = physical(
            state.pointer_location.x.round() as i32,
            state.pointer_location.y.round() as i32,
            scale,
        );
        if let Some(element) = surface_element(renderer, surface, location, Kind::Cursor) {
            elements.push(DrmRenderElement::Surface(element));
        }
    }

    elements
}

/// Resolve the glass plan for one surface and push its frosted element.
fn push_frost(
    renderer: &mut GlesRenderer,
    glass: &mut GlassPasses,
    state: &AetherCompositor,
    rect: Rect,
    damage: &[Rect],
    scale: f64,
    backdrop: Option<&Backdrop>,
    elements: &mut Vec<DrmRenderElement>,
) {
    let material = state.effects.material();
    let surface = GlassSurface {
        id: 0,
        rect,
        corner_radius: material.corner_radius,
        opacity: 1.0,
    };
    let plan = state.effects.plan(&surface, Some(damage));
    if let Some(frost) = glass.frost(renderer, backdrop, &material, &plan, rect, damage, scale) {
        elements.push(frost.element(renderer.context_id()));
    }
}

/// Logical rects of the surfaces that receive the glass material.
fn glass_surfaces(state: &AetherCompositor, output_size: (i32, i32)) -> Vec<Rect> {
    let mut rects = Vec::new();
    for (index, toplevel) in state.toplevels.iter().enumerate() {
        if !toplevel.alive() {
            continue;
        }
        if let Some(window) = state.wm.windows.get(&WindowId(index as u64)) {
            if window.workspace == state.wm.active {
                rects.push(window.rect);
            }
        }
    }
    for entry in state
        .layer_surfaces
        .iter()
        .filter(|entry| CHROME_LAYERS.contains(&entry.layer))
    {
        rects.push(layer_rect(entry, output_size));
    }
    rects
}

/// Elements of the layer surfaces in `layers`, back to front.
fn collect_layers(
    renderer: &mut GlesRenderer,
    state: &AetherCompositor,
    output_size: (i32, i32),
    scale: f64,
    layers: [Layer; 2],
) -> Vec<WaylandSurfaceRenderElement<GlesRenderer>> {
    state
        .layer_surfaces
        .iter()
        .filter(|entry| layers.contains(&entry.layer))
        .filter_map(|entry| layer_element(renderer, entry, output_size, scale))
        .collect()
}

/// Logical rect of a layer surface on the primary output.
fn layer_rect(entry: &LayerSurfaceEntry, output_size: (i32, i32)) -> Rect {
    let surface = entry.surface.wl_surface();
    let cached = with_states(surface, |states| {
        states
            .cached_state
            .get::<LayerSurfaceCachedState>()
            .current()
            .clone()
    });
    let size = crate::layer_surface_size(&cached, Size::from(output_size));
    let (x, y) = layer_location(layer_anchors(cached.anchor), (size.w, size.h), output_size);
    Rect {
        x,
        y,
        w: size.w,
        h: size.h,
    }
}

fn layer_element(
    renderer: &mut GlesRenderer,
    entry: &LayerSurfaceEntry,
    output_size: (i32, i32),
    scale: f64,
) -> Option<WaylandSurfaceRenderElement<GlesRenderer>> {
    let rect = layer_rect(entry, output_size);
    surface_element(
        renderer,
        entry.surface.wl_surface(),
        physical(rect.x, rect.y, scale),
        Kind::Unspecified,
    )
}

fn layer_anchors(anchor: Anchor) -> (bool, bool, bool, bool) {
    (
        anchor.contains(Anchor::LEFT),
        anchor.contains(Anchor::RIGHT),
        anchor.contains(Anchor::TOP),
        anchor.contains(Anchor::BOTTOM),
    )
}

/// Logical -> physical (buffer) coordinates of the output.
fn physical(x: i32, y: i32, scale: f64) -> (i32, i32) {
    (
        (x as f64 * scale).round() as i32,
        (y as f64 * scale).round() as i32,
    )
}

fn surface_element(
    renderer: &mut GlesRenderer,
    surface: &WlSurface,
    location: (i32, i32),
    kind: Kind,
) -> Option<WaylandSurfaceRenderElement<GlesRenderer>> {
    let point = Point::<f64, Physical>::from((location.0 as f64, location.1 as f64));
    with_states(surface, |states| {
        WaylandSurfaceRenderElement::from_surface(renderer, surface, states, point, 1.0, kind)
            .ok()
            .flatten()
    })
}

/// Handle one VBlank: the queued frame reached the screen.
fn on_vblank(state: &mut AetherCompositor) {
    let Some(drm) = state.drm.as_mut() else {
        return;
    };
    match drm.compositor.frame_submitted() {
        Ok(user_data) => {
            drm.vblanks += 1;
            if let Some(user_data) = user_data {
                if user_data.index <= 5 || user_data.index % 120 == 0 {
                    println!(
                        "AETHER_COMPOSITOR_DRM_VBLANK: n={} time_ms={} total={}",
                        user_data.index, user_data.time_ms, drm.vblanks
                    );
                }
            }
        }
        Err(err) => eprintln!("aether-compositor: frame_submitted: {err:?}"),
    }
    let pending = drm.pending.take();
    if let Some(frame) = pending {
        // Only now do clients get their wl_surface.frame callback, so their
        // throttling follows real presentation.
        state.complete_frame(&frame);
    }
}

/// Feed one libinput event into the Wayland seat.
fn process_input_event(state: &mut AetherCompositor, event: InputEvent<LibinputInputBackend>) {
    let Some(seat) = state.seat.clone() else {
        return;
    };
    match event {
        InputEvent::Keyboard { event } => {
            let Some(keyboard) = seat.get_keyboard() else {
                return;
            };
            let serial = state.serials.next_serial();
            let time = event.time_msec();
            let keycode = event.key_code();
            let key_state = event.state();
            if key_state == KeyState::Pressed {
                println!("AETHER_COMPOSITOR_INPUT_KEY: code={keycode:?} state=pressed");
            }
            keyboard.input::<(), _>(state, keycode, key_state, serial, time, |_, _, _| {
                FilterResult::Forward
            });
        }
        InputEvent::PointerMotion { event } => {
            let Some(pointer) = seat.get_pointer() else {
                return;
            };
            let delta = event.delta();
            let location = clamp_to_output(state, state.pointer_location + delta);
            state.pointer_location = location;
            let focus = pointer_focus(state, location);
            let serial = state.serials.next_serial();
            let time = event.time_msec();
            pointer.motion(
                state,
                focus,
                &MotionEvent {
                    location,
                    serial,
                    time,
                },
            );
            pointer.frame(state);
            state.damage_pointer();
        }
        InputEvent::PointerButton { event } => {
            let Some(pointer) = seat.get_pointer() else {
                return;
            };
            let serial = state.serials.next_serial();
            let time = event.time_msec();
            let button = event.button_code();
            let button_state = event.state();
            println!(
                "AETHER_COMPOSITOR_INPUT_BUTTON: button={button} state={}",
                if button_state == ButtonState::Pressed {
                    "pressed"
                } else {
                    "released"
                }
            );
            // Click-to-focus: the surface under the pointer takes the keyboard.
            if button_state == ButtonState::Pressed {
                if let Some(keyboard) = seat.get_keyboard() {
                    if let Some((surface, _)) = pointer_focus(state, state.pointer_location) {
                        keyboard.set_focus(state, Some(surface), serial);
                    }
                }
            }
            pointer.button(
                state,
                &ButtonEvent {
                    serial,
                    time,
                    button,
                    state: button_state,
                },
            );
            pointer.frame(state);
        }
        InputEvent::PointerAxis { event } => {
            let Some(pointer) = seat.get_pointer() else {
                return;
            };
            let time = event.time_msec();
            let frame = AxisFrame::new(time)
                .source(event.source())
                .value(
                    Axis::Horizontal,
                    event.amount(Axis::Horizontal).unwrap_or(0.0),
                )
                .value(Axis::Vertical, event.amount(Axis::Vertical).unwrap_or(0.0));
            pointer.axis(state, frame);
            pointer.frame(state);
        }
        _ => {}
    }
}

/// Clamp the pointer into the primary output's logical area.
fn clamp_to_output(state: &AetherCompositor, location: Point<f64, Logical>) -> Point<f64, Logical> {
    let (w, h) = state
        .outputs
        .first()
        .and_then(|output| output.current_mode())
        .map(|mode| (mode.size.w as f64, mode.size.h as f64))
        .unwrap_or((1920.0, 1080.0));
    Point::from((
        location.x.clamp(0.0, (w - 1.0).max(0.0)),
        location.y.clamp(0.0, (h - 1.0).max(0.0)),
    ))
}

/// Topmost window or layer surface under a logical position, with the position
/// translated into surface-local coordinates.
fn pointer_focus(
    state: &AetherCompositor,
    location: Point<f64, Logical>,
) -> Option<(WlSurface, Point<f64, Logical>)> {
    let point = (location.x as i32, location.y as i32);
    let (ow, oh) = state
        .outputs
        .first()
        .and_then(|output| output.current_mode())
        .map(|mode| (mode.size.w, mode.size.h))
        .unwrap_or((1920, 1080));

    // Shell chrome (panel, dock, overlay) sits above the windows. Background
    // and Bottom layers are wallpaper: they never take input.
    for entry in state
        .layer_surfaces
        .iter()
        .rev()
        .filter(|entry| matches!(entry.layer, Layer::Top | Layer::Overlay))
    {
        let surface = entry.surface.wl_surface();
        let cached = with_states(surface, |states| {
            states
                .cached_state
                .get::<LayerSurfaceCachedState>()
                .current()
                .clone()
        });
        let size = crate::layer_surface_size(&cached, Size::from((ow, oh)));
        let (x, y) = layer_location(layer_anchors(cached.anchor), (size.w, size.h), (ow, oh));
        if point.0 >= x && point.0 < x + size.w && point.1 >= y && point.1 < y + size.h {
            return Some((
                surface.clone(),
                Point::from(((point.0 - x) as f64, (point.1 - y) as f64)),
            ));
        }
    }

    // Then managed windows, topmost last.
    for (index, toplevel) in state.toplevels.iter().enumerate().rev() {
        let Some(window) = state.wm.windows.get(&WindowId(index as u64)) else {
            continue;
        };
        if window.workspace != state.wm.active || !toplevel.alive() {
            continue;
        }
        let rect: Rect = window.rect;
        if point.0 >= rect.x
            && point.0 < rect.x + rect.w
            && point.1 >= rect.y
            && point.1 < rect.y + rect.h
        {
            return Some((
                toplevel.wl_surface().clone(),
                Point::from(((point.0 - rect.x) as f64, (point.1 - rect.y) as f64)),
            ));
        }
    }
    None
}
