# F5 — Installer and Dual Boot

Owner component: `aether-install` (installer), `aether-boot` (boot entries and Windows detection). Partition and boot details: D4.

## 1. Goal

A Windows user can try AetherOS from a USB drive, install it beside Windows, choose which system to boot at startup, and remove AetherOS later without damaging Windows.

## 2. Feature List

| ID | Feature | Priority | Description |
| --- | --- | --- | --- |
| INS-01 | Live USB | MVP | Boot from USB and try the desktop without installing. |
| INS-02 | Hardware report | MVP | Lists CPU, GPU, Wi-Fi, audio, and whether each is supported (links to the hardware list in 04). |
| INS-03 | Install beside Windows | MVP | Uses free space or shrinks the Windows partition. |
| INS-04 | Shrink Windows partition | MVP | Checks free space, runs the NTFS check, and shows the new size before changing anything. |
| INS-05 | Erase disk install | MVP | Full-disk install with a confirmation that names the disk, size, and model. |
| INS-06 | Boot menu | MVP | Shows AetherOS and Windows Boot Manager. Default and timeout are configurable. |
| INS-07 | Secure Boot | Later (public release) | Works with Secure Boot on using a signed boot path (D4 section 5). Not needed for Personal Edition. |
| INS-08 | Disk encryption | v1 | LUKS2 option during install. Recovery key shown once. |
| INS-09 | Uninstall | MVP | Removes AetherOS partitions and boot entry. Windows boot entry is restored. |
| INS-10 | Repair and rescue | v1 | Boots a repair environment to fix the boot menu or reinstall the bootloader. |
| INS-11 | Pre-checks | MVP | Warns about BitLocker, Fast Startup, low battery, missing backup, and pending Windows updates. |
| INS-12 | Dry run | MVP | Shows the full partition plan without writing to disk. |
| INS-13 | Legacy BIOS install | MVP | Installs on MBR disks with GRUB, for PCs without UEFI (ADR-016). Windows BIOS installs are detected and added to the boot menu. |

## 3. Install Flow (User View)

1. Boot the USB drive and choose "Try or Install".
2. Read the hardware report (INS-02).
3. Run the pre-checks (INS-11). Fix or accept each warning.
4. Choose "Install beside Windows" or "Erase disk".
5. Review the partition plan (INS-12). This is the dry run.
6. Confirm the disk name and size.
7. Installation writes the system image and the boot entries.
8. Restart. The boot menu shows AetherOS and Windows.
9. First-run setup: language, time zone, account, network, optional encryption.

## 4. Windows Pre-Checks (Required Before Resizing)

| Check | Why | What the installer does |
| --- | --- | --- |
| Backup done | Partition resizing can fail | Asks the user to confirm a backup. Cannot be skipped in "Install beside Windows". |
| BitLocker status | Resizing may require the BitLocker recovery key | Detects BitLocker. Asks the user to save the recovery key first. |
| Fast Startup | Windows keeps the NTFS volume in a hidden state. Mounting or resizing from another OS can corrupt data. | Asks the user to turn off Fast Startup in Windows. Cannot continue until confirmed. |
| NTFS health | Shrinking needs a clean volume | Runs a read-only check. Stops if errors are found and asks the user to run `chkdsk` in Windows. |
| Free space | Installation needs space | Shows the space available and the minimum required (64 GB recommended). |
| Pending Windows updates | Updates can change the boot order | Warns. Recommends finishing updates before installing. |
| Battery | Power loss during a resize is dangerous | Requires AC power on laptops. |

## 5. Boot Chain (Summary)

- systemd-boot is installed to the EFI System Partition. The Windows Boot Manager on the same partition is detected automatically.
- AetherOS boots through a signed unified kernel image (D4).
- Windows keeps its own boot files. The installer does not change them.

## 6. Uninstall Flow

1. Boot Windows.
2. Use Windows' "Uninstall AetherOS" tool, or boot the USB and choose "Uninstall".
3. The tool removes the AetherOS partitions and boot entry.
4. Free space is added back to the Windows partition, or to an unallocated area (the user chooses).

## 7. Dependencies

- P8 phase.
- P2 hardware support (installer must boot on reference machines).
- D4 partition layout and boot chain.

## 8. Acceptance Criteria

| ID | Criterion | Result |
| --- | --- | --- |
| INS-AC-01 | Live USB boots on 5 reference machines | Not tested |
| INS-AC-02 | Pre-checks block resizing when Fast Startup is on, and pass when it is off | Not tested |
| INS-AC-03 | Dry run output matches the final partition layout | Not tested |
| INS-AC-04 | Install beside Windows 11 on 3 machines; both systems boot | Not tested |
| INS-AC-05 | Uninstall restores Windows boot and removes AetherOS partitions | Not tested |
| INS-AC-06 | 10 of 10 install and uninstall cycles succeed on 3 machines (G5) | Not tested |
| INS-AC-07 | Windows data is unchanged after install and uninstall (file hash check) | Not tested |
| INS-AC-08 | Secure Boot on: AetherOS boots with the signed path | Not tested (Later, public release) |
| INS-AC-09 | Legacy BIOS install on an MBR machine; both systems boot | Not tested |

## 9. Caveats to Show Users

- Always back up before resizing a partition.
- Keep the BitLocker recovery key somewhere safe.
- Turn off Fast Startup in Windows before installing.
- A Windows feature update may reset the boot order. Use the boot menu key (shown at startup) to pick AetherOS, or run the repair tool (INS-10).

## 10. Open Questions

- Minimum free space for installation. Proposed: 32 GB for the base system, 64 GB recommended.
- Whether to support installing AetherOS on a disk that already has another Linux install. Proposed: out of scope for v1.
