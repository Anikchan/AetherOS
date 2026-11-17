# F9 — Internal App Platform (No User-Visible Third-Party Tools)

Owner: `nexa-pkg` (package and store service), `nexa-sandbox` (sandbox policy), `nexa-compat` (Windows engine manager), `nexa-apps-store` (Software app). Internal design: D8.

## 1. What "no third-party application" means here

The user should never need a separate program to install or run apps. The OS provides one install path, one launcher, one update path, and one uninstall path for every supported kind of app.

Two things are not avoided:
- **Open-source engines inside the OS.** Running Windows programs requires a Windows-compatible layer. Writing our own Windows API layer would take many years, so the OS includes Wine (LGPL-2.1) as an engine. It is built, versioned, and updated by the OS. The user never sees a Wine app, a Wine menu, or a Wine window.
- **Open-source libraries.** Image, media, PDF, and archive support come from open-source libraries linked into our own apps.

The OS does not include any user-visible app from another company or project for installing software. Our own store, installer, and sandbox do that work.

Owner question Q11 was answered on 2026-11-17: engines such as Wine are handled entirely by the OS, so platform-specific apps feel native to the user.

## 2. Supported App Kinds

| Kind | Source | How it runs | User experience | Status |
| --- | --- | --- | --- | --- |
| Native OS bundles | Our own format (signed, read-only image plus manifest) | Our runner inside our sandbox | Click to install, click to run | MVP |
| Portable apps (AppImage format) | Any AppImage file | Runs directly; no install tool needed | Double-click to run; offered to launcher | MVP |
| Windows programs (`.exe`, `.msi`) | Windows installers or portable programs | Internal Wine engine in a private prefix (one per app) | Double-click the installer; app appears in launcher | v1 (basic in Personal Edition) |
| Cross-platform bundles | Developer ships Linux or Windows builds (Electron, Java, Python, Flutter, Qt, GTK, web apps) | Our runner or the Windows engine, depending on build | Same as native | MVP |
| macOS packages (`.dmg`, `.pkg`, `.app`) | Apple format | Not supported | Clear message with alternatives (section 5) | Not supported (D7) |

## 3. Feature List

| ID | Feature | Priority | Description |
| --- | --- | --- | --- |
| AP-01 | One install path | MVP | Double-click any supported file; the same install dialog shows the source, permissions, and size. |
| AP-02 | Own store (Software app) | MVP (basic list) | Browse, install, update, and remove apps from our repository and from local files. |
| AP-03 | Native bundle format | MVP | Signed manifest plus read-only image; installs under `~/Applications` or `/Applications`. |
| AP-04 | Portable app support | MVP | AppImage files run without installation. Launcher integration on request. |
| AP-05 | Windows engine | v1 | Internal Wine engine, one prefix per app, prefix backup and removal. |
| AP-06 | Windows installer flow | v1 | `.exe` and `.msi` installers run inside their prefix; shortcuts appear in the launcher; uninstall removes the prefix. |
| AP-07 | Sandbox | MVP | Each app runs with only the permissions it asks for (D8 section 5). |
| AP-08 | Permissions prompt | MVP | Files, network, camera, microphone, location, background activity. Changeable later in Settings. |
| AP-09 | Updates | MVP | Apps update independently; engines (Wine, runner) update with the system. |
| AP-10 | Clean uninstall | MVP | Every installed file is recorded, so removal leaves nothing behind. |
| AP-11 | Compatibility database | v1 | Tier for each Windows app (A to D; F3 section 3). Shown in the store before install. |
| AP-12 | File associations | MVP | "Open with" lists the default app first, then compatible apps. Windows programs are listed when they support the file type. |
| AP-13 | Launcher and dock integration | MVP | Installed apps appear in the launcher, dock, and overview with the same behavior as built-in apps. |
| AP-14 | Windows-friendly shortcuts | v1 | Optional keyboard profile: Ctrl+C, Ctrl+V, Ctrl+S, Alt+F4 (F4 CUS-05). Default is Super-based shortcuts. |
| AP-15 | Process management for apps | MVP | Every app uses the lifecycle manager (F2), including Windows programs. |
| AP-16 | Offline install | v1 | Install from a file without internet access. |

## 4. Windows Program Behavior

- Each Windows app gets its own prefix (a private folder with its own registry and files). Removing an app removes its prefix only.
- A prefix can be backed up and restored (AP-05).
- Windows programs that need a Windows service, a kernel driver, or anti-cheat cannot run. The store shows Tier D and the dual boot option (F5).
- Windows programs start slower than native apps. The target is in F11 (LA-08).

## 5. macOS Packages

The store and the "Open with" menu detect macOS packages (`.dmg`, `.pkg`, `.app`). They show this message:

"This is a macOS app. AetherOS cannot run macOS apps. Options: choose a version for Windows or Linux if the developer provides one, use a web version, or use an equivalent app from the store."

Reasoning and the long-term research option are in D7.

## 5.1 MS Office and similar Windows productivity apps

Microsoft Office is a Windows program. It is supported through the internal Windows engine (AP-05, AP-06) in its own prefix, like any other Windows app. Some Office features that need Windows services, kernel drivers, or anti-cheat-style components will not work and are listed as Tier C or D in the compatibility database (F3 section 3). The store shows the tier before install. Web-based Office (Office on the web) is always available through the browser (F8 DA-13). Dual boot remains the fallback for full Office (F5).

## 5.2 Apps the owner develops on macOS

macOS packages (`.dmg`, `.pkg`, `.app`) are not supported (section 5). The owner's own macOS apps must be shipped to AetherOS as cross-platform builds (Windows or Linux targets, Electron, Flutter, Qt, Java, Python, or web) or as native AetherOS bundles. The same launcher, install, update, and uninstall paths then apply (F9 sections 2 and 3). A research track for macOS command-line tools only may open after M5 (D7 section 6); macOS GUI apps are not planned for Personal Edition.

## 6. Dependencies

- P6 phase (internal app platform).
- F2 process management (AP-15).
- F4 configuration (per-app settings).
- D8 contracts.

## 7. Acceptance Criteria

| ID | Criterion | Result |
| --- | --- | --- |
| AP-AC-01 | A native bundle installs, runs from the launcher, and uninstalls with no leftover files | Not tested |
| AP-AC-02 | An AppImage runs by double-click without any installation tool | Not tested |
| AP-AC-03 | A Windows test installer runs in its own prefix and its app launches from the launcher | Not tested (v1) |
| AP-AC-04 | Removing a Windows app removes only its prefix | Not tested (v1) |
| AP-AC-05 | A sandboxed app cannot read a folder it did not ask for (test with an access attempt) | Not tested |
| AP-AC-06 | A macOS package shows the explanation message and does not start an install | Not tested |
| AP-AC-07 | No user-visible third-party app is present in the default image (checked by a file list in the build) | Not tested |
| AP-AC-08 | Every installed app appears in the launcher and is managed by F2 | Not tested |

## 8. Open Questions

- Q11: Resolved 2026-11-17 — engines are hidden so platform-specific apps feel native (see section 1).
- Q13: Resolved 2026-11-17 — own store first; Flathub catalog reading is an optional later enhancement.
- Q15: Resolved 2026-11-17 — Windows program support (AP-05, AP-06) is included in Personal Edition.
