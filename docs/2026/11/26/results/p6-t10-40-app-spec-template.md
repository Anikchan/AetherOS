# P6-T10 — 40-App Compatibility Spec Template (reference machine)

- Session: S4 (`session - 4 (Nov26)`), 2026-11-26
- Task: roadmap P6-T10 / 04 §7 / F3 (APP-AC-06: 30 of 40 apps in Tier A or B)
- Artifacts:
  - `apps/runner/compat/40-app-spec-template.json` — the generated harness spec
    (40 slots, installer + probe commands).
  - `apps/runner/src/compat_reference.rs` — the table and builder that generate it.
  - `aether-runner compat reference-template --out FILE [--payload DIR] [--engine V] [--machine NAME]`
    — regenerate; `compat reference-slots` prints the table.
- Status: **template ready; the real runs are hardware-gated.**

## What the spec contains

Each of the 40 slots (keys fixed by 04 §7, `CompatDb::test_plan()` names) gets:

| Field | Content |
| --- | --- |
| `installer` | `<payload>/<vendor installer>` (`.exe`/`.msi`) |
| `checks[install]` | `aether-runner windows install --installer … --id com.aether.compat.<slot> --entry <exe>` (900 s) |
| `checks[launch]` | `aether-runner launch com.aether.compat.<slot>` (180 s) |
| `checks[window]` | `xdotool search --name "<window title>"` (60 s) |
| `checks[input]` | `aether-compat-probe input --window "<title>" --exe <exe>` |
| `checks[audio\|video\|files\|network\|persistence]` | `aether-compat-probe <check> --id <app id>` |
| `checks[stability]` | `aether-compat-probe stability --id <app id> --seconds 60` (300 s) |
| `tweak` | documented prefix tweak for the known Tier B/D candidates |
| `args` | entry point, for a manual re-launch |

A check passes when the command exits 0 within its timeout; the exit code and
stderr tail become the `detail` of the `CheckResult`, which is what the tier
engine grades (see `p6-t09-t10-p9-t02.md`).

### Probe helper contract (`aether-compat-probe`)

The helper is the only non-stock command in the spec. It is deliberately small
so it can be written against the engine as P6-T02/T03 helpers settle:

```
aether-compat-probe input        --window TITLE --exe EXE   # type + shortcut into the focused window
aether-compat-probe audio        --id APP-ID                # default sink produces sound during a 5 s clip
aether-compat-probe video        --id APP-ID                # hw-decoded video frame advances (no black frame)
aether-compat-probe files        --id APP-ID                # open/save inside the prefix and ~/Documents
aether-compat-probe network      --id APP-ID                # outbound HTTPS from the prefix succeeds
aether-compat-probe persistence  --id APP-ID                # settings survive an engine restart
aether-compat-probe stability    --id APP-ID --seconds 60   # no crash/hang/leak over the window
```

Exit 0 = pass, non-zero = fail with a one-line reason on stderr. Until the
helper exists, the slot records `cannot run aether-compat-probe` → check failed
→ Tier C/D, which is honest (the gate stays closed rather than passing by
default).

## The 40 reference programs

Order matches `TEST_SLOTS`. "Tweak" is the documented prefix change (Tier B) or
an expected-fail note (Tier D).

| # | Slot | Program | Vendor | Installer | Window probe | Tweak / note |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Browser app 1 | Google Chrome | Google | ChromeSetup.exe | Google Chrome | — |
| 2 | Browser app 2 | Mozilla Firefox | Mozilla | Firefox Setup.exe | Mozilla Firefox | — |
| 3 | Browser app 3 | Brave | Brave Software | BraveBrowserSetup.exe | Brave | — |
| 4 | Office and productivity app 1 | LibreOffice Writer | TDF | LibreOffice_Win_x86-64.msi | LibreOffice Writer | — |
| 5 | Office and productivity app 2 | LibreOffice Calc | TDF | LibreOffice_Win_x86-64.msi | LibreOffice Calc | — |
| 6 | Office and productivity app 3 | LibreOffice Impress | TDF | LibreOffice_Win_x86-64.msi | LibreOffice Impress | — |
| 7 | Office and productivity app 4 | Adobe Acrobat Reader | Adobe | AcroRdrDC_en_US.exe | Adobe Acrobat | — |
| 8 | Office and productivity app 5 | Notion | Notion Labs | Notion-Setup.exe | Notion | — |
| 9 | Office and productivity app 6 | Mozilla Thunderbird | Mozilla | Thunderbird Setup.exe | Mozilla Thunderbird | — |
| 10 | Office and productivity app 7 | Todoist | Doist | TodoistSetup.exe | Todoist | — |
| 11 | Office and productivity app 8 | PDFCreator | pdfforge | PDFCreator-Setup.exe | PDFCreator | printer driver installs manually |
| 12 | Media app 1 | VLC media player | VideoLAN | vlc-win64.exe | VLC media player | — |
| 13 | Media app 2 | foobar2000 | foobar2000 | foobar2000.exe | foobar2000 | — |
| 14 | Media app 3 | Shotcut | Meltytech | shotcut-win64.exe | Shotcut | — |
| 15 | Media app 4 | GIMP | GIMP Team | gimp-setup.exe | GIMP | — |
| 16 | Media app 5 | HandBrake | HandBrake | HandBrake-Win_GUI.exe | HandBrake | — |
| 17 | Media app 6 | Spotify | Spotify | SpotifySetup.exe | Spotify | — |
| 18 | Development app 1 | Visual Studio Code | Microsoft | VSCodeUserSetup-x64.exe | Visual Studio Code | — |
| 19 | Development app 2 | Visual Studio 2022 Community | Microsoft | vs_community.exe | Microsoft Visual Studio | offline layout first |
| 20 | Development app 3 | PuTTY | PuTTY | putty-64bit-installer.msi | PuTTY | — |
| 21 | Development app 4 | DBeaver | DBeaver | dbeaver-ce-setup.exe | DBeaver | — |
| 22 | Development app 5 | GitHub Desktop | GitHub | GitHubDesktopSetup-x64.exe | GitHub Desktop | — |
| 23 | Development app 6 | Docker Desktop | Docker | Docker Desktop Installer.exe | Docker Desktop | needs WSL2/Hyper-V → expected D |
| 24 | Development app 7 | Postman | Postman | Postman-win64-Setup.exe | Postman | — |
| 25 | Games app 1 | Fallout 4 | Bethesda | setup_fallout_4.exe | Fallout4 | DX11 (GOG offline) |
| 26 | Games app 2 | Cyberpunk 2077 | CD Projekt | setup_cyberpunk_2077.exe | Cyberpunk 2077 | force vkd3d-proton |
| 27 | Games app 3 | Celeste | Maddy Makes Games | setup_celeste.exe | Celeste | OpenGL/FNA |
| 28 | Games app 4 | Steam | Valve | SteamSetup.exe | Steam | launcher-based |
| 29 | Games app 5 | Cuphead | Studio MDHR | setup_cuphead.exe | Cuphead | Unity |
| 30 | Games app 6 | ABZU | Giant Squid | setup_abzu.exe | ABZU | Unreal Engine |
| 31 | Games app 7 | Age of Empires II (1999) | Ensemble | setup_aoe2.exe | Age of Empires II | 32-bit |
| 32 | Games app 8 | VALORANT | Riot Games | RiotClientInstalls.exe | VALORANT | Vanguard needs a kernel driver → expected D |
| 33 | Utilities app 1 | 7-Zip | Igor Pavlov | 7z-x64.exe | 7-Zip | — |
| 34 | Utilities app 2 | Dropbox | Dropbox | DropboxInstaller.exe | Dropbox | — |
| 35 | Utilities app 3 | KeePass 2 | Dominik Reichl | KeePass-Setup.exe | KeePass | — |
| 36 | Utilities app 4 | HWiNFO64 | Martin Malik | HWiNFO64-Setup.exe | HWiNFO | sensor access may need a tweak |
| 37 | Utilities app 5 | ShareX | ShareX Team | ShareX-setup.exe | ShareX | — |
| 38 | Utilities app 6 | AnyDesk | AnyDesk Software | AnyDesk.exe | AnyDesk | — |
| 39 | Utilities app 7 | WireGuard | WireGuard | wireguard-installer.exe | WireGuard | TUN device required |
| 40 | Utilities app 8 | Duplicati | Duplicati | Duplicati.msi | Duplicati | — |

Substitutes are allowed: keep the slot name (the database key) and change the
program. Record the swap in the run notes.

## Running it on the reference machine

```bash
# 1. Payload: installers in one directory, plus a manifest with pinned versions
#    and SHA-256 values that the operator fills in after download.
install -d /var/lib/aether/compat/payload
#    copy the 40 installers there

# 2. Regenerate the spec if the payload dir or engine version changed
cargo run -q -p aether-runner -- compat reference-template \
  --payload /var/lib/aether/compat/payload \
  --engine wine-10.0 --machine dell-xps \
  --out apps/runner/compat/40-app-spec-template.json

# 3. Dry run: every slot stays Untested, gate stays OPEN
cargo run -q -p aether-runner -- compat harness \
  --spec apps/runner/compat/40-app-spec-template.json \
  --db /var/lib/aether/compat/db.json --dry-run

# 4. Real run (reference machine, installers present). This takes hours:
#    each install has a 900 s budget, stability runs 60 s per app.
cargo run -q -p aether-runner -- compat harness \
  --spec apps/runner/compat/40-app-spec-template.json \
  --db /var/lib/aether/compat/db.json

# 5. Report / gate
cargo run -q -p aether-runner -- compat report --db /var/lib/aether/compat/db.json
```

The run is resumable: results are written to the database after each slot, and
an import run (`slot.recorded` filled in) reuses earlier observations.

## Verified on this host (no hardware)

```
$ cargo run -q -p aether-runner -- compat reference-template --out apps/runner/compat/40-app-spec-template.json
COMPAT_REFERENCE_TEMPLATE slots=40 payload=/var/lib/aether/compat/payload file=apps/runner/compat/40-app-spec-template.json

$ cargo run -q -p aether-runner -- compat reference-slots | tail -2
Utilities app 7	WireGuard	WireGuard	wireguard-installer.exe	WireGuard
Utilities app 8	Duplicati	Duplicati	Duplicati.msi	Duplicati
COMPAT_REFERENCE_SLOTS total=40

$ cargo run -q -p aether-runner -- compat harness \
    --spec apps/runner/compat/40-app-spec-template.json --db /tmp/s4-compat-db.json --dry-run
COMPAT_HARNESS executed=0 imported=0 dry_run=40 skipped=0 unknown=0
COMPAT_SUMMARY total=40 a=0 b=0 c=0 d=0 untested=40 avg_score=0 gate=OPEN

$ cargo test -p aether-runner
unittests: 40 passed (6 new in compat_reference), windows_flow: 4 passed
```

## Still open (hardware)

1. Download/pin the 40 installers and record version + SHA-256 per file (the
   installer names in the spec are version-less on purpose).
2. Write `aether-compat-probe` against the engine helpers, or substitute an
   equivalent probe and document it in the run log.
3. Run the 40 apps on the reference machine; export
   `docs/2026/11/26/results/compat-db-<machine>.json` and the summary.
4. APP-AC-06 gate: 30 of 40 in Tier A or B.

## Notes

- Real vendor names appear only in the test plan; the OS does not bundle these
  installers. The operator supplies them from the vendor sites.
- The spec intentionally keeps `engine` and `machine` labels (`wine-10.0`,
  `reference-machine`) so an imported run carries provenance.
- `apps/runner/compat/40-app-spec-template.json` is generated, not hand-edited;
  regenerate with the command above after changing the table.
