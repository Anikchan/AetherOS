# F12 — Android App Support (Optional Pack A)

Owner: `nexa-android` (manager service), with an internal Android container engine (Waydroid-based; license to verify in P0-T03). Internal design: D9.

## 1. What the user should feel

The three requirements, and how the design meets each one:

| # | Requirement | How it is met | How it is measured |
| --- | --- | --- | --- |
| R1 | The OS is natively running the app | Each Android app opens in its own window with our title bar, appears in the launcher, dock, Activity Manager, and overview, and follows our theme. The Android home screen and system bars are never shown. Files go through our picker. | Spike S6 (P6-AND-T01) must prove per-app windows. Checklist AND-AC-01. |
| R2 | No delay or performance issues | The container uses the host kernel (no emulator), so there is no virtual machine overhead. The graphics path uses the host GPU driver. Keep-ready mode removes the cold start. Translation cost for ARM-only apps is measured and disclosed. | Gates G-AND1 to G-AND4 (D6 and D10). |
| R3 | OS performance is not impacted | Hard caps on memory, CPU, and disk for the whole container. The container is frozen when not focused and is the first group to hibernate under pressure. Native apps keep priority. | Gates G-AND2, G-AND3, G-AND5. |

Honest statement: "No delay" cannot be literal. A cold start of the Android container takes seconds. The design hides that wait behind a progress view and offers keep-ready mode, which trades memory for speed.

## 2. Hard Limits (Read Before Installing)

Owner decision Q18 (2026-11-17): wants these limits removed. The plan will investigate options (user-supplied translators, alternative stores, microG), but proprietary translators and Google Play Store cannot be shipped in any image; limits remain unless a legal and technical path is found.

| Limit | Cause | What the user sees |
| --- | --- | --- |
| Most Play Store apps are ARM-only and will not run on an Intel or AMD PC | Translation layers (libhoudini from Intel, libndk from Google) are proprietary; we cannot ship them | Store shows "needs ARM translation". Owner may add a user-supplied translator for personal use (AND-15). Not included in any public image. |
| Google Play Store and Google Play Services are not included | Google certifies only single-device builds; generic Android systems are not eligible | No Play Store. microG (open-source replacement) optional (AND-16). Some apps will fail. |
| Streaming apps that need Widevine L1 may play at lower quality or not at all | Widevine certification is the same licensing issue | Shown as a known limit in the store. |
| The container shares the host kernel | Container isolation is weaker than a virtual machine | Documented. Stronger virtual-machine mode is a later option (D9 section 9). |
| Intel and AMD graphics only | Container graphics path needs open drivers | NVIDIA shown as "not supported". |
| Kernel must include binder support | Android inter-process communication | Build config in P1-T01; checked at pack install. |

## 3. Feature List

| ID | Feature | Priority | Description |
| --- | --- | --- | --- |
| AND-01 | Optional pack install | MVP (after M5 by default) | Settings → Features → Android apps. Shows download size, disk use, and idle cost before install. Off by default. |
| AND-02 | Container engine | MVP | Android userspace in a container using the host kernel; internal engine, not a visible app. |
| AND-03 | Install apps from our store | MVP | Apps from an open app repository (F-Droid index format, read by our own store code). Also "Open with" for APK files. |
| AND-04 | Per-app windows | MVP (pending spike S6) | Each Android app is a separate window with our decorations. Fallback if spike fails: one Android window (documented as a major limit). |
| AND-05 | Launcher, dock, overview, and Activity Manager integration | MVP | Installed Android apps behave like other apps (F2 states, F13 metrics). |
| AND-06 | Lazy start | MVP | Container starts when the first Android app is opened. A progress view shows the stage. |
| AND-07 | Keep-ready mode | v1 (option) | Container is started in the background and frozen with compressed memory. Costs the memory in D9 section 6. Off by default. |
| AND-08 | Resource caps | MVP | Memory limit (default 1.5 GB, changeable), low CPU weight, low disk I/O weight. Caps are shown in Activity Manager. |
| AND-09 | Freeze and hibernate | MVP | Frozen when no Android app is focused for the grace period; hibernated under pressure. Container is the first group to hibernate after background native apps. |
| AND-10 | Notifications | v1 | Android notifications appear in our notification center. Spike needed (P6-AND-T07). |
| AND-11 | Clipboard | v1 | Text copy and paste between native and Android apps. Spike needed. |
| AND-12 | Files | MVP | Android apps can open files only through our picker. Optional shared Downloads folder (v1). |
| AND-13 | Audio | MVP | Through PipeWire on the host. |
| AND-14 | Camera, microphone, and location | v1 | Through the permission broker (F9 AP-08). Each permission asked once. |
| AND-15 | ARM translation (user-supplied) | Owner option (personal use only) | Off by default. Never shipped in the OS image. Store marks ARM-only apps. |
| AND-16 | microG (open-source replacement for Google services) | v1 (option) | Installed on request. Not a Google product. Compatibility list in the store. |
| AND-17 | Input mapping | v1 | Android back action mapped to Alt+Left or Escape (owner choice in settings). Long press on right mouse button. Touch gestures with touchpad. |
| AND-18 | Clean uninstall | MVP | Removes the container image, app data, and registry entries (D8 section 4). |
| AND-19 | App updates | MVP | Per app, from the store. |
| AND-20 | Android app metrics | MVP | CPU, memory, GPU, and energy per Android app in Activity Manager (F13). |

## 4. Performance Targets

| ID | Measure | Target | Limit | Gate |
| --- | --- | --- | --- | --- |
| AND-T1 | Cold start of the container (first app, lazy mode) | 8 s | 15 s (with progress shown) | G-AND1 |
| AND-T2 | Warm launch of a typical x86 Android app (container ready) | 1.5 s | 3 s | G-AND1 |
| AND-T3 | Warm launch of an ARM-only app with translation | 3 s | 6 s | G-AND1 (reported separately) |
| AND-T4 | Frame rate of a typical x86 app (UI scrolling) | 55 fps | 45 fps | G-AND4 |
| AND-T5 | Frozen container memory | ≤ 150 MB | 250 MB | G-AND3 |
| AND-T6 | Restore from frozen | ≤ 1.5 s | 3 s | G-AND3 |
| AND-T7 | Memory cap of the container | 1.5 GB (default) | Hard limit by kernel | G-AND2 |
| AND-T8 | Native app frame gap while Android app runs in background | No gap over 1 s | — | G-AND5 |

These are targets. Each one is measured on the reference machines before the pack is marked ready.

## 5. Dependencies

- P6-AND phase (after M5 by default; option to include it before M5, see 03 section 2.1).
- F2 (process management), F9 (permission broker and sandbox), F13 (Activity Manager), D9 (design), D10 (metrics).
- Kernel build: binder support in P1-T01 (added to the kernel fragment).

## 6. Acceptance Criteria

| ID | Criterion | Result |
| --- | --- | --- |
| AND-AC-01 | An Android app opens in its own window with our title bar; no Android home screen or system bars visible | Not tested |
| AND-AC-02 | The app appears in launcher, dock, overview, and Activity Manager | Not tested |
| AND-AC-03 | Cold start shows progress and reaches the first app within 15 s | Not tested |
| AND-AC-04 | Warm launch of a typical x86 app within 3 s (G-AND1) | Not tested |
| AND-AC-05 | Container memory never exceeds the cap; host pressure stays Normal when the cap is reached alone | Not tested |
| AND-AC-06 | Frozen container memory ≤ 250 MB; restore ≤ 3 s | Not tested |
| AND-AC-07 | Native foreground app keeps frame gaps under 1 s while an Android app runs in the background | Not tested |
| AND-AC-08 | Install from the store and sideload an APK; uninstall leaves no files | Not tested |
| AND-AC-09 | Store shows ARM-only apps as "needs translation" and does not install them without the translator | Not tested |
| AND-AC-10 | Removing the pack returns disk use to the pre-install value and removes all processes | Not tested |

## 7. Open Questions

- Q18: Accept the limits in section 2, especially "most Play Store apps are ARM-only" and "no Google Play Store".
- Q19: Lazy start (default: no memory cost until used) or keep-ready (faster, costs memory when enabled)?
- Q22 (also in 03 section 2.1): Include Android in Personal Edition before M5 (adds about 620 hours and moves M5 to about June 2030), or keep it as the first optional pack after M5?
