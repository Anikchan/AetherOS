# P7/P8 — configuration, aetherctl, hooks, features, installer

- Date: 2026-11-23
- Session: S4 (v2 plan), autonomous run
- Scope: `config/`, `installer/`

## Verified

```bash
cargo test -p aether-config      # 34 tests pass
cargo test -p aether-installer  # 23 tests pass
```

Total: **57 tests, 0 failed**.

### P7 — configuration (F4/F15)

| Task | Evidence |
|---|---|
| P7-T01 schema + layered files | 25 keys across `aether.toml`, `shell/{dock,menubar,launcher,notifications}.toml`, `theme/tokens.toml`, `process/lifecycle.toml`; every key has a valid default, a file and a TOML key; defaults → system → user merge; invalid layer reported while the last valid value stays active; file/line in TOML errors |
| P7-T02 theme engine | tokens from store; reload detects a valid change; invalid change keeps the last valid tokens; `aetherctl theme show/reload` |
| P7-T03 `aetherctl` | `config get/set/reset/check`, `reload`, `theme`, `keybindings`, `hooks`, `features`, `apps`, `version`; set preserves comments in the user file; invalid values write nothing |
| P7-T04 Lua hooks | discovery by file name/header; 200 ms limit; timeout and failure disable + persist; missing `lua` reported; real Lua hook run and infinite-loop timeout verified |
| P7-T06 keybindings | normalization (`Ctrl+Alt+T` = `ctrl+alt+t`), conflict detection, user-over-default merge, defaults include Super+Escape |
| P7-T07 feature manager | 7 packs from F15 with disk/idle cost, cost preview, idle-budget refusal, install/remove/disable, persistence, clean state removal |
| P5-T16/P7-PK-T03 battery report | `aether-power-report` text/JSON report exists (powerd) |
| P7-T05 extension API | deferred by the roadmap |

CLI transcript:

```text
$ aetherctl config get dock.position
"bottom"
$ aetherctl config set dock.position left
SET dock.position="left"
$ aetherctl config check
CONFIG_OK keys=25
$ aetherctl keybindings check
KEYBINDINGS_OK count=7
$ aetherctl features install D
INSTALLED D Advanced monitoring (20 MB)
```

### P8 — installer (F5)

| Task | Evidence |
|---|---|
| P8-T02 detection | `lsblk --json` parse (disk, model, size, ESP, Windows, free space, removable); ESP mounts; Windows mount probe; hardware report (CPU, memory, GPU, Wi-Fi, audio, firmware) |
| P8-T03 planner + dry run | alongside with free space reuses the ESP; no free space plans an NTFS shrink with a warning; < 32 GB rejected; erase needs the disk confirmed; removable disks and existing AetherOS rejected; swap = min(RAM, 8 GiB); deterministic text/JSON dry run |
| P8-T04 boot menu | systemd-boot `loader.conf` + AetherOS and Windows entries; Windows file path untouched |
| P8-T06 uninstall plan | removes only AetherOS partitions in reverse order, removes its boot entry, restores the Windows Boot Manager, optional grow; no install → error |
| P8-T07 pre-checks | backup, BitLocker/recovery key, Fast Startup, NTFS health, free space, pending updates, AC/battery; blocking list blocks the plan |
| P8-T01 live image | **not done** — needs the image build (S1 territory) |
| P8-T05 Secure Boot | deferred to public release |

CLI:

```text
$ aether-installer detect
Firmware: BIOS
Disks: (no disks reported; lsblk unavailable on this host)

$ aether-installer apply --plan plan.json
APPLY_REFUSED: disk writes run only from the AetherOS live image.
```

## Honest limits

- macOS has no `/proc`, `/sys` or `lsblk`; detection reports "not reported"
  rather than inventing hardware. All parsing is tested with fixtures.
- Disk writes are intentionally not implemented here: applying runs only from
  the AetherOS live image (INS-12 dry run is what this code provides).
- INS-AC-01/03/04/05/06/07/09 need real machines and the live image.
- Theme consumption by the shell is an S3 binding; this crate provides tokens
  and reload semantics (tested).
- Hooks use the system `lua` interpreter with a hard timeout; an embedded VM
  is a follow-up.

## Reproduce

```bash
cargo test -p aether-config -p aether-installer
aetherctl --root /tmp/aether-config config check
aetherctl --root /tmp/aether-config features list
aether-installer detect
```
