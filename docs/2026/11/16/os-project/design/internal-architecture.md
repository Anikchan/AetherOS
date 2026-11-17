# D2 — Internal Architecture

Scope: system layers, components, interfaces, file layout, security model, packaging and updates, crash handling, and the decision log (ADRs). Process manager internals are in D3. Image build is in D4.

## 1. Layers

| Layer | Contents | Language | Who maintains |
| --- | --- | --- | --- |
| L0 Hardware | x86-64 UEFI machines | — | — |
| L1 Kernel | Linux LTS with custom config (ADR-001) | C | Upstream plus our config fragment |
| L2 Base | glibc, systemd minimal profile, core tools, Mesa, firmware subset, NetworkManager, PipeWire, UPower, power-profiles-daemon | C, Rust in parts | Buildroot recipes (ADR-002) |
| L3 Services | `nexa-lifecycled`, `nexa-powerd`, `nexa-confd`, `nexa-pkg`, `nexa-compat`, `nexa-updater`, `nexa-boot` | Rust | This project |
| L4 Compositor | `nexa-compositor` on Smithay (ADR-005) | Rust | This project |
| L5 Shell | `nexa-shell`, `nexa-settings`, `nexa-install` UI | Rust with Slint UI (ADR-004, pending spike S1) | This project |
| L6 Apps | Native, Flatpak, AppImage, Wine apps, cross-platform apps | Any | Third parties |

## 2. Components

| Component | Responsibility | Process type | Restart policy |
| --- | --- | --- | --- |
| `nexa-compositor` | Display output, input routing, windows, effects, XWayland | User session (compositor) | Restarted by session manager; session logged out on repeated crash |
| `nexa-shell` | Menu bar, dock, launcher, control center, notifications, overview | User session | Restarted on crash, max 3 times in 60 s |
| `nexa-lifecycled` | App states, cgroup management, hibernation, pressure killer (D3) | User service, with a privileged helper for cgroup writes | Restarted on crash; app states are rebuilt from cgroups |
| `nexa-powerd` | Power policy: profiles, device power saving, battery estimate, low-battery actions (D5) | System service (root only for sysfs writes, through a small helper) | Restarted on crash; reads current state on start |
| `nexa-confd` | Reads and validates configuration; notifies components | User service | Restarted on crash; keeps last valid config |
| `nexa-pkg` | Flatpak operations, AppImage integration | User service | On demand (socket activated) |
| `nexa-compat` | Wine prefixes, runtimes, per-app settings | User service | On demand |
| `nexa-updater` | A/B image updates, health checks (ADR-011) | System service | Runs on timer and on demand |
| `nexa-boot` | Boot entry management, Windows detection | System service and installer tool | On demand |
| `nexactl` | Command-line control | Command | Not a service |

## 3. Interfaces and IPC

| Interface | Transport | Used by | Notes |
| --- | --- | --- | --- |
| Wayland protocols | Unix socket | Apps, shell, compositor | Core, XDG shell, layer shell, plus custom protocol `nexa_lifecycle_v1` for focus and window state |
| D-Bus (session bus) | Unix socket | Freedesktop services, portals, notifications, tray (StatusNotifierItem), MPRIS | Standard Linux desktop interfaces |
| `nexa-lifecycled` control | Unix domain socket (`$XDG_RUNTIME_DIR/nexa/lifecycle.sock`), JSON messages | Shell, `nexactl`, apps via SDK | Versioned messages |
| `nexa-confd` | Unix domain socket and file watch | All user services | Change notifications |
| Portals | D-Bus (xdg-desktop-portal) | Sandboxed apps | Background, file chooser, screenshot, notification |
| PSI and cgroups | Files under `/proc` and `/sys/fs/cgroup` | `nexa-lifecycled` | Kernel interfaces (D3) |

Rules:
- No component parses another component's private files. Use the socket or file interface.
- All IPC messages carry a version field.

## 4. File Layout

| Path | Contents | Writable by |
| --- | --- | --- |
| `/usr` | System binaries and libraries (read-only squashfs) | Image build only |
| `/usr/share/nexa/defaults` | Default configuration and themes | Image build only |
| `/etc/nexa` | System overrides | Administrator |
| `/var` | Logs, state, caches (overlay, writable) | Services |
| `/home/<user>` | User data | User |
| `~/.config/nexa` | User configuration (F4) | User |
| `~/.local/share/nexa` | User state (hibernation index, Wine prefixes index) | `nexa` services |
| `$XDG_RUNTIME_DIR/nexa` | Sockets and runtime state | `nexa` services |

## 5. Security Model

| Area | Design |
| --- | --- |
| Boot | Signed unified kernel image; Secure Boot path with shim (D4 section 5) |
| System integrity | Read-only system partition; updates replace the whole image (A/B) |
| App isolation | Flatpak sandbox (bubblewrap); portals for files, camera, screenshots, background access |
| Privileges | Services run as the user where possible. Only `nexa-updater`, `nexa-boot`, and the cgroup helper run as root, each with the minimum capabilities |
| Network | Firewall default deny inbound (nftables); outbound allowed |
| Encryption | Optional LUKS2 for the user data partition (F5 INS-08) |
| Updates | Signed bundles; rollback on failed health check |
| Code review | Installer and lifecycle daemon fuzzed in P9 |

## 6. Packaging and Updates

| Item | Method | Reason |
| --- | --- | --- |
| System image | A/B partitions, full image per update, RAUC (ADR-011) | Simple, safe rollback |
| User apps | Flatpak (primary), AppImage | Sandboxed, independent of system image |
| Wine and runtimes | Installed by `nexa-compat` into the user's data directory | Keeps base image small |
| Configuration | Files in `/usr/share/nexa/defaults` merged with user files | Survives updates |

## 7. Stability and Crash Handling

- Every service logs to the journal with a component tag.
- Repeated crashes (above the restart limit) switch the component to a safe mode and show a notification.
- The shell has a safe-mode option that loads default configuration only. Triggered by the user or after three failed starts.
- Crash reports are stored locally. No upload without the user's consent.

## 8. Extension API (v1 outline)

- Extensions are Lua scripts or small QML plugins in `~/.config/nexa/extensions/`.
- Extensions declare their permissions in a manifest (for example, "read config", "add menu item").
- Extensions cannot access the compositor directly. They use the shell API.
- Full API document is written in P7-T05.

## 9. Decision Log (ADRs)

Status values: Proposed, Accepted, Changed, Rejected. All decisions start as Proposed until the owner review in P0-T02.

| ID | Decision | Status | Reason | Alternatives | Revisit when |
| --- | --- | --- | --- | --- | --- |
| ADR-001 | Use Linux LTS kernel with custom configuration | Proposed | Drivers, cgroup v2, PSI, zram, and EFI support already exist | Own kernel (rejected) | Never, unless the hardware target changes |
| ADR-002 | Build the base image with Buildroot | Proposed | Small, reproducible, simple configuration | Debian minimal, Arch | If the image grows past budget (D4) |
| ADR-003 | Use systemd (minimal profile) as init for MVP | Proposed | cgroup v2 and unit management are built in | runit, s6 | P1-T02 measures size and boot time |
| ADR-004 | Shell UI in Slint (Rust) | Proposed | Rust-native, small runtime, one language. Spike S1 (P0-T07) must confirm layer-shell and blur support | iced with layer-shell (fallback), Qt 6 QML, GTK4 | Spike S1 fails, or shell memory exceeds the budget in F6 during P4 |
| ADR-005 | Compositor based on Smithay (Rust) | Proposed | Memory safety, modern Wayland support | wlroots (C) | If Smithay gaps block P3 features |
| ADR-006 | Lifecycle daemon written in Rust | Proposed | Safety and low overhead | C | Never, unless performance fails |
| ADR-007 | Configuration in TOML; scripts in Lua | Proposed | Readable and small; embeddable scripting | JSON, Python | Extension needs |
| ADR-008 | Flatpak for user apps; AppImage second | Proposed | Sandbox, Flathub, portals | Snap | Store policy changes |
| ADR-009 | Windows apps through Wine and Proton, managed by `nexa-compat` | Proposed | Legal, no Windows license needed | VM | Compatibility results in P6 |
| ADR-010 | Boot with systemd-boot and signed UKI | Proposed | Detects Windows Boot Manager; small | GRUB | Secure Boot issues found in P8 |
| ADR-011 | A/B updates with RAUC | Proposed | Simple and proven rollback | OSTree | Update size or tooling problems in P9 |
| ADR-012 | OS name "AetherOS" and component prefix | Accepted (owner, 2026-11-17) | Owner chose AetherOS as the final OS name. Component names keep the "nexa-" prefix as a placeholder until code starts, then rename to "aether-". | Trademark check before public release (P0-T03b) | Final name chosen by owner |
| ADR-013 | Rust is the primary language for all custom code | Proposed | Memory safety without a garbage collector; small binaries; low resource use; one language to maintain; strong open-source ecosystem | C, C++, Go | Only if a component cannot meet its budget |
| ADR-014 | Power policy (`nexa-powerd`) uses UPower and power-profiles-daemon as mechanisms | Proposed | Reuses standard battery and profile services; our policy is the differentiator | Own battery and profile stack | The services cannot provide data that F7 needs |
| ADR-015 | Personal Edition first; public-release tasks marked Deferred | Accepted (owner, 2026-11-16) | Owner answered Q3 (personal use now) and Q4 (one person, 26 hours per week) | Public release first | Owner decides to release publicly |
| ADR-016 | Target x86-64 only; legacy BIOS boot included in Personal Edition | Proposed | "Any system" (Q2) includes older PCs without UEFI. Apple Silicon is out of scope (Q5); Windows-on-ARM is deferred to future scope (Q8: no for now, maybe later) | UEFI only | Q8 answered "future scope" (adds an ARM64 phase later) |
| ADR-017 | Development on an Apple Silicon Mac host; testing on Linux x86-64 virtual machines and a spare x86-64 PC | Proposed (owner answered Q9, 2026-11-17) | Kernel features used here (cgroups, PSI, zram, suspend) exist only on Linux; a spare x86-64 test machine is available | Install Linux on the Mac hardware | Q9 answer changes the host setup |
| ADR-018 | Kernel-specific code isolated in one crate (`nexa-kernel`) | Proposed | Keeps a separate kernel possible later (addendum D7 section 3) | No isolation (rejected: later change would touch the whole system) | Kernel change decided |
| ADR-019 | App platform uses our own bundle format, sandbox, and store; Windows programs run through a hidden Wine engine; no user-visible third-party tool | Proposed (owner answered Q11 and Q13, 2026-11-17) | Owner requirement (addendum F9): engines are hidden so apps feel native (Q11); own store first, Flathub catalog optional later (Q13) | Flatpak and Flathub as visible tools (rejected by requirement) | Q11 or Q13 answer changes it |
| ADR-020 | Optional local AI assistant, off by default, removable, unloaded when idle | Proposed (owner answered Q14, 2026-11-17) | Owner requirement (addendum F10); model add-on up to 4-8 GB total; keeps idle cost at zero | Built into the base image (rejected: size and idle cost) | Q14 answer changes the model |
| ADR-021 | Android apps run in a container on the host kernel as an optional pack; ARM translators are owner-supplied only and never shipped | Proposed | Native-feeling, fast x86 apps; avoids a virtual machine; ARM translators are proprietary (addendum 2 F12, D9) | Virtual machine (heavier, slower); no Android (rejected by requirement) | Spike S6 result; Q18 answer; a virtual-machine mode decision after M5 |
| ADR-022 | Activity Manager is the single monitoring app (replaces "Activity") | Proposed (owner answered Q20, 2026-11-17) | One place for all metrics, Windows-style layout (addendum 2 F13); named "Activity Manager" by owner decision Q20 | Separate Activity app (duplicate work) | Q20 answer |
| ADR-023 | Heavier features are optional packs; the core stays inside the F6 budgets | Proposed | Keeps the core light and makes each heavy feature's cost visible (addendum 2 F15) | All features in core (rejected: idle cost) | Gate G-OPT1 result |

## 10. Change Procedure

1. Propose the change in the ADR table with the reason and alternatives.
2. Update the affected design and feature documents in the same session.
3. Record the change in the progress log (03 section 4).
