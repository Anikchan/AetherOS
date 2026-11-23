# AetherOS — User Guide (draft, 2026-11-23)

This guide matches the code that exists today. Steps that need the AetherOS
live image or hardware are marked **pending**.

## 1. What AetherOS is

A light Linux desktop with an AetherOS shell, a process manager that suspends
and hibernates inactive apps, battery-aware power management, and an app
platform with permissions and a sandbox.

## 2. Trying and installing

1. **Live USB** — **pending** (P8-T01). Until then the desktop can be built and
   run from source on Linux.
2. **Hardware report** — `aether-installer detect` lists CPU, memory, GPU,
   Wi-Fi, audio, firmware (UEFI/BIOS), disks, and Windows/BitLocker state.
3. **Pre-checks** — the installer blocks on: no backup, BitLocker without a
   saved recovery key, Windows Fast Startup on, an unclean NTFS volume, less
   than 32 GB free, or a low battery without AC.
4. **Choose a mode** — beside Windows or erase the disk. The dry-run plan shows
   every partition and operation before anything is written.
5. **Review and confirm** — disk writes run only from the live image
   (`aether-installer apply` refuses elsewhere).
6. **Uninstall** — `aether-installer uninstall-plan --disk ...` removes only
   AetherOS partitions and its boot entry; the Windows Boot Manager stays.

## 3. Installing apps

```bash
# Inspect a bundle without changing anything
aether-pkg inspect ./MyApp

# Install (asks about permissions)
aether-pkg install ./MyApp

# List / remove
aether-pkg list
aether-pkg uninstall org.example.MyApp --yes
```

Every app gets its own data folder (`~/.local/share/aether/apps/<id>/data`)
and a `.desktop` entry. AppImages work directly; Windows installers need the
Windows engine pack.

### Permissions

Apps ask for network, files (only files you pick), camera, microphone,
location, notifications, or background running. A denied permission stays
denied. `aether-sandbox list APP-ID` shows grants; `aether-sandbox grant` and
`revoke` change them.

## 4. Activity Manager

The core is available as `aether-taskmanager`:

```bash
aether-taskmanager --once          # process + system snapshot
aether-taskmanager --tui           # live table (Ctrl+C to exit)
aether-taskmanager --end-task 1234 # dry run; --yes to act
aether-taskmanager --efficiency 1234 on --yes
aether-taskmanager --export report.csv --format csv
aether-taskmanager --spike-s7      # sensors/GPU availability
```

Columns: name, PID, state, CPU %, memory, disk, GPU (when reported), energy
impact, wakeups, threads, open files. Efficiency mode lowers the app's CPU
priority; End task refuses PID 1 and the manager itself.

## 5. Battery and power

```bash
aether-powerd --once          # one sample + status JSON
aether-power-report           # battery health, cycles, sessions, energy by app
powerd/bench/battery-sampler.sh W1   # 1-hour idle benchmark
```

Profiles switch to **Saver** on battery and **Balanced** on AC unless you chose
a profile for that power source. At 20% you get a warning; at 10% Saver is
forced; at 5% the configured action runs (suspend or hibernate); at 3% it
hibernates when a swap file exists, otherwise suspends. Lid close suspends when
not charging.

## 6. Settings and `aetherctl`

Settings are files. Layering: `/usr/share/aether/defaults` → `/etc/aether` →
`~/.config/aether`.

```bash
aetherctl config get dock.position
aetherctl config set dock.position left
aetherctl config reset dock.position
aetherctl config reset --file shell/dock.toml
aetherctl config check
aetherctl reload
aetherctl theme show
aetherctl keybindings check
aetherctl hooks list
aetherctl features list
aetherctl apps list
```

Invalid values are rejected; the last valid configuration stays active.

## 7. Optional packs

`aetherctl features list` shows Android, Windows, AI, advanced monitoring,
Screen Time limits, battery reports, and the storage analyzer with disk and
idle cost. `aetherctl features install D` refuses if the idle budget would be
exceeded.

## 8. Troubleshooting

| Symptom | Check |
|---|---|
| App will not launch | `aether-sandbox doctor` — sandbox enforcement is mandatory |
| "Estimating..." battery time | normal for the first 2 minutes after a power change |
| No temperature/GPU readings | `aether-taskmanager --spike-s7` shows what the hardware reports |
| Settings change ignored | `aetherctl config check`; an invalid file is reported and skipped |
| Windows does not appear in the boot menu | reinstall the bootloader from the live image (repair pending) |

## 9. Status of this guide

- CLI behaviour is tested (`docs/2026/11/23/results/`).
- Live USB, disk writes, real battery gates, Secure Boot, and the 30-day
  daily-driver trial are still **pending** (P8/P9/P10).
