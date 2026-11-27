# Boot switch — Ubuntu ⇄ AetherOS USB, no F12 (2026-11-28)

Owner requirement: rebuild the image, flash it from Ubuntu, then test it by
rebooting twice — no manual F12. The Dell must ping-pong between Ubuntu (internal
disk) and AetherOS (USB stick) automatically, and must never be left unbootable.

Status: **implemented and verified on macOS with a fake-efibootmgr harness**
(93/93 cases). Not yet exercised on the Dell: the device was unreachable
(read-only) in this session, so the on-hardware confirmation is still open.

## Files

| Path | Purpose |
| --- | --- |
| `os-build/board/aetheros/overlay/usr/bin/aether-boot-switch` | the one implementation (POSIX sh). Also the Ubuntu host helper. |
| `os-build/scripts/aether-boot-switch.sh` | symlink to the file above, so Ubuntu and the image cannot drift |
| `os-build/board/aetheros/overlay/usr/libexec/aether-boot-return.sh` | the every-boot systemd job on AetherOS |
| `os-build/board/aetheros/overlay/etc/systemd/system/aether-boot-return.service` | oneshot unit, `After=local-fs.target`, `RemainAfterExit=yes` |
| `os-build/test/boot-switch-selftest.sh` | fake-efibootmgr harness (93 assertions, runs on macOS) |

## Why `BootNext` and not `BootOrder`

`BootNext` is a *one-shot* variable: the firmware reads it, boots that entry,
and deletes it. That single property gives all the safety we need:

- A wrong or stale choice cannot persist — the following boot uses `BootOrder`.
- If the target stick is unplugged, the firmware ignores the missing entry and
  falls through to the normal `BootOrder`, so the machine still boots Ubuntu.
- The switch cannot loop, because each direction has to be re-armed explicitly.

`BootOrder` is therefore never rewritten by a switch. The only write that
touches it is `efibootmgr -c`, which prepends the new `AetherOS` entry when it
does not exist yet; the script logs `BootOrder before` / `BootOrder after` around
that call, and even a leftover entry in the order is skipped by the firmware when
the USB is absent.

## State machine

State is `(BootNext, BootCurrent)` plus one runtime flag. `AETHER_BOOT_SWITCH:`
lines are the machine-readable form of every transition.

```
        Ubuntu booted (BootCurrent = internal)
                 │
                 │  aether-boot-switch aether        (owner: one command)
                 ▼
        BootNext = <AetherOS entry>        [result=switched | created]
                 │  reboot (fw uses BootNext, then deletes it)
                 ▼
        AetherOS booted (BootCurrent = USB)
                 │
                 ├── aether.boot=stay on cmdline, or /run/aether-boot-stay
                 │        → unit logs and exits; BootNext stays unset
                 │          (target=AetherOS; explicit manual stay)
                 │
                 └── normal boot
                          │  aether-boot-return.service → aether-boot-switch ubuntu
                          ▼
                 BootNext = <internal disk entry>   [result=switched]
                          │  reboot
                          ▼
                 Ubuntu booted, BootNext consumed and gone
                 (the cycle is idle until `aether` is run again)
```

Idempotency: if `BootNext` already points at the target, no write happens at all
(`result=already`), so the unit can run on every boot without churn.

## What the owner runs

On Ubuntu, once per flash-and-test cycle:

```sh
sudo os-build/scripts/aether-boot-switch.sh status    # read-only sanity check
sudo os-build/scripts/aether-boot-switch.sh aether    # next boot -> AetherOS
sudo reboot
```

After the AetherOS side has done its work, `sudo reboot` again returns to Ubuntu.
Nothing else is needed; F12 is no longer part of the loop.

On AetherOS (optional):

```sh
aether-boot-switch status       # what the next boot will be
aether-boot-switch stay         # keep AetherOS for the next boot as well
reboot
```

Recovery, if a pathologically stale `BootNext` is ever suspected:

```sh
sudo aether-boot-switch clear   # delete BootNext; the next boot follows BootOrder
sudo aether-boot-switch status
```

Expected output (stable marker line, one per invocation):

```
AETHER_BOOT_SWITCH: next=0001 label=AetherOS current=0000 result=switched
```

`status` on a dual-boot Dell with a Windows entry present looks like:

```
AetherOS boot switch
  BootCurrent : 0003
  BootNext    : 0002 (Windows Boot Manager)
  BootOrder   : 0003,0000,0001,0002
  Entries     :
    0000 on  ubuntu
    0001 on  UEFI: SanDisk Extreme 0001
    0002 on  Windows Boot Manager
    0003 on  AetherOS
  stay flag   : absent
AETHER_BOOT_SWITCH: next=0002 label=Windows Boot Manager current=0003 result=status
```

## Entry selection rules

AetherOS target (`aether`):

1. An *active* entry whose label contains `aether` (case-insensitive). If more
   than one matches, the one whose EFI device path contains the USB ESP's
   PARTUUID wins; anything still ambiguous is refused.
2. If none exists: create it with
   `efibootmgr -c -d <usb disk> -p 1 -L AetherOS -l '\EFI\BOOT\BOOTX64.EFI'`,
   then re-read the table and confirm the entry is there before touching
   `BootNext`.
3. The USB disk is found as `/dev/disk/by-label/AETHEROS` (the live image
   formats the ESP with `-n AETHEROS`), then `AETHEROS-ESP`, then
   `/dev/disk/by-partlabel/AETHEROS-ESP`, then `blkid -L AETHEROS`; the partition
   number and parent disk are derived from that device (`sdb1` → `sdb`, `1`;
   `nvme0n1p1` → `nvme0n1`, `1`). Overrides: `AETHER_USB_DISK`,
   `AETHER_USB_PART`, `AETHER_USB_PARTNUM`, `AETHER_USB_PARTUUID`.

Ubuntu target (`ubuntu`):

1. An *active* entry whose label contains `ubuntu` → used.
2. Fallback: "the entry that is neither the USB nor AetherOS" — every active
   entry whose label is not `aether`/`usb` and whose device path does not
   reference the USB ESP PARTUUID. Exactly one such entry is used; zero is a
   no-op; more than one is refused with the candidate list (label the internal
   entry `ubuntu` and it becomes deterministic).
3. Never falls back to "any entry" when the table has only one OS entry: that is
   reported as "only one OS entry exists (1); nothing to switch back to".

## Safety rules (all enforced in code and covered by the selftest)

| Rule | Implementation |
| --- | --- |
| Never write `BootOrder` to switch | only `efibootmgr -n` / `-N` are used; `-c` prepends the new entry once and is logged before/after |
| Never point `BootNext` at a non-existent entry | `set_next` refuses unless the index is in the table just read |
| Inactive entries are not used | the pickers require the `*` (active) marker; inactive matches produce a no-op with an explanation |
| Missing target is a no-op, never a guess | picks exit 1 (none) or 2 (ambiguous), both print `result=noop` and change nothing |
| Every write is verified | after `efibootmgr -n`, `BootNext` is re-read; mismatch or non-zero exit → exit 4, `result=failed`, nothing else touched |
| No `efibootmgr`, no half-write | exit 3 no-op with an "install BR2_PACKAGE_EFIBOOTMGR=y / apt install efibootmgr" hint; the systemd unit treats it as success |
| Cannot strand the machine | `BootNext` is consumed by firmware, so the worst case is one unexpected boot, then `BootOrder` |
| Explicit escape hatch | `aether.boot=stay` on the cmdline or `/run/aether-boot-stay` stops the return for that boot |

`efivarfs` entries are immutable, so the script never writes efivars directly;
`efibootmgr` is what handles the immutable/pending model. On a non-EFI boot the
unit is skipped by `ConditionPathExists=/sys/firmware/efi/efivars`.

## A boot cycle, step by step

1. Ubuntu: `sudo os-build/scripts/aether-boot-switch.sh aether`
   → `efibootmgr -n 0001` (creating `0001` first if needed) → read-back confirms
   → `AETHER_BOOT_SWITCH: next=0001 label=AetherOS current=0000 result=switched`.
2. `sudo reboot`. Firmware sees `BootNext=0001`, boots the USB, deletes `0001`
   from `BootNext`.
3. AetherOS boots. `aether-boot-return.service` starts after `local-fs.target`,
   runs `aether-boot-switch ubuntu`, which writes `BootNext=<ubuntu>` and
   verifies it. Journal and `/var/log/aether-boot.log` both get
   `AETHER_BOOT_SWITCH: next=0000 label=ubuntu current=0001 result=switched`
   (the file is on tmpfs, so the journal is the durable copy).
4. `reboot`. Firmware boots Ubuntu and deletes `BootNext`. The machine is idle in
   the safe state; run step 1 again for the next test round.

Net effect: one command before the first reboot, nothing after that.

## Failure modes and what they look like

| Symptom | Cause | What the software does | Owner action |
| --- | --- | --- | --- |
| `result=noop` + "no internal-disk (Ubuntu) entry found" | Ubuntu entry relabelled/removed | nothing written; unit exits 0 | `efibootmgr -v`, relabel with `efibootmgr -b XXXX -L ubuntu` |
| `result=noop` + "only one OS entry exists" | fresh single-OS install, or the USB entry was deleted | nothing written | nothing, or create the other entry |
| `result=noop` + "an AetherOS entry exists but is deactivated" | entry toggled off in firmware setup | nothing written | `efibootmgr -b XXXX -a`, or delete it |
| `result=noop` + "more than one entry could be the internal disk" | several OS entries and none labelled `ubuntu` | refuses to guess, lists candidates | label the internal one `ubuntu` |
| `result=noop` + "efibootmgr is not available" | image built without `BR2_PACKAGE_EFIBOOTMGR` | no read, no write; unit exits 0 | rebuild with the package (see requests below) |
| `result=failed` + "was not confirmed" | firmware refused `SetVariable` (RO efivars, firmware bug, fw password) | `result=failed`, exit 4, `BootOrder` untouched | `aether-boot-switch status`; if a stale value persists, `aether-boot-switch clear` |
| `result=failed` + "efibootmgr -c failed" | no space in NVRAM, or wrong disk | stops before touching `BootNext` | `efibootmgr -v`; remove stale entries with `efibootmgr -b XXXX -B` |
| Unit logs "cannot write /run/aether-boot-stay" | ran `stay` as non-root | no write | `sudo aether-boot-switch stay` |
| AetherOS booted again after a reboot | `aether.boot=stay` on that boot's cmdline, or `/run/aether-boot-stay` present | intended | `aether-boot-switch status`; nothing else |

## Verification done in this session (macOS, no UEFI)

```
$ sh -n os-build/board/aetheros/overlay/usr/bin/aether-boot-switch      # clean
$ dash -n os-build/board/aetheros/overlay/usr/bin/aether-boot-switch    # clean
$ bash -n os-build/board/aetheros/overlay/usr/libexec/aether-boot-return.sh   # clean
$ bash -n os-build/test/boot-switch-selftest.sh                        # clean
$ os-build/test/boot-switch-selftest.sh
...
93 passed, 0 failed        (exit 0)
```

The harness covers: entry exists → write + read-back; entry missing → created
then written; no Ubuntu entry → no-op; one OS entry → no-op; `aether.boot=stay`
and `/run/aether-boot-stay` → no write; already-correct → idempotent with zero
`efibootmgr -n` calls; write failure and silent write failure → exit 4 with
`BootNext` unchanged; missing `efibootmgr` → exit 3 no-op and a unit that still
exits 0; ambiguous and inactive entries → refused; unreadable table → no-op;
`clear`; a full round trip; `BootOrder` untouched by a switch; and a captured
real `efibootmgr -v` dump with spaced labels and a `UEFI: ...` USB entry.

Also exercised manually on macOS: the degraded path with no `efibootmgr`
(`status` → exit 3 no-op, return unit → exit 0 no-op) and the whole command set
under `dash` (Ubuntu's `/bin/sh`).

Not verified: the Dell itself. `192.168.68.150` did not answer `ping`/`ssh` in
this session and is read-only tonight, so `efibootmgr -v` output shape and the
`efivarfs` layout could not be captured from hardware; the parser is tested
against the documented output shape instead. First confirmation comes from the
first real cycle (step 1 above, then step 3's marker in `journalctl -u
aether-boot-return -b`).

## Requests for the sessions that own the adjacent files

1. **defconfig (ui-image session, `os-build/buildroot/aetheros_defconfig`)**:
   add `BR2_PACKAGE_EFIBOOTMGR=y`. Buildroot selects `efivar` and `popt` as its
   dependencies, so no extra lines are expected; if the build complains, use
   `BR2_PACKAGE_EFIVAR=y` + `BR2_PACKAGE_POPT=y` explicitly. Without the package
   the unit degrades to a documented no-op (`result=noop`, exit 3 → unit exit 0)
   and the ping-pong stops working, so this is required for the owner requirement.
2. **systemd enablement (the session that owns
   `overlay/etc/systemd/system/`)**: add the symlink
   `overlay/etc/systemd/system/multi-user.target.wants/aether-boot-return.service`
   → `../aether-boot-return.service`. Same convention as
   `multi-user.target.wants/show-ip.service` already in the tree. Until then the
   unit ships but is not started, and nothing hands the boot back.
