# aether-support — first-boot diagnostic bundle (2026-11-28)

Owner requirement: when the graphical start fails, there must be a way to
diagnose it **even when SSH or networking is the thing that broke**. Today the
only offline fallback is "log in on tty2 and read what you can", which loses
the evidence as soon as the machine is power-cycled.

Status: **implemented and verified on macOS with a fake-root harness**
(42/42 cases). Not yet run on the Dell: the on-hardware confirmation of the
`--usb` path against a real FAT ESP is still open.

## Files

| Path | Purpose |
| --- | --- |
| `os-build/board/aetheros/overlay/usr/bin/aether-support` | the one implementation (POSIX sh, 153 lines) |
| `os-build/scripts/aether-support.sh` | symlink to the file above, same one-implementation rule as `aether-boot-switch` |
| `os-build/test/support-selftest.sh` | fake-root harness (42 assertions, runs on macOS) |

## How it is used on a broken machine

```
tty2 login (root)
  aether-support            -> /var/log/aether-support-<UTC timestamp>.tar.gz
  aether-support --usb      -> the same file on the FAT ESP (/dev/sda1)
  scp root@<ip>:<path> .    (printed by the tool; works only if networking does)
```

`--usb` mounts `/dev/sda1` read-write, copies the bundle as
`aether-support-<timestamp>.tar.gz`, runs `sync`, unmounts, and never leaves the
ESP mounted (the `EXIT` trap unmounts again if the run is interrupted). With no
ESP present it prints `aether-support: /dev/sda1 is not present; no USB copy`
and still exits 0, because the bundle in `/var/log/` is the thing that matters.

## Bundle member list

23 members, all under `aether-support-<timestamp>/` inside the tar:

```
uname.txt                          uname -a
proc-cmdline.txt                   /proc/cmdline (which aether.boot= and root= won)
dmesg.txt                          kernel ring, unfiltered
journalctl-boot.txt                journalctl -b --no-pager -n 5000
systemctl-failed.txt               systemctl --failed --no-pager
unit-aether-session.txt            status + journalctl -b -n 200 -u aether-session
unit-aether-shell.txt              status + journal, aether-shell
unit-aether-boot-return.txt        status + journal, aether-boot-return
unit-getty-tty2.txt                status + journal, getty@tty2 (the fallback console)
etc-aether-ui-build.txt            /etc/aether/ui-build.txt (drm vs headless build)
etc-os-release.txt                 /etc/os-release
var-log-aether-boot.log            /var/log/aether-boot.log
var-log-aether-compositor.log      /var/log/aether/compositor.log (note if absent)
var-log-aether-test.log            /var/log/aether-test.log (note if absent)
dev-dri.txt                        ls -l /dev/dri (card0 / renderD128 present?)
sys-class-drm-card<N>-<conn>-status.txt   every /sys/class/drm/card*-*/status
sys-class-drm-card<N>-<conn>-modes.txt    every /sys/class/drm/card*-*/modes
lsmod.txt                          lsmod
lspci.txt                          lspci -nnk (the GPU driver actually bound)
lsusb.txt                          lsusb
ip-4-addr.txt                       ip -4 addr (which address to scp to)
ip-route.txt                        ip route
systemd-analyze-blame.txt           systemd-analyze blame | head -20 (if available)
```

Every collector is best-effort: a missing tool or file writes a one-line note
`not available: journalctl` / `not present: /var/log/aether-test.log`, and the
run still exits 0 as long as the tar.gz exists. A failing command keeps its
output and gets an `(exit N)` line appended.

## Redaction rule

One filter, applied to **every** captured stream (files, command output and
unit journals):

```sh
sed -E 's/(psk|password|passphrase)[^ ]*/\1=<redacted>/Ig'
```

so `psk="supersecret"`, `Password=hunter2` and `WPA-PASSPHRASE=abcd` all become
`psk=<redacted>` / `Password=<redacted>`. Independently, the script **never**
reads `/run/aether-wpa.conf` and never dumps the environment, so a Wi-Fi
passphrase has no path into a bundle that could be emailed or pasted into an
issue. The harness proves this both ways: a `psk=` planted in
`/var/log/aether-boot.log` is rewritten, and a `psk=` planted in
`/run/aether-wpa.conf` never appears anywhere in the archive.

## Environment overrides (test only)

| Variable | Default | Use |
| --- | --- | --- |
| `AETHER_SUPPORT_ROOT` | `/` | prefix for every file the collector reads |
| `AETHER_SUPPORT_OUT` | `<root>/var/log` | where the tar.gz lands |
| `AETHER_SUPPORT_ESP` | `/dev/sda1` | ESP partition used by `--usb` |

## Verification (macOS, fake root)

```
$ bash os-build/test/support-selftest.sh
ok  1 - sh -n aether-support
ok  2 - bash -n aether-support
ok  3 - os-build/scripts/aether-support.sh is a symlink
ok  4 - host symlink resolves to the image binary
ok  5 - collection exits 0
ok  6 - bundle created under /var/log
ok  7 - bundle has the expected member list
ok  8 - proc cmdline captured
ok  9 - os-release captured
ok 10 - ui-build captured
ok 11 - drivers listed
ok 12 - unit status captured
ok 13 - journalctl boot log captured
ok 14 - journal is bounded to 5000 lines
ok 15 - unit journal is bounded to 200 lines
ok 16 - getty@tty2 is queried
ok 17 - systemd-analyze blame is head -20
ok 18 - drm status captured
ok 19 - drm modes captured
ok 20 - /dev/dri listed
ok 21 - compositor log captured
ok 22 - absent aether-test.log is a note
ok 23 - psk= is redacted
ok 24 - the psk secret is gone
ok 25 - the Wi-Fi password is gone
ok 26 - journal psk is redacted
ok 27 - aether-wpa.conf is never read
ok 28 - no wpa secret anywhere in the bundle
ok 29 - report names the bundle
ok 30 - report gives the size in bytes
ok 31 - report gives the scp hint
ok 32 - run without journalctl exits 0
ok 33 - missing journalctl is a note
ok 34 - missing journalctl is a note in the unit section
ok 35 - systemctl still captured
ok 36 - --usb without an ESP exits 0
ok 37 - --usb without an ESP says so
ok 38 - --usb without an ESP still lands the bundle
ok 39 - --usb without an ESP leaves the bundle in /var/log
ok 40 - nothing is left mounted
ok 41 - unknown argument exits 2
ok 42 - unknown argument explains itself

42 passed, 0 failed
```

`sh -n` and `bash -n` are both clean on the shipped script.

## What a first-boot failure looks like in the bundle

The bundle is designed so each failure mode is readable from one member, in the
order a human would ask the questions:

| Symptom on screen | First member to read | What it shows |
| --- | --- | --- |
| Black screen, no cursor | `unit-aether-session.txt` | `status=1/FAILURE`, `Main PID` churn from `Restart=on-failure`, and the compositor's own stderr in the journal section |
| Compositor exits, service restarts three times then gives up | `unit-aether-session.txt` + `var-log-aether-compositor.log` | `StartLimitBurst` reached (`start-limit-hit`) and the last `AETHER_COMPOSITOR_BACKEND:` line |
| “Headless” 60 s session instead of a picture | `var-log-aether-compositor.log` + `etc-aether-ui-build.txt` | `AETHER_COMPOSITOR_BACKEND: headless` next to a build manifest that says `drm` (the image shipped the wrong compositor) |
| No seat / permission denied on the GPU | `unit-aether-session.txt` + `dev-dri.txt` + `sys-class-drm-*-status.txt` | `LIBSEAT_BACKEND`/`/dev/dri` errors and whether `card0`/`renderD128` exist at all |
| `aether-compositor: not found` at tty | `unit-aether-session.txt` | `aether-session: /usr/bin/aether-compositor is missing (see /etc/aether/ui-build.txt)`, exit 127 |
| Desktop up but shell window missing | `unit-aether-shell.txt` | shell unit failed while `aether-session` is active |
| Nothing starts at all | `systemctl-failed.txt` + `journalctl-boot.txt` | the failed units and the boot journal, in `systemd-analyze blame` order |
| Kernel/driver suspicion | `dmesg.txt` + `lsmod.txt` + `lspci.txt` | i915/amdgpu probe errors and whether the driver module loaded |

The examples above are the shapes each member exposes, not a captured incident:
no graphical failure has been recorded on the Dell with this tool yet. The
`--usb` copy is the one path that still works when the failed start also took
networking down, which is the reason the FAT ESP copy exists at all.

## Known limits / risks

- `--usb` assumes `/dev/sda1` is the ESP. On a machine whose USB stick
  enumerates as `sdb` the copy is skipped with a message; use
  `AETHER_SUPPORT_ESP=/dev/sdb1` only if that is really the ESP.
- If the ESP is already mounted elsewhere, an extra read-write mount is taken
  and released; a dirty FAT could still need `fsck.vfat` on another machine.
- The bundle is a snapshot of `/var/log` and the current boot journal; after a
  power cycle the failing boot's journal is preserved by systemd's persistent
  journal only if `/var/log/journal` exists. Otherwise collect **before**
  rebooting.
- `journalctl -b -n 5000` can be large on a noisy boot; the size is printed so a
  copy over a slow link is a conscious choice.
- `aether-support` is not wired into any unit yet: it is a manual tty2 tool, as
  the owner asked for.
