# Boot switch — Ubuntu ⇄ AetherOS USB, no F12 (2026-11-28)

Owner requirement: rebuild the image, flash it from Ubuntu, then test it by
rebooting twice — no manual F12. The Dell must ping-pong between Ubuntu (internal
disk) and AetherOS (USB stick) automatically, and must never be left unbootable.

Status: **implemented and verified on macOS with a fake-efibootmgr harness**
(245/245 cases). Not yet exercised on the Dell: the device was unreachable
(read-only) in this session, so the on-hardware confirmation is still open.

## Decision: `BootNext` (default) versus `--persistent` (`BootOrder`)

Both modes move the same two entries; they differ only in what survives a power
loss. The owner asked whether the switch should also be done through `BootOrder`
from both sides — it can, but it must be opt-in, because it is the mode that can
strand the machine.

| | `BootNext` (default) | `--persistent` (`BootOrder`) |
| --- | --- | --- |
| Survives a power loss during the AetherOS test | No — the firmware already consumed it, so the next boot is Ubuntu | Yes — AetherOS keeps booting until something restores the order |
| Can strand the machine on AetherOS | No; the worst case is one unexpected boot | **Yes**, if AetherOS never reaches `aether-boot-return.service` |
| Writes `BootOrder` | Never | Yes: AetherOS to the front, the previous order recorded first |
| Reversal | Automatic (firmware deletes the variable) | `ubuntu --persistent`, the boot-time unit, or `clear` |
| Use it when | normal flash-and-test loops, unattended boots, anything you want fail-safe | a test must survive a hard power cut / a firmware that ignores `BootNext`, or the owner explicitly wants the USB sticky |

Stated plainly: with `--persistent`, if the AetherOS image does not boot far
enough to run `aether-boot-return.service` (bad kernel, panic before
`local-fs.target`, USB removed before boot), the machine comes back up on
AetherOS every time. Recovery is either the `stay` escape (which keeps AetherOS
on purpose) or, from AetherOS's own console, `aether-boot-switch ubuntu
--persistent`, or in the worst case `efibootmgr -o <recorded order>` in firmware
setup. That is why `BootNext` is the default and `--persistent` is never implied.

## Files

| Path | Purpose |
| --- | --- |
| `os-build/board/aetheros/overlay/usr/bin/aether-boot-switch` | the one implementation (POSIX sh). Also the Ubuntu host helper. |
| `os-build/scripts/aether-boot-switch.sh` | symlink to the file above, so Ubuntu and the image cannot drift |
| `os-build/board/aetheros/overlay/usr/libexec/aether-boot-return.sh` | the every-boot systemd job on AetherOS |
| `os-build/board/aetheros/overlay/etc/systemd/system/aether-boot-return.service` | oneshot unit, `After=local-fs.target`, `RemainAfterExit=yes` |
| `os-build/test/boot-switch-selftest.sh` | fake-efibootmgr + fake-ESP harness (245 assertions, runs on macOS) |
| `/var/lib/aether-boot/original-order` (runtime) | the recorded `BootOrder` that makes a persistent switch reversible |

## Why `BootNext` is the default

`BootNext` is a *one-shot* variable: the firmware reads it, boots that entry,
and deletes it. That single property gives all the safety we need:

- A wrong or stale choice cannot persist — the following boot uses `BootOrder`.
- If the target stick is unplugged, the firmware ignores the missing entry and
  falls through to the normal `BootOrder`, so the machine still boots Ubuntu.
- The switch cannot loop, because each direction has to be re-armed explicitly.

In the default mode `BootOrder` is only touched by `efibootmgr -c`, which
prepends the new `AetherOS` entry when it does not exist yet; the script logs
`BootOrder before` / `BootOrder after` around that call, and even a leftover
entry in the order is skipped by the firmware when the USB is absent.

## State machine

State is `(BootNext, BootCurrent, BootOrder, <state>/original-order)` plus one
runtime flag. Every transition prints one marker line:

```
AETHER_BOOT_SWITCH: next=<idx|none> label=<label|none> current=<idx|none> result=<...> mode=<...>
```

`mode` is what will actually happen on the next boot: `next` (`BootNext` wins),
`persistent` (`BootOrder` starts with the AetherOS entry), `default` (neither),
or `unknown` (the table could not be read/confirmed).

Default mode (one-shot `BootNext`):

```
        Ubuntu booted (BootCurrent = internal)
                 │
                 │  aether-boot-switch aether        (owner: one command)
                 ▼
        BootNext = <AetherOS entry>        [result=switched | created, mode=next]
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
                 BootNext = <internal disk entry>   [result=switched, mode=next]
                          │  reboot
                          ▼
                 Ubuntu booted, BootNext consumed and gone
                 (the cycle is idle until `aether` is run again)
```

Persistent mode (`--persistent`, opt-in):

```
        Ubuntu booted, order = ubuntu,fedora,aether
                 │  aether-boot-switch aether --persistent
                 │     1. write the ESP marker:  <esp>/aether-boot-mode
                 │           mode=persistent
                 │           original=ubuntu,fedora,aether
                 │           armed_utc=2026-11-28T03:55:02Z
                 │        (refused outright if the ESP cannot be written)
                 │     2. efibootmgr -o aether,ubuntu,fedora   (verified)
                 ▼
        BootOrder = aether,ubuntu,fedora   [result=switched, mode=persistent]
                 │  reboot (no BootNext involved)
                 ▼
        AetherOS booted
                 │  unit: `aether-boot-switch mode` (ESP mounted read-only)
                 │        prints persistent because the marker is there
                 │        → aether-boot-switch ubuntu --persistent
                 │        → efibootmgr -o ubuntu,fedora,aether (verified)
                 │        → marker deleted (ESP remounted read-write for that
                 │          one operation, then unmounted)
                 ▼
        BootOrder = ubuntu,fedora,aether  ← byte for byte what it was
                 │  reboot
                 ▼
        Ubuntu booted, mode=default
```

And the case this exists to protect — the owner's own choice:

```
        BootOrder = aether,ubuntu   (set in BIOS, or armed and already undone)
        no marker on the ESP
                 │  AetherOS boots
                 ▼
        unit: mode=owner → logs "that is the owner's choice" and exits
                 │  no BootNext, no BootOrder write: AetherOS stays the default
                 ▼
        reboot → AetherOS again, for ever, until the owner says otherwise
        (aether-boot-switch ubuntu --persistent is how they say otherwise)
```

Idempotency: if `BootNext` already points at the target, or `BootOrder` already
equals the recorded order, no write happens at all (`result=already`), so the
unit can run on every boot without churn.

### Where the armed state lives, and why it is the ESP

AetherOS mounts `/var` from tmpfs (`overlay/etc/fstab`), so
`/var/lib/aether-boot/original-order` — written on Ubuntu — simply does not exist
on the AetherOS side. Anything the two systems must agree on has to live on a
store both can read, and for this machine that is the USB ESP.

So a persistent arm writes a plain-text marker next to `BOOTX64.EFI`:

```
<esp>/aether-boot-mode
  mode=persistent
  original=0000,0001
  armed_utc=2026-11-28T03:55:02Z
```

Rules around it:

- **Written before `BootOrder`, and the arm is refused if it cannot be written.**
  A persistent switch the AetherOS side cannot see would never be undone, which
  is precisely the failure this replaces.
- **Read on a read-only mount.** `mode`, `status` and the restore decision never
  have write access to the ESP.
- **Deleted on a read-write mount held only for that one operation**, then
  remounted read-only and unmounted.
- **Absent means "not ours".** `BootOrder` starting with AetherOS *without* a
  marker is the owner's choice (BIOS, or an arm that was already undone) and the
  unit leaves the boot table untouched: no `BootOrder` write and no `BootNext`
  write. `mode` reports that state as `owner`.
- `/var/lib/aether-boot/original-order` is kept only as a convenience copy for
  commands run on the same side (`--persistent`, `clear`). When both exist the
  marker wins; a `BootOrder` entry named in the marker but since deleted is
  dropped, and an entry that still exists is put back in its recorded place.

The unit does the mount through `aether-boot-switch` (one implementation, and the
same code runs on Ubuntu), so the ESP is mounted exactly once per decision and
always unmounted again — `os-build/test/boot-switch-selftest.sh` records the
`mount`/`umount` calls and asserts the `-o ro` mount plus the unmount.

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

Recovery, if a pathologically stale `BootNext` or persistent order is ever
suspected:

```sh
sudo aether-boot-switch clear   # restore the recorded order, drop it, drop BootNext
sudo aether-boot-switch status
```

Persistent variant of the same cycle (opt-in):

```sh
sudo os-build/scripts/aether-boot-switch.sh aether --persistent
sudo reboot                      # AetherOS boots with no BootNext involved
# AetherOS boots, the unit restores the order, then:
sudo reboot                      # back on Ubuntu, order byte-identical
# or, from AetherOS, to undo it before rebooting:
aether-boot-switch ubuntu --persistent
```

`--persistent` arms a marker on the AetherOS stick's ESP
(`<esp>/aether-boot-mode`), so the stick must be plugged in and the ESP writable
when it is run; if it is not, the command refuses and changes nothing. That is
deliberate: an arm the AetherOS side cannot see would never be undone.

Expected output (stable marker line, one per invocation):

```
AETHER_BOOT_SWITCH: next=0001 label=AetherOS current=0000 result=switched mode=next
AETHER_BOOT_SWITCH: next=none label=AetherOS current=0000 result=switched mode=persistent
AETHER_BOOT_SWITCH: next=none label=ubuntu current=0001 result=restored mode=default
```

`status` on a dual-boot Dell with a Windows entry present looks like:

```
AetherOS boot switch
  BootCurrent : 0003
  BootNext    : 0002 (Windows Boot Manager)
  BootOrder   : 0003,0000,0001,0002
  Mode        : BootNext=0002 (one-shot; the firmware deletes it)
  Entries     :
    0000 on  ubuntu
    0001 on  UEFI: SanDisk Extreme 0001
    0002 on  Windows Boot Manager
    0003 on  AetherOS
  stay flag   : absent
AETHER_BOOT_SWITCH: next=0002 label=Windows Boot Manager current=0003 result=status mode=next
```

and with a persistent switch armed:

```
  BootOrder   : 0003,0000,0001,0002
  Mode        : persistent (armed marker on the ESP, original 0000,0001,0002)
  Marker      : present (aether-boot-mode)
  Recorded    : 0000,0001,0002 (/var/lib/aether-boot/original-order, convenience copy)
```

and when AetherOS is first with no marker (the owner's choice):

```
  BootOrder   : 0003,0000,0001,0002
  Mode        : owner (AetherOS first, no armed marker: left alone)
  Marker      : absent (aether-boot-mode)
AETHER_BOOT_SWITCH: next=none label=none current=0003 result=status mode=owner
```

If the firmware does not report `BootOrder` at all, `status` says
`BootOrder   : not reported by this firmware (BootCurrent=0003)` and
`--persistent` refuses with a no-op instead of writing something it cannot
verify; the one-shot mode keeps working.

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
| `BootOrder` is touched only in `--persistent` mode | default mode uses `efibootmgr -n` / `-N` only; `-c` prepends the new entry once and is logged before/after |
| Never write `BootOrder` without a way back | the previous order is recorded *before* the write, in `<state>/original-order` (tmp file + `mv`); a failed write keeps the record |
| Never point `BootNext` at a non-existent entry | `set_next` refuses unless the index is in the table just read |
| Never put a non-existent entry in `BootOrder` | every order is rebuilt with `order_keep` (only indexes in the live table, de-duplicated) |
| Inactive entries are not used | the pickers require the `*` (active) marker; inactive matches produce a no-op with an explanation |
| Missing target is a no-op, never a guess | picks exit 1 (none) or 2 (ambiguous), both print `result=noop` and change nothing |
| Every write is verified | `BootNext`/`BootOrder` are re-read after the write; mismatch or non-zero exit → exit 4, `result=failed` |
| A failed `BootOrder` write is rolled back | the previous order is written back and re-read; confirmed → "table is exactly as before", unconfirmed → exit 4 naming the manual command |
| Nothing is ever half-written | the target order is computed in full, from the live table, before any write; a failure either rolls back or leaves the table alone |
| No `efibootmgr`, no half-write | exit 3 no-op with an "install BR2_PACKAGE_EFIBOOTMGR=y / apt install efibootmgr" hint; the systemd unit treats it as success |
| No verifiable `BootOrder`, no persistent mode | `--persistent` refuses (exit 1, `result=noop`) when the firmware does not report `BootOrder` |
| Cannot strand the machine (default mode) | `BootNext` is consumed by firmware, so the worst case is one unexpected boot, then `BootOrder` |
| Explicit escape hatch | `aether.boot=stay` on the cmdline or `/run/aether-boot-stay` stops the return for that boot (also in persistent mode) |
| The arm must be visible to AetherOS | the ESP marker is written *before* `BootOrder`; if it cannot be written the arm is refused (`result=noop`, exit 1) and `BootOrder` is untouched |
| The ESP is read-only unless it must be written | `mode`/`status`/the restore decision use an `-o ro` mount; the marker is removed on a read-write mount held only for that operation, then remounted `ro` and unmounted |
| A `BootOrder` without a marker is never touched | `mode=owner` → the unit writes neither `BootOrder` nor `BootNext` |
| A failed arm un-arms itself | if the `BootOrder` write is not confirmed the marker is deleted again, so the other side never sees a half-done arm |
| `clear` cannot leave a switch armed | restores the recorded order, drops the record, drops `BootNext`; if the restore fails the record is kept and the command exits 4 |

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

Persistent cycle, step by step (`--persistent`):

1. Ubuntu: `sudo os-build/scripts/aether-boot-switch.sh aether --persistent`
   → record `0000,0001` in `/var/lib/aether-boot/original-order` → `efibootmgr -o
   0001,0000` → read-back confirms → `mode=persistent`, `result=switched`.
2. `sudo reboot`. No `BootNext` is involved; the firmware reads `BootOrder`.
3. AetherOS boots. The unit asks `aether-boot-switch mode`, gets `persistent`
   (the live order starts with the AetherOS entry), runs `ubuntu --persistent`,
   which writes the recorded order back and drops the record. Marker:
   `AETHER_BOOT_SWITCH: next=none label=ubuntu current=0001 result=restored mode=default`.
4. `reboot` → Ubuntu, with `BootOrder` byte for byte what it was before step 1.

## Failure modes and what they look like

| Symptom | Cause | What the software does | Owner action |
| --- | --- | --- | --- |
| `result=noop` + "no internal-disk (Ubuntu) entry found" | Ubuntu entry relabelled/removed | nothing written; unit exits 0 | `efibootmgr -v`, relabel with `efibootmgr -b XXXX -L ubuntu` |
| `result=noop` + "only one OS entry exists" | fresh single-OS install, or the USB entry was deleted | nothing written | nothing, or create the other entry |
| `result=noop` + "an AetherOS entry exists but is deactivated" | entry toggled off in firmware setup | nothing written | `efibootmgr -b XXXX -a`, or delete it |
| `result=noop` + "more than one entry could be the internal disk" | several OS entries and none labelled `ubuntu` | refuses to guess, lists candidates | label the internal one `ubuntu` |
| `result=noop` + "efibootmgr is not available" | image built without `BR2_PACKAGE_EFIBOOTMGR` | no read, no write; unit exits 0 | rebuild with the package (see requests below) |
| `result=noop` + "does not report BootOrder" | firmware with no `BootOrder` variable | `--persistent` refuses before writing; one-shot mode still works | use the default mode |
| `result=noop` + "no restore order can be derived" | AetherOS is the only entry, or already first with nothing else | nothing written (a switch that could not be undone is not made) | create the other entry first |
| `result=failed` + "was not confirmed" | firmware refused `SetVariable` (RO efivars, firmware bug, fw password) | exit 4; `BootNext`/`BootOrder` re-read and rolled back | `aether-boot-switch status`; `aether-boot-switch clear` |
| `result=noop` + "cannot write the armed marker" | ESP not writable/not present (stick unplugged, mounted read-only, permissions) | refuses to arm before touching `BootOrder` | plug the stick in, `sudo`, or set `AETHER_USB_PART` / `AETHER_ESP_DIR` |
| `result=failed` + marker "could not be removed" | order restored but the ESP would not take the delete | order is correct, the marker stays, exit 4 | `sudo rm <esp>/aether-boot-mode` (or reboot AetherOS once more, which retries) |
| unit logs "that is the owner's choice" | `BootOrder` starts with AetherOS and there is no marker | nothing is written, exit 0 | intended; use `aether-boot-switch ubuntu --persistent` to change it |
| `result=failed` + "efibootmgr -c failed" | no space in NVRAM, or wrong disk | stops before touching `BootNext` | `efibootmgr -v`; remove stale entries with `efibootmgr -b XXXX -B` |
| `result=failed` + "recorded order ... has no entry that still exists" | every index in the record was deleted | table untouched, record kept so it can be fixed by hand | `efibootmgr -v`, then `clear` or re-arm |
| persistent restore "did not complete" in the unit log | order write refused on the AetherOS side too | the unit falls back to a one-shot `BootNext` return, so the machine still comes back | `efibootmgr -o <recorded order>` in firmware setup |
| machine keeps booting AetherOS after a power cut | `--persistent` was armed and AetherOS never reached `local-fs.target` | nothing can run before the kernel boots: this is the documented trade-off | use the default `BootNext` mode for unattended runs; otherwise `reboot`, then `aether-boot-switch ubuntu --persistent` |
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
245 passed, 0 failed        (exit 0)
```

The harness covers, for the default mode: entry exists → write + read-back;
entry missing → created then written; no Ubuntu entry → no-op; one OS entry →
no-op; `aether.boot=stay` and `/run/aether-boot-stay` → no write;
already-correct → idempotent with zero `efibootmgr -n` calls; write failure and
silent write failure → exit 4 with `BootNext` unchanged; missing `efibootmgr` →
exit 3 no-op and a unit that still exits 0; ambiguous and inactive entries →
refused; unreadable table → no-op; `clear`; a full round trip; `BootOrder`
untouched; a captured real `efibootmgr -v` dump with spaced labels and a
`UEFI: ...` USB entry.

And for the persistent mode: arm writes `BootOrder` and the record with no
`BootNext`; a second arm writes nothing; the full round trip restores the order
**byte for byte** and drops the record; restore with no record puts the internal
disk first and AetherOS last; no Ubuntu entry → no write; AetherOS entry missing
→ no write; a record referencing a deleted entry has the dead index dropped; a
record with nothing reachable left → exit 4 with the table untouched and the
record kept; `BootOrder` reported but not writable → exit 4, order unchanged,
record kept for the retry; a silent `BootOrder` failure → exit 4 with the
read-back named; duplicate indexes collapsed; AetherOS already first with no
record → a restorable record is derived and nothing is written; AetherOS as the
only entry → refused; `status` renders `mode=next|persistent|default` and the
"not reported" firmware case; `clear` restores the order, drops the record and
drops `BootNext`, and keeps the record (exit 4) when the restore fails; the unit
drops a stale record on the non-persistent path; and the tmpfs case (record
deleted, as AetherOS sees `/var/lib`) still restores byte for byte from the ESP
marker.

For the ESP marker specifically: the arm writes `mode=persistent`,
`original=<order>` and `armed_utc=<ts>`; two `status` reads leave it in place; the
restore and `clear` remove it; a marker naming a deleted entry has it dropped; a
marker whose entry is missing from `BootOrder` puts it back in its recorded
place; a marker that cannot be deleted is reported honestly (exit 4, order still
restored); an unwritable ESP refuses the arm with no `BootOrder` write at all;
`BootOrder` starting with AetherOS and **no** marker produces zero writes and
`mode=owner`; and a fake `mount`/`umount` pair proves the ESP is mounted `-o ro`
for the decision and unmounted afterwards, with no needless `remount,rw`.

Also exercised manually on macOS: the degraded path with no `efibootmgr`
(`status` → exit 3 no-op, return unit → exit 0 no-op) and arm → status → unit
restore → clear under `dash` (Ubuntu's `/bin/sh`).

Not verified: the Dell itself. `192.168.68.150` did not answer `ping`/`ssh` in
this session and is read-only tonight, so `efibootmgr -v` output shape, the
`efivarfs` layout and whether this firmware accepts `efibootmgr -o` could not be
captured from hardware; the parser and both write paths are tested against the
documented output shape instead. First confirmation comes from the first real
cycle (step 1 above, then step 3's marker in `journalctl -u aether-boot-return
-b`).

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
