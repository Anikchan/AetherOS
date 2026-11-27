# UI image packaging — the graphical session in the bootable image

- Date: 2026-11-27/28 (overnight run)
- Session: `ui-image` (critical path)
- Status: **wiring complete and statically verified; the image build is the
  proof and is owned by S1** (CI `buildroot-build` runs on the push).
- Related: `docs/2026/11/27/results/ui-readiness-status.md` (the gap this task
  closes), `docs/2026/11/27/overnight-parallel-plan.md` (workstream table),
  `os-build/buildroot/ui/README.md` (packaging contract).

## 1. What was wrong before this change

| Gap | Evidence |
| --- | --- |
| The image shipped no Rust binary at all | `grep -rn aether- os-build/buildroot/aetheros_defconfig` matched nothing; the overlay's only added file was `root/ test.sh`. |
| No fonts and no font stack, so a copied Slint binary would abort at startup | no `fontconfig`, no `freetype`, no font package in the defconfig. |
| No seat library for the compositor's DRM backend | no `libseat`/`seatd`. |
| **Mesa was silently dropped** | `BR2_PACKAGE_MESA3D` has `depends on BR2_INSTALL_LIBSTDCPP`; only `BR2_TOOLCHAIN_BUILDROOT_CXX` selects it, and the defconfig never set it. This is why `glxinfo`/`vulkaninfo` were absent from the P2 image (`p2-hardware-validation-run2.md`, "not in image"). |
| The console getty owned tty1 | to run KMS the compositor needs the VT, and the console login had to move so a broken graphical start still leaves a login. |
| No session unit at all | `boot` reached `multi-user.target` and stopped there. |

## 2. defconfig lines added

All of these are in `os-build/buildroot/aetheros_defconfig`.

```diff
+# C++ (libstdc++) is required by Mesa3d in 2025.11 ...
+BR2_TOOLCHAIN_BUILDROOT_CXX=y

+# The console getty moves to tty2 (2026-11-28, UI image) ...
+BR2_TARGET_GENERIC_GETTY_PORT="tty2"

+# Runs before the filesystem images are made ...
+BR2_ROOTFS_POST_BUILD_SCRIPT="../../post-build.sh"

+# UI image: EGL/GLES/GBM for the compositor's DRM backend
+BR2_PACKAGE_MESA3D_GALLIUM_DRIVER_SOFTPIPE=y
+BR2_PACKAGE_MESA3D_OPENGL_EGL=y
+BR2_PACKAGE_MESA3D_OPENGL_ES=y
+BR2_PACKAGE_MESA3D_GBM=y

+# Graphical session runtime
+BR2_PACKAGE_FONTCONFIG=y
+BR2_PACKAGE_FREETYPE=y
+BR2_PACKAGE_DEJAVU=y
+BR2_PACKAGE_LIBXKBCOMMON=y
+BR2_PACKAGE_XKEYBOARD_CONFIG=y
+BR2_PACKAGE_SEATD=y
+BR2_PACKAGE_SEATD_BUILTIN=y

+# Silent-drop fix (WirePlumber): the version choice lives inside
+# `if BR2_PACKAGE_LUA`, so the interpreter itself must be enabled
+BR2_PACKAGE_LUA=y
+BR2_PACKAGE_LUA_5_4=y          (already present)
+BR2_PACKAGE_WIREPLUMBER=y      (already present)

+# hwdata (kernel-input request, P2 run 3: lsusb "unable to initialize usb spec")
+BR2_PACKAGE_HWDATA=y
+BR2_PACKAGE_HWDATA_USB_IDS=y

+# Boot switching (boot-switch session request)
+BR2_PACKAGE_EFIBOOTMGR=y
```

Untouched on purpose: `BR2_PACKAGE_LUA_5_4=y` (the WirePlumber fix) and
`BR2_PACKAGE_WIREPLUMBER=y` stay exactly as they were; `check-ui-image.sh`
fails if they disappear. **`BR2_PACKAGE_LUA=y` was added to them** — see the
next section.

### 2.1 Silent dependency drops (the class this image keeps tripping over)

Image build `46041689625` (commit `dabbb76`) still logged `Build wireplumber :
NO` although the defconfig already had `BR2_PACKAGE_LUA_5_4=y`. Cause:
`package/lua/Config.in` in 2025.11 puts the version choice **inside**
`if BR2_PACKAGE_LUA`, so with the interpreter off the version symbol does not
exist and olddefconfig deletes the line without a word. The chain is three
symbols deep:

```
BR2_PACKAGE_WIREPLUMBER -> BR2_PACKAGE_HAS_LUA_INTERPRETER
  -> BR2_PACKAGE_LUA (menu) -> BR2_PACKAGE_LUA_5_4 (version, inside the menu)
```

The fix is `BR2_PACKAGE_LUA=y` next to the version line, with the chain spelled
out in a comment in the defconfig. This is the same failure mode as Mesa
(`BR2_PACKAGE_MESA3D` needs `BR2_TOOLCHAIN_BUILDROOT_CXX`, §1) and the getty
(`BR2_PACKAGE_SEATD_BUILTIN` needs `BR2_PACKAGE_SEATD`).

Two guards now exist in `os-build/scripts/check-ui-image.sh`:

1. **Always (macOS and Linux): a child/parent table.** Every sub-option the
   image uses (`LUA_5_4`, the Mesa EGL/GBM/gallium rows, `SEATD_BUILTIN`,
   `DEJAVU_*`, `ALSA_UTILS_*`, `BLUEZ5_UTILS_*`, `WPA_SUPPLICANT_NL80211`,
   `OPENSSH_SERVER/KEY_UTILS`, `HWDATA_*`, `LINUX_FIRMWARE_*`, …) is listed with
   the package menu it lives in; a child without its package fails the check.
   `BR2_PACKAGE_WIREPLUMBER` + `BR2_PACKAGE_LUA` + `BR2_PACKAGE_LUA_5_4` are also
   asserted by name.
2. **Authoritative on Linux: `--config <generated .config>`.** Every `=y` symbol
   in the defconfig must appear as `=y` in the `.config` Buildroot actually
   resolved. `os-build/post-build.sh` runs exactly this against `$PWD/.config`
   on every image build and records `wiring_guard: ok|failed` in
   `/etc/aether/ui-build.txt`, printing one `AETHER_UI_IMAGE_WARN: guard: …`
   line per dropped symbol. A drop is reported, not fatal, so the 2-hour image
   still lands; the annotation makes it impossible to miss.

Negative tests run in this session:

| Test | Result |
| --- | --- |
| Defconfig with `BR2_PACKAGE_LUA=y` removed | 3 failures: required symbol, WirePlumber chain, `LUA_5_4` child without parent |
| Synthetic `.config` with every defconfig `=y` symbol, minus the four known-inactive Mesa rows | `162 passed, 0 failed, 8 notes` |
| Same `.config` with `BR2_PACKAGE_SEATD_BUILTIN=y` removed | `FAIL BR2_PACKAGE_SEATD_BUILTIN=y … MISSING from the generated .config (silent drop)` |

### 2.2 Boot switching (wiring only, not implemented here)

The `boot-switch` session owns the implementation; this workstream only makes it
installable:

- `BR2_PACKAGE_EFIBOOTMGR=y` — 2025.11 dependencies are all met by this image
  (`EFIVAR_ARCH_SUPPORTS` on x86_64, `!BR2_STATIC_LIBS` with the glibc
  toolchain, threads, headers ≥ 3.12 for the 6.18 kernel, gcc ≥ 4.9); `efivar`
  and `popt` are selected by the package, nothing else is needed.
- `multi-user.target.wants/aether-boot-return.service` → `../aether-boot-return.service`
  (a real symlink in the overlay, so rsync copies it as-is).
- `check-ui-image.sh` asserts the wants symlink **and** the unit file (plus that
  the unit has an `ExecStart`, and that `efibootmgr` is in the image).

The unit file itself is written by the other session; until that commit lands
the checker reports one FAIL naming the missing unit, which is deliberate (a
wants symlink without its unit is exactly the silent non-feature this guard is
for). Nothing else in this change depends on it.

### 2.3 hwdata (kernel-input request)

`BR2_PACKAGE_HWDATA=y` + `BR2_PACKAGE_HWDATA_USB_IDS=y`: P2 run 3 saw `lsusb`
print "unable to initialize usb spec" because the image had no
`/usr/share/hwdata/usb.ids`. `BR2_PACKAGE_HWDATA_PCI_IDS` defaults to `y`, so
`lspci` gains its database too.

Version notes (Buildroot **2025.11**, checked against the package `Config.in`
files on `gitlab.com/buildroot.org/buildroot/-/raw/2025.11/`):

- `BR2_TOOLCHAIN_BUILDROOT_CXX` is the user-visible symbol (`package/gcc/Config.in.host:84`);
  `BR2_INSTALL_LIBSTDCPP` itself is hidden and cannot be set from a defconfig.
- `BR2_PACKAGE_SEATD` provides `libseat` (`package/seatd/Config.in`, seatd 0.9.1).
  With no logind and no daemon, `BR2_PACKAGE_SEATD_BUILTIN` is what libseat uses
  at runtime; `BR2_PACKAGE_SEATD` must be set for the sub-option to exist.
- `BR2_PACKAGE_XKEYBOARD_CONFIG` lives under `package/x11r7/` in 2025.11; it
  installs `/usr/share/X11/xkb`, without which `libxkbcommon` cannot compile a
  keymap.
- **LLVM consequence (important):** in 2025.11 `iris`, `radeonsi`, ANV and RADV
  all `depends on BR2_PACKAGE_MESA3D_LLVM`. They are left in the file but are
  inactive (the check script prints them as NOTES). Enabling LLVM would add
  roughly half an hour to the image build and a large amount of image size, so
  the image ships `softpipe` — a no-LLVM gallium driver, which gives
  `EGL_PLATFORM_GBM` → `kms_swrast` software rendering over the i915/amdgpu KMS
  plane. Hardware EGL on the Dell's Iris Xe needs the owner to accept
  `BR2_PACKAGE_MESA3D_LLVM=y` (decision recorded as a residual risk, §7).
- `BR2_PACKAGE_MESA3D_VULKAN_DRIVER_AMD` (pre-existing line) is **not a 2025.11
  symbol**; there is no RADV entry. Kconfig ignores it; the check script prints
  a note. Left untouched because Vulkan is outside this workstream.

## 3. Files created / changed

| Path | Purpose |
| --- | --- |
| `os-build/buildroot/aetheros_defconfig` | Symbols in §2. |
| `os-build/post-build.sh` *(new)* | `BR2_ROOTFS_POST_BUILD_SCRIPT`: installs a Rust toolchain if absent, installs the same dev packages `ci.yml` uses, runs `cargo build --release` for the compositor (`--features drm`, one headless retry), the shell (also yields `aether-greeter`) and the apps, copies the binaries into `$TARGET_DIR/usr/bin`, writes `/etc/aether/ui-build.txt` and prints `AETHER_UI_IMAGE*` markers + GitHub annotations. |
| `os-build/buildroot/ui/binaries.txt` *(new)* | The install list: `package<TAB>binary<TAB>core|app`. Single source of truth for the post-build step and the checker. |
| `os-build/buildroot/ui/README.md` *(new)* | Why this is a post-build step and not a cargo package, and how to migrate later. |
| `os-build/board/aetheros/overlay/usr/bin/aether-session` *(new, 0755)* | Session entry point: exports `AETHER_COMPOSITOR_BACKEND=drm`, `LIBSEAT_BACKEND=builtin`, `XDG_RUNTIME_DIR=/run/aether`, `AETHER_COMPOSITOR_LOG=/var/log/aether/compositor.log`, creates the runtime dir, `exec`s `/usr/bin/aether-compositor`. |
| `os-build/board/aetheros/overlay/usr/bin/aether-shell-host` *(new, 0755)* | Shell entry point: probes `aether-shell --help` for `--connect`, waits ≤ 20 s for `/run/aether/wayland-0`, then runs `--connect --hold-ms 86400000`; falls back to `--interactive --journal …` when the binary predates the surface host. |
| `os-build/board/aetheros/overlay/etc/systemd/system/aether-session.service` *(new)* | The compositor on tty1 (details in §4). |
| `os-build/board/aetheros/overlay/etc/systemd/system/aether-shell.service` *(new)* | The shell host, ordered after the compositor. |
| `os-build/board/aetheros/overlay/etc/systemd/system/multi-user.target.wants/aether-session.service` *(new symlink)* | Enables the session. |
| `os-build/board/aetheros/overlay/etc/systemd/system/getty@tty1.service` *(new symlink → `/dev/null`)* | Masks the tty1 getty so the kernel-console getty generator cannot put a login on the compositor's VT. |
| `os-build/board/aetheros/overlay/etc/systemd/system/getty.target.wants/getty@tty2.service` *(new symlink; replaces the tty1 one)* | Console login on tty2. |
| `os-build/board/aetheros/overlay/etc/systemd/system/show-ip.service` *(edited)* | The harness address dump moved from `/dev/tty1` to `/dev/tty2` (tty1 now belongs to KMS). |
| `os-build/scripts/check-ui-image.sh` *(new, 0755)* | Static assertions, macOS-safe (§6); includes the silent-drop guard (§2.1) and the boot-switch wiring checks (§2.2). |
| `os-build/board/aetheros/overlay/etc/systemd/system/multi-user.target.wants/aether-boot-return.service` *(new symlink → `../aether-boot-return.service`)* | Enables the boot-switch unit owned by the other session (§2.2). |

Nothing outside the agreed territory was touched (no kernel fragment, no
`test.sh`, no Rust crate, no workflow).

## 4. Unit graph and startup order

```
multi-user.target  (reached first: sshd, tty2 getty, networkd, show-ip)
   └─ wants: aether-session.service          (After=multi-user.target)
        ├─ ExecStart=/usr/bin/aether-session  → aether-compositor (DRM/KMS on tty1)
        ├─ ConditionPathExists=/dev/dri       → skipped on CI QEMU / containers
        ├─ Conflicts=getty@tty1.service       → tty1 has no login
        └─ wants: aether-shell.service        (After=, PartOf=aether-session)
             └─ ExecStart=/usr/bin/aether-shell-host
                  → waits for /run/aether/wayland-0
                  → aether-shell --connect --hold-ms 86400000
                    (fallback: aether-shell --interactive --journal …)
```

Ordering rationale:

1. **Nothing blocks boot, and no ordering cycle is possible.** Both units are
   `WantedBy=multi-user.target`; a `Wants=` edge is *not* an ordering edge, so
   the target cannot be ordered after them. `aether-session.service` is ordered
   `After=systemd-user-sessions.service systemd-udev-settle.service` instead of
   `After=multi-user.target`:
   - `systemd-user-sessions.service` is the conventional anchor for anything that
     runs with gettys (it flushes `/run/user` at boot);
   - `systemd-udev-settle.service` guarantees udev has created `/dev/dri/card*`
     before this unit's `ConditionPathExists=/dev/dri` is evaluated, so a modular
     DRM driver cannot make the session skip itself;
   - nothing orders *before* these units, so a slow settle or a failing session
     delays only the screen, never the console, SSH or DHCP.
   The earlier draft used `After=multi-user.target` (which systemd also does not
   treat as a cycle, because `Wants` carries no ordering), but that makes first
   paint wait for the whole boot and reads like a cycle to every reviewer. It is
   gone, and `check-ui-image.sh` now fails if it comes back.
   **A boot with a broken shell looks like this:** `systemctl --failed` lists
   `aether-session.service` (or it is skipped with "condition failed" when there
   is no `/dev/dri`), `multi-user.target` is still reached, `sshd` answers and
   tty2 (Ctrl+Alt+F2) prints `login:`.
2. **tty1 has no login.** `getty@tty1.service` is masked (`/dev/null`) and the
   kernel-console getty generator is neutralised by `Conflicts=` +
   `DefaultInstance=tty2` (`BR2_TARGET_GENERIC_GETTY_PORT="tty2"`). tty2 is
   enabled with `getty.target.wants/getty@tty2.service -> /usr/lib/systemd/system/getty@.service`:
   the *instance* comes from the link name, and pointing at the template is
   exactly what `systemctl enable getty@tty2.service` creates (a link to
   `getty@tty2.service` would dangle, since that unit file does not exist —
   systemd resolves the name through the template either way, which is why the
   old tty1 link worked on the Dell, but this form is the canonical one).
3. **Fail-soft, and the retry cannot spin.** `Restart=on-failure`,
   `RestartSec=3`, `StartLimitBurst=3` / `StartLimitIntervalSec=120` in
   `[Unit]`: three attempts inside two minutes, then systemd refuses further
   starts (`start request repeated too quickly`), leaves the unit failed and the
   machine is a console + SSH box. The shell cannot take the compositor down
   (`Wants=`, not `Requires=`) and vice versa: `PartOf=aether-session.service` +
   `Restart=always` means the shell is stopped and restarted only when the
   session unit itself is stopped/restarted, which is bounded by the *session's*
   start limit — so the pair cannot ping-pong forever. The wrappers exit 127 with
   a readable message when a binary is missing.
4. **tty1 ownership.** `TTYPath=/dev/tty1`, `TTYReset/TTYVHangup/TTYVTDisallocate`,
   `StandardInput=tty` — the same contract `getty@.service` uses, which is what a
   VT switch and a foreground job on the VT need.
5. **Seat.** The unit is a root system service with no logind session, so
   `LIBSEAT_BACKEND=builtin` tells libseat to open `/dev/dri/card*` and the evdev
   nodes itself (Buildroot's `seatd` is built with the builtin backend). No
   seatd daemon, no logind dependency.
6. **Logs.** The compositor's stdout/stderr go to `/var/log/aether/compositor.log`
   (`LogsDirectory=aether`, tmpfs-backed `/var`); `journalctl -u aether-session`
   and `journalctl -u aether-shell` carry the rest.
7. **Writable state, and nothing expects `/root`.** The root is read-only
   squashfs, so the shell runs with `HOME=/var/lib/aether-shell` and
   `XDG_CONFIG_HOME=/var/lib/aether-shell/config` from
   `StateDirectory=aether-shell` (tmpfs). Verified against the code: the only
   persistent writes are `SettingsStore::save` (→ `$XDG_CONFIG_HOME/aether/…`),
   the no-hang start ledger (`StartLedger::path` → `store.root()/session.json`,
   same directory) and the layer-surface SHM files, which go to
   `XDG_RUNTIME_DIR` (`/run/aether`, owned by the compositor unit). `$HOME` is
   only *read* (`desktop.rs::search_dirs` looks for
   `$HOME/.local/share/applications`, and `/usr/share/applications` is searched
   regardless), so `/root` is never needed.
8. **Wrappers cannot disagree with the units.** Both wrappers are committed mode
   755 and the three shared paths are asserted to agree by `check-ui-image.sh`:
   compositor log (`StandardOutput=append:` = `AETHER_COMPOSITOR_LOG` = wrapper
   default), runtime dir (`RuntimeDirectory=aether` = `XDG_RUNTIME_DIR=/run/aether`
   = wrapper default) and `WAYLAND_DISPLAY=wayland-0` (shell unit = wrapper).

## 5. The route used: post-build on the build host (fallback, documented)

**Route taken: the documented fallback** — `BR2_ROOTFS_POST_BUILD_SCRIPT` runs
`cargo build --release` on the build host (ubuntu-22.04, glibc 2.35) and copies
the binaries into `$TARGET_DIR/usr/bin`. **Not** Buildroot's cargo
infrastructure.

Why:

1. `aether-compositor` must be built with `--features drm` *and* nothing else
   must be; Buildroot's cargo infra builds one package with one feature set.
2. The workspace is 25 path-dependent crates with a single `Cargo.lock`;
   Buildroot's cargo support expects a self-contained vendored crate set per
   package. Making that work needs a `cargo vendor` step inside Buildroot plus
   `BR2_PACKAGE_HOST_RUSTC` and a target-toolchain cargo config — a large,
   unverifiable change from the macOS dev host.
3. The image glibc (Buildroot 2025.11, glibc 2.41) is newer than the build host's
   (2.35), so the binaries load unchanged; CI already builds and tests this exact
   workspace with this exact dev-package set (`ci.yml`), so the build itself is a
   proven configuration.
4. Cost control: `CARGO_PROFILE_RELEASE_LTO=false` is set for the image build
   only. The workspace profile's `lto = true` would add 10+ minutes to a 2-core
   runner inside a 90-minute `make` step (the whole job is capped at 150
   minutes). Optimisation level and `strip = true` stay as they are.

Failure policy in the script (deliberate, so one bad crate cannot cost the
2-hour image):

- compositor: `--features drm` first, **one** retry without the feature
  (headless); the manifest records which one shipped.
- shell: a failure is reported but the build continues (the apps and the rest of
  the image are still worth having).
- apps: one bulk `cargo build`; a failure is reported and the binaries that exist
  are installed.
- **the image build fails only when none of `aether-compositor`,
  `aether-shell`, `aether-greeter` can be installed** — that is the regression
  this work exists to prevent ("image ships no Rust binary").

Every outcome is written to `/etc/aether/ui-build.txt` **in the image** and
printed as `AETHER_UI_IMAGE: RESULT: ok|partial|failed` plus
`::warning`/`::error` annotations in the Actions log, so a partial build is
visible without reading the whole log.

## 6. Verification (run in this session, on macOS)

| Command | Result |
| --- | --- |
| `sh -n os-build/post-build.sh` | OK |
| `sh -n os-build/board/aetheros/overlay/usr/bin/aether-session` | OK |
| `sh -n os-build/board/aetheros/overlay/usr/bin/aether-shell-host` | OK |
| `sh -n os-build/scripts/check-ui-image.sh` | OK |
| `XDG_RUNTIME_DIR=/tmp/aether-rt-test sh …/usr/bin/aether-session` | creates the runtime dir (`drwx------`), then `aether-session: /usr/bin/aether-compositor is missing (see /etc/aether/ui-build.txt)`, exit 127 — the fail-soft path taken on a card without the binary |
| `python3` INI parse of both unit files | both parse; `[Unit]`/`[Service]`/`[Install]` keys as intended |
| `sh os-build/scripts/check-ui-image.sh` | **88 passed, 1 failed, 4 notes** — the single failure is the not-yet-committed `aether-boot-return.service` unit (§2.2), owned by the `boot-switch` session; the wants symlink and `efibootmgr` assertions pass |
| `sh os-build/post-build.sh /tmp/aether-ui-target` with `CARGO_TARGET_DIR=/tmp/aether-ui-image-target` | see §6.1 |

### 6.1 What the post-build dry run proves

The dry run executes the whole script against a scratch `$TARGET_DIR` on the
macOS host with a scratch `CARGO_TARGET_DIR` (so the shared `target/` of the
other sessions is untouched):

```
CARGO_TARGET_DIR=/tmp/aether-ui-image-target \
  sh os-build/post-build.sh /tmp/aether-ui-target
```

Observed (log: `/tmp/post-build-test3.log`, exit 0):

- the missing-dev-package branch is taken (`pkg-config modules missing: libudev
  libseat libdrm gbm libinput`, then `no apt-get/sudo` → warning, not a
  failure),
- `cargo build --release -p aether-compositor --features drm` fails on macOS
  (the backend is Linux-only; the link needs `libxkbcommon`, which this host
  does not have) → the script retries **once** without the feature → that also
  fails on macOS for the same host reason → `AETHER_UI_IMAGE_ERROR` +
  `::error` annotation, and the run continues,
- `aether-shell` (with `aether-greeter`) and all eleven apps build,
- 13 binaries are copied into `…/usr/bin` (108 MB total on this host),
- `/etc/aether/ui-build.txt` is written with `result: partial`,
  `compositor_features: not-built`, `installed:` and `missing:  aether-compositor`,
- the script exits **0** because two session binaries shipped (fail-soft); it
  would exit 1 only when all three are missing.

This is the exact fallback path the Linux runner takes if the `drm` feature
fails there, exercised for real rather than argued on paper. On the Linux
runner `libxkbcommon-dev`/`libseat-dev`/… are installed by the script (the same
set `ci.yml` installs), so the `drm` build is expected to succeed there; that is
what `compositor_features: drm` in the build log confirms.

Wrapper and skip paths:

| Command | Result |
| --- | --- |
| `AETHER_SKIP_RUST_BUILD=1 sh os-build/post-build.sh <dir>` | `RESULT: skipped`, target root untouched, exit 0 |
| `XDG_RUNTIME_DIR=/tmp/aether-rt-test sh …/usr/bin/aether-session` | runtime dir created `0700`, then exit 127 with the missing-binary message |

### 6.2 What only the CI/Linux run can prove

macOS cannot run Buildroot, so these are **S1 checks on the build log** (they are
listed again in §8):

1. `BR2_ROOTFS_POST_BUILD_SCRIPT` is honoured: the log contains
   `Executing post-build script(s)` … `../../post-build.sh`.
2. The defconfig symbols survived `make defconfig`: the build log/size shows
   `fontconfig`, `freetype`, `dejavu`, `libxkbcommon`, `xkeyboard-config`,
   `seatd`, `mesa3d`, `hwdata`, `efibootmgr`, and the target has
   `/usr/share/X11/xkb`, `/usr/share/fonts/truetype/dejavu`,
   `/usr/lib/libEGL.so*`, `/usr/lib/libgbm.so*`, `/usr/lib/libseat.so*`,
   `/usr/share/hwdata/usb.ids`.
3. `AETHER_UI_IMAGE: RESULT: ok` and `compositor_features: drm` (not
   `partial`/`headless`).
4. `AETHER_UI_IMAGE: defconfig drop guard: every defconfig =y symbol resolved`
   (and `wiring_guard: ok` in `/etc/aether/ui-build.txt`). If it reports
   `guard: …` lines, a defconfig symbol was dropped — see §2.1.
5. `Build wireplumber : YES` in the log (image build `46041689625` logged `NO`):
   with `BR2_PACKAGE_LUA=y` this is the direct proof the Lua chain resolved.
6. `aether-boot-return.service` is enabled in the image:
   `ls -l /etc/systemd/system/multi-user.target.wants/aether-boot-return.service`
   and `which efibootmgr`.
7. The image budget gate (`≤ 150 MB` compressed) still passes — see the risk in
   §7.1.

## 7. Residual risks for the first boot on the Dell

### 7.1 Image size and build time (highest risk to CI, not to boot)

- Measured on the dry run: **13 binaries, 108 MB uncompressed** (`usr/bin`,
  macOS release build with `lto=false` and `strip=true`). Squashfs+xz should put
  ~30-35 MB of that into the compressed image.
- Adding `BR2_TOOLCHAIN_BUILDROOT_CXX=y` makes Mesa actually build for the first
  time, plus fontconfig/DejaVu/xkb data. Expect roughly **+35-45 MB compressed**
  in total.
- The workflow's hard gate is `rootfs.squashfs ≤ 150 MB` compressed and the last
  recorded number is P1's 15 MB (before P2's firmware, Mesa, PipeWire and
  XWayland). If the gate trips, the cheapest lever is this file: delete the
  `app` rows from `os-build/buildroot/ui/binaries.txt` (one line per binary) and
  leave the three `core` rows — that removes ~95 MB uncompressed in one edit.
  Raising the budget is a separate owner decision in `os-build/budgets.toml`.
- The Rust build happens inside the 90-minute `make` step. With `LTO=false` the
  workspace build took **~15 minutes** for a cold release build on this Mac
  (including ~2 minutes for the 11 apps); a 2-core runner should stay inside the
  step, but if it does not, the same `app`-row trim fixes it.

### 7.2 Hardware EGL is software rendering tonight

`softpipe` (no LLVM) is what EGL/GBM resolves to. Expect `AETHER_COMPOSITOR_DRM_RENDERER`
to name a software renderer and expect low frame rates; the first frame should
still appear. Enjoying hardware GLES on the Iris Xe needs
`BR2_PACKAGE_MESA3D_LLVM=y` (owner decision, §2).

### 7.3 The shell paints palette surfaces, not the Slint scene

`aether-shell --connect` maps one solid palette buffer per layer surface
(`shell/src/wayland.rs`), at the size the compositor configures. So the first
boot should show the shell's *geometry* (menu-bar strip, dock strip, background)
in five dark tones, not the rendered UI. Two independent reasons:

1. the surface buffers are single colours today;
2. even with content, the workspace's Slint build has no on-device window
   backend (`renderer-software` + `software-renderer-systemfonts` only), so the
   Slint scene is still produced by `aether-shell --render` to PNG.

Next step for the `shell` workstream: render the `MinimalSoftwareWindow` into the
SHM buffer instead of the palette fill. Nothing in this image wiring changes when
that lands.

### 7.4 libseat builtin on a VT

`LIBSEAT_BACKEND=builtin` assumes libseat can open `/dev/tty0`, take KMS and
activate tty1 as the process's VT. If `AETHER_COMPOSITOR_DRM_SESSION` is missing
from the log, try from tty2:

```sh
AETHER_COMPOSITOR_BACKEND=drm LIBSEAT_BACKEND=builtin \
  XDG_RUNTIME_DIR=/run/aether /usr/bin/aether-compositor
```

and record the error. Fallbacks (in order): run the session through a logind
session (`BR2_PACKAGE_SYSTEMD_LOGIND=y` + `PAMName=login`), or ship
`BR2_PACKAGE_SEATD_DAEMON=y` and let libseat talk to seatd.

### 7.5 Fonts and keyboard data

`fontconfig` + DejaVu cover the shell's text; `xkeyboard-config` provides the
keymap data. If the compositor logs an xkb error, check
`ls /usr/share/X11/xkb/rules`. Both are asserted by the check script at the
defconfig level and by §8.2 in the image.

### 7.6 Read-only root and tmpfs state

The shell's config lives in `/var/lib/aether-shell` (tmpfs) and the compositor
log in `/var/log/aether` (tmpfs): both vanish at reboot, which is intended.

## 8. First-boot checklist on the Dell (S1 / owner)

```sh
# 0. Unit graph on a real systemd (could not be run in the session: the Dell
#    was unreachable and there is no systemd on the macOS dev host).
systemd-analyze verify /etc/systemd/system/aether-session.service \
                       /etc/systemd/system/aether-shell.service \
                       /etc/systemd/system/aether-boot-return.service
systemd-analyze critical-chain aether-session.service
systemctl is-enabled aether-session.service aether-boot-return.service
systemctl is-enabled getty@tty2.service       # expect: enabled (instance via template)
systemctl is-enabled getty@tty1.service       # expect: masked

# 1. Did the graphical units even try?
systemctl status aether-session.service aether-shell.service --no-pager
journalctl -b -u aether-session -u aether-shell --no-pager

# 2. Did the compositor get DRM + EGL + the seat?
grep -E 'AETHER_COMPOSITOR_BACKEND|AETHER_COMPOSITOR_DRM_(SESSION|DEVICE|CONNECTOR|RENDERER|OUTPUT|VBLANK)' \
  /var/log/aether/compositor.log

# 3. Did the shell map its surfaces?
journalctl -b -u aether-shell --no-pager | grep -E 'AETHER_SHELL_|surface'
cat /etc/aether/ui-build.txt          # which binaries/features this image has

# 4. Fail-soft: the console login is on tty2 (Ctrl+Alt+F2), SSH untouched.
# 5. What the screen shows: five dark panels (shell geometry), no widgets yet.
# 6. Boot switching (boot-switch session owns the logic):
systemctl status aether-boot-return.service --no-pager
efibootmgr -v | head -5               # after the unit ran, BootNext should be Ubuntu
lsusb                                 # hwdata: no "unable to initialize usb spec"
```

Expected on the first boot: `RESULT: ok`, `compositor_features: drm`,
`AETHER_COMPOSITOR_BACKEND: drm`, a `_DRM_SESSION … backend=libseat` marker, an
output/connector marker for eDP-1, and a shell surface report. If EGL fails, the
compositor exits with its reason in `/var/log/aether/compositor.log` and tty2
still logs in.

## 9. Evidence summary (D6)

| # | Check | Command | Result |
| --- | --- | --- | --- |
| 1 | post-build shell syntax | `sh -n os-build/post-build.sh` | OK |
| 2 | session wrapper syntax | `sh -n …/usr/bin/aether-session` | OK |
| 3 | shell wrapper syntax | `sh -n …/usr/bin/aether-shell-host` | OK |
| 4 | checker syntax | `sh -n os-build/scripts/check-ui-image.sh` | OK |
| 5 | static image assertions | `sh os-build/scripts/check-ui-image.sh` | **88 passed, 1 failed, 4 notes** (the failure is the pending `aether-boot-return.service`, §2.2) |
| 5b | drop guard, negative | defconfig with `BR2_PACKAGE_LUA=y` removed | 3 failures (required symbol, WirePlumber chain, `LUA_5_4` without parent) |
| 5c | drop guard, `--config` | `--config` with a synthetic `.config` | clean: 162 passed / 0 failed; one symbol removed: caught as a silent drop |
| 6 | post-build end-to-end | `CARGO_TARGET_DIR=… sh os-build/post-build.sh /tmp/aether-ui-target` | exit 0, `RESULT: partial` (compositor cannot link on macOS), 13 binaries installed, manifest written |
| 7 | skip switch | `AETHER_SKIP_RUST_BUILD=1 sh os-build/post-build.sh …` | exit 0, `RESULT: skipped` |
| 8 | fail-soft wrapper | `XDG_RUNTIME_DIR=… sh …/usr/bin/aether-session` | exit 127 + readable message, runtime dir `0700` |
| 9 | unit files parse | `python3` INI read of both units | both parse, expected keys/sections |
| 10 | unit ↔ wrapper agreement | `check-ui-image.sh -v` | log path, runtime dir and `WAYLAND_DISPLAY` agree across both units and both wrappers |
| 11 | ordering guard | `check-ui-image.sh` | fails if `After=multi-user.target` returns; asserts the `systemd-user-sessions`/`udev-settle` ordering |
| 12 | getty instance marker | `check-ui-image.sh` | `getty.target.wants/getty@tty2.service` points at the `getty@.service` template |

Not verified here (cannot be): the Linux Buildroot build itself, the `drm`
feature link on Ubuntu, and anything on the Dell. Those are CI/§8 items, and are
declared as such rather than claimed.

**No `systemd-analyze verify` in this session:** the Dell is powered off/asleep
(and it is not a macOS tool), so unit-graph validation could not be run on a real
systemd. That check is on the first-boot list in §8 instead
(`systemd-analyze verify /etc/systemd/system/aether-*.service`, and
`systemd-analyze critical-chain aether-session.service`).
