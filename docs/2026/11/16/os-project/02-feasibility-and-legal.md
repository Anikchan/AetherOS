# 02 — Feasibility and Legal

This document separates what can be built from what cannot. It sets the expectations that the blueprint depends on.

## 1. Feasibility Verdicts

| # | Requirement | Verdict | Explanation | Alternative or mitigation |
| --- | --- | --- | --- | --- |
| 1 | Run on any x86-64 PC that can run Windows or Linux (Q2) | Feasible with limits | Linux runs on most x86-64 PCs, with UEFI or legacy BIOS boot. Devices that only have Windows drivers cannot be used, because Windows drivers cannot be loaded into Linux. Some devices need proprietary firmware or have no driver (certain Wi-Fi, fingerprint readers, hybrid graphics). | Hardware coverage report (P2-T08) and coverage goal (04 section 11.1). Reading of Q2 to be confirmed in Q10. |
| 2 | Run alongside Windows | Feasible | Dual boot through UEFI. Both systems are installed, but only one runs at a time. | Boot menu with a Windows entry (F5). |
| 3 | macOS-style UI with glass look | Feasible | We write our own implementation of blur, translucency, and highlights. | Do not copy Apple icons, fonts (SF family), wallpapers, or the names "macOS", "Apple", or "Liquid Glass" in product branding. |
| 4 | Build our own kernel | Not recommended | Writing a kernel takes many years and gives no driver coverage. | Use the Linux kernel with a custom configuration. |
| 5 | Run macOS applications | Not feasible in this plan (research track only; see addendum D7) | Apple's macOS license allows macOS to run on Apple hardware. macOS apps also depend on Apple frameworks (AppKit, Metal APIs) and Mach-O and XNU kernel interfaces. Neither can be legally distributed or emulated for this purpose. | Support cross-platform apps (Electron, Flutter, Java, Qt, web apps) and Linux apps. Use a macOS-like look only. |
| 6 | Unix-like customization | Feasible | Linux provides a full Unix environment. | Configuration files, CLI, and Lua scripting (F4). |
| 7 | Free app support like Windows | Feasible | Flathub hosts thousands of free apps. | Flatpak, AppImage, Flathub (F3). |
| 8 | Run Windows apps | Feasible with limits | Wine and Proton run many Windows programs. Some fail: kernel-level anti-cheat, some DRM, some Windows-only drivers, some newer DirectX features. | Compatibility tiers (F3); dual boot for apps that must run natively. |
| 9 | iOS-style management of 50 apps | Feasible with limits | Frozen or hibernated apps use very little RAM. Active apps still need memory. iOS works because apps cooperate and are limited by the system. | Process manager (F2, D3). Benchmark (04). |
| 10 | Base image 250–512 MB | Feasible for the base system | Without Wine, large apps, and full firmware, the image fits. Full firmware and Mesa drivers are the largest risks. | Selected firmware only (D4). |
| 11 | Liquid glass effects on low-end hardware | Feasible with tiers | Blur and shaders cost GPU time. | Full, Reduced, and Minimal effect tiers (F1, D1). |
| 12 | Light on RAM and CPU | Feasible | A Linux base with a Rust compositor and a small shell can idle under 300 MB. | Budgets in F6. |
| 13 | Run Android apps as native-feeling windows, fast, without hurting the OS | Feasible with limits (addendum 2 F12) | Container on the host kernel (no emulator) can give near-native speed for x86 apps. Most Play Store apps are ARM-only and need proprietary translators we cannot ship. No Google Play Store on a generic system. Container memory must be capped. | Caps, lazy start, freeze; owner-supplied translator for personal use only; microG option. |
| 14 | Windows-style Activity Manager with full metrics, battery history, and screen time | Feasible | Linux exposes CPU, memory, disk, network, temperature, battery, and (driver permitting) GPU data. Some sensors and GPU counters are missing on some hardware. | "Not reported" display; spikes S7 and S8; monitoring overhead gates (addendum 2 D10). |

## 2. What "Run All Apps From macOS, Unix, and Windows" Means in Practice

| Source | Supported | How | Limits |
| --- | --- | --- | --- |
| Unix and Linux | Yes | Native packages, Flatpak, AppImage, command-line tools | Apps that need the Linux desktop libraries work best |
| Windows | Yes, partly | Wine and Proton, plus dual boot for the rest | Compatibility varies by app (see F3) |
| macOS | No | Not feasible (see row 5) | Use cross-platform equivalents instead |

## 3. Legal Notes

### 3.1 Licenses of the main components

These licenses were verified against the projects' own repositories and license
files on 2026-11-17 (P0-T03, session implementation-1, with cited web research).
Re-verify each at the pinned version before the first public release (P0-T03b).

| Component | Verified license (SPDX, 2026-11-17) | Compatibility note |
| --- | --- | --- |
| Linux kernel | GPL-2.0-only | Shipping binaries is fine; offer complete corresponding source (kernel plus any out-of-tree patches). |
| Buildroot | GPL-2.0-or-later | Build tool only. Output images inherit the license of their packages, not of Buildroot. |
| systemd | LGPL-2.1-or-later (minor parts GPL-2.0) | Dynamic linking and unmodified binaries are fine; keep license texts, offer source for patches. |
| Smithay | MIT | Permissive. Keep copyright and license notice. |
| Slint (planned shell UI toolkit) | GPL-3.0-or-later OR Slint Royalty-Free Desktop License | The royalty-free option requires attribution: a "Made with Slint" entry in an About/Settings screen or splash, or a badge on the public download page. The GPL option avoids that but makes the shell GPL-3.0. Decide in P4 (and re-check in P0-T03b). |
| UPower | GPL-2.0-or-later | Used as an installed service, not forked. |
| power-profiles-daemon | GPL-3.0-or-later | Used as an installed service, not forked. |
| NetworkManager | GPL-2.0-or-later (libnm: LGPL-2.1-or-later) | Used as an installed service; keep license texts. |
| iwd | LGPL-2.1-or-later | Alternative network stack (P1-T04). Re-check at pinned version. |
| greetd | GPL-3.0-or-later | Used as installed service (P4-T06). GPL-3 "installation information" applies if preinstalled on a device. |
| RAUC | LGPL-2.1-or-later | A/B update framework (P9-T01). |
| Rust crates (planned) | Usually MIT or Apache-2.0 | Check each crate's license when it is added; `cargo deny`-style check recommended in CI. |
| Wine | LGPL-2.1-or-later | Wine's own built-in DLLs may be redistributed in the image with license text and source offer. Never ship genuine Microsoft DLLs copied from a Windows installation. |
| Mesa | MIT (plus a few permissive exceptions) | Check per driver; include notices. |
| Flatpak | LGPL-2.1-or-later | Used as an installed tool, not forked. |
| PipeWire | MIT | Permissive. |
| QEMU, SeaBIOS, EDK2/OVMF (test environment only) | QEMU GPL-2.0-or-later; SeaBIOS GPL-3.0-or-later (with LGPL-3.0 linking exception); EDK2 BSD-2-Clause-Patent | Host-side test tools, not shipped in AetherOS images; notes kept for completeness. |
| Alpine Linux (test ISO) | Aggregate; per-package licenses (mostly GPL, LGPL, MIT, BSD) | Freely redistributable; pass through license texts if shared beyond this machine. |
| Waydroid (Android container reference, addendum D9) | GPL-3.0-or-later (host tooling); container image AOSP/LineageOS-derived (Apache-2.0 and others) | Runs on the host kernel via binder (mainline since 5.18). Kernel needs binder enabled (P1-T01). Ship package and image with their own notices. |
| Lucide icons (planned) | ISC | Permissive. |
| llama.cpp (planned optional AI runtime, addendum F10) | MIT | Permissive. Model files have their own licenses; each is checked before use. |
| Inter font (planned) | SIL OFL 1.1 | Redistribution allowed; keep the license file. |

### 3.2 Trademarks and branding

- Do not use "macOS", "Apple", "iOS", "Liquid Glass", "Windows", or "Microsoft" in the product name, logo, or marketing.
- Describe compatibility factually, for example "runs Windows apps through Wine".
- Chosen product name: AetherOS (decided 2026-11-17). Run a trademark search for "AetherOS" (and the AetherPad text app name) before public release (P0-T03b, deferred).
- Check that "AetherOS" and "AetherPad" do not conflict with existing software, cloud services, or registered marks in the target markets.

### 3.3 Windows

- You need your own Windows license to dual boot. AetherOS does not ship Windows.
- Windows is not redistributed. The installer only detects an existing installation.

### 3.4 Media codecs

- Some codecs (for example H.264 and AAC) have patent licensing. Plan the default codec policy during P2 (audio and video) and document it.

## 4. Technical Limits to Communicate to Users

1. **Driver coverage.** Hardware with proprietary or missing drivers may have reduced functions. The certified list shows what works.
2. **App compatibility.** Some Windows apps and games will not run. The compatibility database (F3) shows the status of each app.
3. **Background apps.** An app that is frozen cannot respond to network requests. The process manager has exceptions for audio, downloads, and VPN (F2).
4. **Memory.** Inactive apps use little memory, but the number of fully active apps is limited by real RAM.
5. **Dual boot risk.** Resizing a Windows partition can cause data loss. Backups are required. BitLocker and Fast Startup need special steps (F5).

## 5. Verdict

The project is feasible as a Linux-based desktop OS with a custom shell, compositor, and process manager. It is not feasible as a native macOS-compatible OS, and macOS apps cannot run. Windows app support is feasible through Wine and Proton, with per-app limits. Dual boot with Windows is feasible and is the recommended way to run apps that have no Linux or Wine support.

The owner should answer open questions Q8 to Q10 in README.md and accept or change the decisions in D2 section 9 before P1 starts.
