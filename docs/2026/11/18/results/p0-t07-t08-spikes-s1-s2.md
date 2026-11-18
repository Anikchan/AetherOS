# Results — Spikes S1 and S2: graphics stack feasibility (P0-T07, P0-T08)

- Date: 2026-11-18 (implementation-3; graphics-spikes run 44236833280, green)
- Environment: GitHub ubuntu-latest Linux x86-64 VM, headless (no display);
  S1 hosted by the S2 spike compositor over a real Wayland socket.

## Spike S2 — minimal Smithay compositor, one client window: PASS

| Marker | Meaning |
| --- | --- |
| `S2_SOCKET_READY: wayland-0` | Smithay 0.7 (wayland_frontend only) created a listening Wayland socket headlessly |
| `CLIENT_WINDOW_MAPPED` (client, exit 0) | Test client created an xdg-toplevel with a 100×100 SHM buffer and completed the configure/commit cycle |
| `S2_CLIENT_MAPPED` | The compositor detected the client surface committed with a buffer attached (mapped) |

Scope note: this proves the compositor core — server, shell protocol
handling, surface mapping — without DRM/KMS or GPU. Visible presentation on
a real output is proven in the graphical VM session (P2 graphics bring-up),
per the owner's VM-first decision.

**ADR impact:** ADR-009 (Smithay) and ADR-016 validated at the core level;
the headless constraint of the original spike safeguard is retired for the
compositing path.

## Spike S1 — Slint layer-shell surface + blur region: PASS (with one recorded finding)

| Marker | Meaning |
| --- | --- |
| `S1_LAYERSHELL_SURFACE_OK` | Slint 1.19 (custom `Platform` + `SoftwareRenderer`) rendered a test UI into a `zwlr_layer_shell_v1` surface over pure-Rust `wayland-client`; initial commit → configure → ack → frame commit completed |
| `S1_BLUR_UNAVAILABLE: compositor does not advertise org_kde_kwin_blur_manager` | Expected finding: the minimal S2 host does not implement KDE blur, so the blur request path could not complete end-to-end |

Findings:
1. **Layer-shell works with Slint** — but not through winit (winit has no
   layer-shell support, rust-windowing/winit#2582). The working route is a
   custom Slint platform + software renderer; this is a supported, stable
   API path. **ADR-004 (Slint) confirmed; the iced fallback is NOT
   triggered.**
2. **Background blur** is requestable only on compositors advertising
   `org_kde_kwin_blur_manager`. Smithay can implement the KDE blur protocol
   as an extension — added to the AetherOS compositor backlog (P2 graphics
   work), so the shell's blur-region design stays viable.

## Fixed along the way (for future sessions)

- Protocol crates need `features = ["client"]`; the generated KDE-blur
  scanner output needs a direct `wayland-backend` dependency; `new_id` must
  be an `<arg>` (the scanner silently drops request attributes); SHM pools
  take `BorrowedFd` (`pool_fd.as_fd()`).
- Smithay `delegate_*!` macros are `#[macro_export]` at the crate root and
  xdg-shell dispatch requires a `SeatHandler` implementation even without a
  seat global.
- Local Linux type-checks work on the Mac (see session plan recipe); CI
  remains the runtime gate.
