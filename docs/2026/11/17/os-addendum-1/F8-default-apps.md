# F8 — Built-in Default Apps

Owner: `nexa-apps` (a set of small original apps built with Slint and Rust). Parent plan: `docs/2026/11/16/os-project/`.

## 1. Goal

Every common task works on the first boot without installing anything: writing plain text, calculating, viewing images and documents, playing media, using a terminal, managing archives, and keeping time. The apps feel like the rest of the OS and start fast.

## 2. Naming and Look Rules

- Use functional names (for example, "Text", "Calculator", "Images"). Do not use Microsoft product names such as "Notepad", "Paint", or "Explorer".
- The text app's working name is AetherPad (the OS is AetherOS); the generic label "Text" is used in the feature list until the code names are fixed (owner Q12).- Match functions and expected behavior from common Windows and macOS editors, but write the code, icons, and layouts from scratch.
- Use the same glass, tokens, and motion as the shell (D1).
- Each app is a single small binary plus its views. No embedded web engine (F6 LW-07).

Naming decision: owner question Q12.

## 3. Feature List

| ID | App | Functions | Priority | Phase |
| --- | --- | --- | --- | --- |
| DA-01 | AetherPad (text and code editor) | Open and save; UTF-8, UTF-16, and Windows-1252 encodings; CRLF and LF line endings; word wrap; find and replace; go to line; insert date and time; undo and redo; zoom; status bar with line and column; recent files; drag and drop; syntax colors for common languages (later) | MVP | P4-T10 |
| DA-02 | Calculator | Standard arithmetic; keyboard input; history of last 20 results; scientific mode (v1) | MVP | P4-T11 |
| DA-03 | Images | JPEG, PNG, WebP, GIF, BMP, TIFF; zoom and pan; rotate and flip; crop; save as; slideshow (v1) | MVP | P4-T12 |
| DA-04 | Documents (PDF and text viewer) | PDF view, search, page thumbnails, print (v1); annotations (Later) | MVP (view), v1 (print) | P4-T13 |
| DA-05 | Player (audio and video) | Common audio and video formats; play, pause, seek, volume; playlist; keyboard media keys; codec policy per D4 and open question on patented codecs | MVP | P4-T14 |
| DA-06 | Terminal | Tabs; the default shell (F4 CUS-07); copy and paste; font size; scrollback; profile colors | MVP | P4-T15 |
| DA-07 | Calendar and Clock | Month and week view; clock, alarm, timer, stopwatch; time zones | v1 | P4-T16 |
| DA-08 | Archive | Create and extract ZIP and TAR (with GZIP and ZSTD); read 7z (v1) | MVP | P4-T17 |
| DA-09 | Files | Already in F1 (SHL-12) | MVP | P4-T08 |
| DA-10 | Activity Manager (replaces "Activity") | Addendum 2 F13; F2 PRM-10 | MVP | P5-T07 |
| DA-11 | Settings | Already in F1 (SHL-11) | MVP | P4-T07 |
| DA-12 | Screenshot | Already in F1 (SHL-17) | MVP | P4-T04 area |
| DA-13 | Browser (lightweight) | Tabbed browsing; bookmarks; history; downloads; private window; media playback (audio and video) with hardware acceleration where available; streaming site support; reader mode; find in page; zoom; password manager integration (later) | MVP | P4-T18 |
| DA-14 | Mail (v1) | Read, send, and organize email; multiple accounts; attachments; search | v1 | P4-T19 |
| DA-15 | Notes (v1) | Quick notes; folders; search; optional sync later | v1 | P4-T20 |
| DA-16 | Accessibility (v1) | Magnifier; high-contrast and text-size options; keyboard navigation; screen-reader hooks | v1 | P4-T21 |

Browser: included by default as a lightweight browser with high-quality media playback and streaming support (owner question Q16). The Software app (F9) still offers heavier recommended browsers.

## 3.1 AetherPad plan (DA-01)

- Working name: AetherPad (text app). Generic label "Text" remains in the feature list until code names are fixed.
- Scope: plain text and code editor with the DA-01 functions. No embedded web engine (F6 LW-07). Shared Slint runtime.
- Engine: Rust with a rope or gap buffer for large files; syntax highlighting as a later add-on.
- Tasks: P4-T10 (core editor), P4-T10a (encoding and line-ending handling), P4-T10b (find/replace and go-to-line), P4-T10c (syntax colors, v1).
- Acceptance: DA-AC-01 and DA-AC-02.

## 3.2 Browser plan (DA-13)

- Goal: a lightweight default browser with high-quality media playback and streaming support, without becoming a second desktop environment.
- Engine: an embeddable web engine (chosen in P4; license checked in P0-T03). Prefer whichever engine is lighter. Target HD playback when feasible on the lightweight system; otherwise use reduced quality. No bundled heavy browser.
- Scope: tabs, bookmarks, history, downloads, private window, media playback with hardware acceleration where available, streaming site support, reader mode, find in page, zoom.
- Tasks: P4-T18 (core browser shell), P4-T18a (media and streaming support), P4-T18b (bookmarks, history, downloads).
- Acceptance: cold launch ≤ 500 ms where possible; media playback passes the test files in DA-AC-06; idle cost 0 MB and 0% CPU when closed.
- Heavier browsers (full-featured, developer tools) remain optional through the Software app (F9).

## 3.3 Avoid by default (research, 2026-11-17)

Do not ship these in the default image:

- Electron or Chromium-per-app shells (duplicate runtimes, high RAM and disk).
- Java-based apps or heavyweight cross-platform toolkits not shared by the OS.
- Full office suites (for example LibreOffice); offer them through the Software app.
- Heavy media suites (GIMP, Audacity, Blender); optional installs.
- Redundant players or editors; preinstalled third-party trials or adware.
- Always-on cloud sync or telemetry daemons not needed for first boot.

## 3.4 Gaps to consider (research, 2026-11-17)

The reference operating systems commonly ship these; decide scope before M3:

- Mail (added above as v1).
- Notes (added above as v1; may also be folded into AetherPad).
- Maps (network-dependent; optional).
- Accessibility depth (screen reader, keyboard navigation) (added above as v1).
- Printer support and print dialogs.
- Backup and restore (for example a Time Machine-style tool).

## 4. Speed and Resource Rules

| Rule | Target |
| --- | --- |
| Cold launch to first frame | ≤ 500 ms for each default app (F11) |
| Idle cost when closed | 0 MB and 0% CPU (not running) |
| Idle cost when open and not focused | Suspended like any app (F2) |
| Shared runtime | All apps use one Slint runtime library, so the code is loaded once in memory |
| Size per app | Each binary ≤ 10 MB on disk, not counting shared libraries; media and PDF libraries are the largest parts |

## 5. Dependencies

- P4 desktop shell (windows, menus, file associations, launcher).
- Open-source libraries for image decoding, PDF rendering, media, and archives. License check per library in P0-T03 (see 02 section 3.1).
- F9 app platform, so default apps appear in the same launcher and file-association system as other apps.

## 6. Acceptance Criteria

| ID | Criterion | Result |
| --- | --- | --- |
| DA-AC-01 | Text opens and saves a UTF-16 file and a Windows-1252 file without losing characters | Partial (2026-11-23) — UTF-8 open/save, undo/redo, counts and find/replace are implemented and tested; UTF-16 and CP-1252 decoding are not implemented yet |
| DA-AC-02 | Text find and replace works across a 10 MB file with no UI freeze over 1 s | Partial — find/replace (next/all, case toggle, unicode) is implemented and unit-tested; the 10 MB / 1 s performance run needs the device |
| DA-AC-03 | Calculator produces correct results for a 50-case test set | **Pass (2026-11-23)** — 50-case set in `apps/calculator/tests/acceptance.rs`, all green, plus documented error cases |
| DA-AC-04 | Images opens each listed format and rotates without losing quality | Pass (headless, 2026-11-23) — png/jpeg/gif/bmp decode tested with generated fixtures; 90° rotation/flip are lossless pixel operations |
| DA-AC-05 | Documents opens a 200-page PDF and searches it | Blocked — the poppler adapter is implemented and reports `NotAvailable` when `pdftoppm`/`pdftotext` are missing; no PDF tooling on the dev host |
| DA-AC-06 | Player plays the test files for each supported format | Blocked — GStreamer is not installed on the dev host; transport/playlist logic is tested with the mock engine |
| DA-AC-07 | Terminal runs the default shell with copy and paste, and resizing works | Partial — VT parser and a real forkpty session (spawn/read/write/resize) are tested on unix; copy/paste UI and an event-loop pump are pending |
| DA-AC-08 | Archive extracts ZIP and TAR test files with correct permissions | **Pass (headless, 2026-11-23)** — zip/tar extraction, creation, permissions and delete round-trip tested; `../` entries rejected in both formats |
| DA-AC-09 | Each default app's cold launch ≤ 500 ms (F11 measurement) | Not measured — needs the device (no interactive host yet) |
| DA-AC-10 | With all default apps closed, the apps use 0 MB and 0% CPU | Not measured — needs the device |
| DA-AC-11 | Browser plays the test media files with audio and video, including one streaming site | Blocked — WebKitGTK embedding arrives with the graphics session (P3) |
| DA-AC-12 | Browser cold launch to first frame ≤ 500 ms | Not measured — needs the device |

Evidence: `docs/2026/11/23/results/p4-shell-and-apps.md` and
`docs/2026/11/23/results/screenshots/`.

## 7. Open Questions

- Q12: Resolved 2026-11-17 — text app working name is `<OSname>Pad` (see section 2).
- Q16: Resolved 2026-11-17 — a lightweight browser with high-quality media and streaming support is included by default (see section 3).
- Patented codecs (H.264, AAC, HEVC) in Player: include them or leave them to the user to install. Decide in P4-T14.
