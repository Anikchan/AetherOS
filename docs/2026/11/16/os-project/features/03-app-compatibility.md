# F3 — App Compatibility

Note (addendum 1, 2026-11-17): how these apps are installed and run without user-visible third-party tools is defined in `docs/2026/11/17/os-addendum-1/F9-app-platform.md`.

Owner components: `aether-pkg` (package service), software store (shell), `aether-compat` (Wine and Proton manager).

## 1. Support Model

| Source | Support level | Method | Notes |
| --- | --- | --- | --- |
| Linux native apps | Full | Flatpak (primary), AppImage (portable), distribution packages where available | Best experience |
| Unix command-line tools | Full | POSIX shell, GNU and BSD-like tools, package manager | Terminal is available but not required |
| Cross-platform apps (Java, Python, Node, Electron, Flutter, Qt, GTK, web apps) | Full | Runtime libraries and sample apps | Most modern apps fall in this group |
| Windows apps (x86-64) | Partial | Wine and Proton managed by `aether-compat` | Compatibility varies (section 4) |
| Windows apps that cannot run through Wine | Via dual boot | Boot into Windows | Recommended fallback |
| macOS apps | Not supported as macOS packages | Not feasible (see 02 section 1, row 5) | Use cross-platform builds (Windows or Linux targets, Electron, Flutter, Qt, Java, Python, web) or native AetherOS bundles. The owner's own macOS apps follow this path. |
| Android apps | Not in v1 | Optional later (Waydroid-style container) | Open question Q6 |

## 2. Feature List

| ID | Feature | Priority | Description |
| --- | --- | --- | --- |
| APP-01 | Flatpak and Flathub | MVP | Install, update, and remove apps from Flathub. Sandbox permissions are shown to the user. |
| APP-02 | AppImage | MVP | Double-click to run. Offer to integrate into the launcher. |
| APP-03 | Runtime support | MVP | Java, Python, Node, Electron, Flutter, Qt, and GTK work out of the box. Documented in the developer guide (P7). |
| APP-04 | Software store | MVP (basic list), Later (full store) | Basic list of Flatpak and AppImage apps with search and updates. Full shell-styled store with categories is Later. |
| APP-05 | Windows apps through Wine | v1 | Run `.exe` and `.msi` files. Double-click opens with a prefix chooser. |
| APP-06 | Wine prefix manager | v1 | One prefix per app. Create, duplicate, back up, and delete prefixes. Install common runtime components. |
| APP-07 | Windows games through Proton | Later | Proton and DXVK or VKD3D-Proton for DirectX translation. |
| APP-08 | Compatibility database | v1 | Tier list per app (section 3). Users can submit reports. |
| APP-09 | "Open with" associations | v1 | File types map to native apps or to Windows apps through Wine. |
| APP-10 | Native app format guide | MVP | Document how developers package a Linux app (Flatpak manifest and AppImage). |
| APP-11 | Dual-boot guidance | MVP | When an app is Tier D, the store and compatibility list point to dual boot. |

## 3. Compatibility Tiers

| Tier | Meaning | Action for user |
| --- | --- | --- |
| A | Works with no changes | Install and run |
| B | Works after a documented setting or prefix tweak | Follow the guide in the compatibility database |
| C | Partly works | Use with known issues, or use dual boot for the missing features |
| D | Does not work | Use dual boot |

## 4. Known Limits of Windows Compatibility

- Kernel-level anti-cheat and some DRM do not work.
- Some apps need features of a specific Windows version.
- Some hardware-specific Windows drivers (for example, special printers or audio interfaces) are not available.
- Performance can be lower than native Windows.
- Some apps need a Windows license or online activation that cannot be satisfied through Wine.
- Microsoft Office runs through the Windows engine in its own prefix (F9 section 5.1). Features that need Windows services or drivers may be limited; web-based Office is always available through the browser.

## 5. Dependencies

- P6 compatibility phase.
- F2 process manager (Wine processes are managed like any other app; each Wine prefix is one app group).
- F4 configuration (per-app settings).

## 6. Acceptance Criteria

| ID | Criterion | Result |
| --- | --- | --- |
| APP-AC-01 | Install, launch, update, and remove a Flathub app from the software store | Not tested |
| APP-AC-02 | Launch an AppImage by double-click and integrate it into the launcher | Not tested |
| APP-AC-03 | Sample Java, Python, Node, Electron, Flutter, Qt, and GTK apps launch | Not tested |
| APP-AC-04 | Create a Wine prefix, install a Windows app, and launch it from the launcher | Not tested |
| APP-AC-05 | Remove a Wine prefix without affecting other prefixes | Not tested |
| APP-AC-06 | 30 of 40 test Windows apps reach Tier A or B (04 section 7) | Not tested |
| APP-AC-07 | Software store shows the tier for each Windows app in the compatibility database | Not tested |
| APP-AC-08 | Tier D apps show the dual-boot guidance | Not tested |

## 7. Open Questions

- Which Wine build to ship: stable upstream Wine or a Proton-based build. Decide in P6-T03.

Implementation note (spike S5, P0-T12, 2026-11-18, headless scope): Wine 10.0 ran a
Windows console program in the Linux VM with a 20 ms startup. A GUI-app-through-Wayland
residual is tracked for the graphical VM session (P2). The 20 ms number is a
functional data point only, not a bare-metal measurement.
- Whether to ship Wine by default or install it on first use. Proposed: install on first use to save space in the base image (see D4 size budget).
