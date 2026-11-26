# D6 — P3-T02 → P4 layer-shell host wiring (shell surfaces)

- Date: 2026-11-26
- Session: S3 (`shell/`, `ui/`, `apps/`, `docs/.../features/`)
- Task: wire the shell's layer-shell surfaces to the compositor host (P3-T02),
  verify bind/configure, headless test.
- Host: macOS development host (no `libwayland`/`libxkbcommon`; the compositor
  binary can only be linked and run on Linux)

## What the shell now owns

`shell/src/host.rs` is the shell-side half of the layer-shell contract:

| Piece | Meaning |
| --- | --- |
| `host::plan()` / `HostRequest` | One `zwlr_layer_surface_v1` request per descriptor in `shell/src/layers.rs`: namespace, layer enum (`Top`=2, `Overlay`=3), anchor bitfield (`top`=1 `bottom`=2 `left`=4 `right`=8), requested size, exclusive zone, keyboard interactivity (`exclusive`=1 for launcher/notifications/control centre). |
| `HostRequest::negotiate(output)` | Mirrors the compositor's `layer_surface_size`: anchored edges stretch to the output, a non-zero request wins, otherwise the axis fills the output. |
| `host::BindingReport::scan(log)` | Reads the compositor's own markers (`AETHER_COMPOSITOR_OUTPUT`, `..._LAYER_SURFACE`, `..._LAYER_CONFIGURE`) and reports per surface: bound, configured size, expected size, layer match. |
| `aether-shell --host-check <log|->` | Prints the report; exit 0 only when every surface bound, configured on the right layer, at the negotiated size. |
| `aether-shell --host-plan` | Prints the request plan (what the on-device host sends). |

## Verified

| Check | Command | Result |
| --- | --- | --- |
| Shell library tests | `cargo test -p aether-shell --lib` | 91 pass (6 new `host::tests`) |
| Layer-host integration | `cargo test -p aether-shell --test layer_host` | 5 pass (drives the real `aether-shell` binary) |
| Plan values | `cargo run -q -p aether-shell --bin aether-shell -- --host-plan` | five requests, namespaces `aether-shell-{menubar,dock,launcher,notifications,control-center}` |
| Complete log | `aether-shell --host-check <complete.log>` | `AETHER_SHELL_LAYER_HOST_OK`, exit 0 |
| Missing surface / wrong size | `aether-shell --host-check <broken.log>` | `AETHER_SHELL_LAYER_HOST_FAILED`, exit 1, names the surface and the expected size |
| HiDPI (2880×1800 @1.5 → 1920×1200 logical) | `--host-check -` with the log on stdin | menubar configured 1920×28; OK |

Recorded plan (`--host-plan`, default 0 means "fill the output"):

```text
LAYER_REQUEST name=menubar namespace=aether-shell-menubar layer=Top anchors=13 size=0x28 exclusive_zone=28 keyboard=0
LAYER_REQUEST name=dock namespace=aether-shell-dock layer=Top anchors=2 size=720x64 exclusive_zone=0 keyboard=0
LAYER_REQUEST name=launcher namespace=aether-shell-launcher layer=Overlay anchors=0 size=640x480 exclusive_zone=0 keyboard=1
LAYER_REQUEST name=notifications namespace=aether-shell-notifications layer=Top anchors=9 size=360x0 exclusive_zone=0 keyboard=1
LAYER_REQUEST name=control-center namespace=aether-shell-control-center layer=Top anchors=9 size=340x0 exclusive_zone=0 keyboard=1
AETHER_SHELL_LAYER_PLAN_OK
```

Recorded verification on a 1920×1080 log with one surface truncated:

```text
LAYER_HOST output=eDP-1 logical=1920x1080
LAYER_SURFACE name=menubar namespace=aether-shell-menubar bound=true configured=1920x28 expected=1920x28 ok=true
LAYER_SURFACE name=dock namespace=aether-shell-dock bound=true configured=720x64 expected=720x64 ok=true
LAYER_SURFACE name=launcher namespace=aether-shell-launcher bound=true configured=640x480 expected=640x480 ok=true
LAYER_SURFACE name=notifications namespace=aether-shell-notifications bound=true configured=none expected=360x1080 ok=false
LAYER_SURFACE name=control-center namespace=aether-shell-control-center bound=false configured=none expected=340x1080 ok=false
AETHER_SHELL_LAYER_HOST_PROBLEM: notifications bound but was never configured (no AETHER_COMPOSITOR_LAYER_CONFIGURE)
AETHER_SHELL_LAYER_HOST_PROBLEM: control-center was never created (no AETHER_COMPOSITOR_LAYER_SURFACE for namespace=aether-shell-control-center)
AETHER_SHELL_LAYER_HOST_FAILED
```

The same log completed for all five surfaces prints
`AETHER_SHELL_LAYER_HOST_OK` and exits 0.

Negotiated sizes on a 1920×1080 output (what the compositor configures):
menubar 1920×28 (exclusive zone 28), dock 720×64, launcher 640×480,
notifications 360×1080, control centre 340×1080.

## Why the socket-level bind is not claimed here

The host that creates the surfaces is a Wayland client. `libwayland` is not
available on the macOS development host (`brew` ships `wayland` for Linux only,
and the compositor does not link: `ld: library 'xkbcommon' not found`), so no
socket-level bind can be executed and observed on this host. The two halves meet
at the protocol:

- **Server side:** the compositor's own headless test
  (`compositor/tests/p3_t02_headless.rs`) creates a `zwlr_layer_shell_v1`
  surface against the real compositor and asserts the configure size and
  fractional scale.
- **Shell side:** this module emits the same protocol values and verifies the
  compositor's markers with the shipped binary, so the on-device run exercises
  exactly the code path tested here.

## Remaining (on-device)

1. Run `aether-shell --host-check <journalctl -u aether-compositor>` on the
   image and record the output (harness §1).
2. Promote spike S1's Slint layer-shell `Platform` into `shell/` when the image
   ships `libwayland-client` and the shell is launched inside the session
   (currently the shell runs headlessly: `--render`, `--status`, `--host-check`).
