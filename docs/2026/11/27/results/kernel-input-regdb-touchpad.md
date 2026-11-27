# Kernel input / regulatory fixes — regdb firmware and the I2C-HID touchpad

- Date: 2026-11-27 (device evidence collected 2026-11-28)
- Session: kernel/input session (subagent worktree, shared with S1 and 5 others)
- Device: Dell Inspiron 5402, i5-1135G7 (Tiger Lake-U), running the P2 run-3
  AetherOS image over USB — `Linux buildroot 6.18.0 #1 SMP PREEMPT_DYNAMIC Thu
  Nov 26 19:57:10 UTC 2026 x86_64`
- Input: `docs/2026/11/27/results/p2-hardware-validation-run3.md` (problems 4 and
  the P2-T05 touchpad item)
- Files changed: `os-build/kernel/aether.fragment`,
  `os-build/scripts/kernel-config-check.sh`
- Status: **fixed in the kernel fragment and verified by a real `olddefconfig`
  merge against linux-6.18; on-hardware confirmation belongs to the next image
  build (S1)**. The device was inspected read-only and was **not** rebooted.

---

## 1. Defect — `regulatory.db` never loads, regdomain stuck at `country 00`

### 1.1 Device evidence

```
$ dmesg | grep -iE "regulatory|cfg80211"
[    1.002154] cfg80211: Loading compiled-in X.509 certificates for regulatory database
[    1.008019] faux_driver regulatory: Direct firmware load for regulatory.db failed with error -2
[    1.009773] cfg80211: failed to load regulatory.db

$ ls -l /lib/firmware/regulatory.db*
-rw-r--r--  1 root root 6328 Nov 26 19:31 /lib/firmware/regulatory.db
-rw-r--r--  1 root root 1085 Nov 26 19:31 /lib/firmware/regulatory.db.p7s

$ iw reg get
global
country 00: DFS-UNSET
        (2402 - 2472 @ 40), (6, 20), (N/A)
        (2474 - 2494 @ 20), (6, 20), (N/A), NO-OFDM, PASSIVE-SCAN
        (5170 - 5250 @ 80), (6, 20), (N/A), AUTO-BW, PASSIVE-SCAN
        (5250 - 5330 @ 80), (6, 20), (0 ms), DFS, AUTO-BW, PASSIVE-SCAN
```

The files are in the image and correctly signed; the load happens at `t=1.0 s`,
i.e. **during initcalls, before the squashfs root is mounted**. `error -2` is
`ENOENT` from the kernel firmware loader, and because the image does not enable
`CONFIG_FW_LOADER_USER_HELPER` no userspace retry ever happens — the boot-time
miss is permanent.

### 1.2 Kernel evidence (linux-6.18 sources)

| Where | What it proves |
| --- | --- |
| `net/wireless/reg.c:4312` — `#ifndef MODULE` / `late_initcall(regulatory_init_db);` | Built-in cfg80211 issues the firmware request from a **late_initcall**, i.e. inside `do_basic_setup()`, before `prepare_namespace()` mounts the root filesystem. `/lib/firmware` does not exist yet → `-ENOENT`. |
| `net/wireless/reg.c:4265` `regulatory_init_db()` → `regulatory_hint_core()` → `query_regdb_file()` → `request_firmware_nowait(..., "regulatory.db", ...)` (reg.c:1063) | The only load site; nothing re-requests it later. |
| `net/wireless/reg.c:4315` `regulatory_init()` — `#ifdef MODULE return regulatory_init_db();` | As a **module**, the identical load runs at module load, after systemd-udev is up and the squashfs root (`/lib/firmware`) is readable. |
| `net/wireless/reg.c:1048` `MODULE_FIRMWARE("regulatory.db")` | Lets depmod/udev know the module needs that file, so coldplug orders it correctly. |
| `net/wireless/Kconfig:18` `config CFG80211` — `tristate` | Both configurations are supported by Kconfig. |
| `drivers/net/wireless/intel/iwlwifi/Kconfig` — `config IWLWIFI` `tristate ... depends on PCI && HAS_IOMEM && CFG80211` | iwlwifi is tristate and bounded by cfg80211, so it must follow cfg80211 to `=m` (Kconfig would otherwise cap it). |

### 1.3 Options considered and rejected

- **`CONFIG_CFG80211_INTERNAL_REGDB`** — removed from the kernel (deprecated in
  4.15, deleted in 5.0); the symbol does not exist in 6.18, so it cannot be used.
- **Ship `regulatory.db` in an initramfs** — AetherOS deliberately has no
  initramfs (`aether.fragment`, USB-storage block: "there is no initramfs to load
  modules before the root filesystem is mounted"; the cmdline relies on
  `rootwait`). Introducing an initramfs to deliver one 6 KB file is a much larger
  change to the P1-T05/P1-T07 boot chain than moving two drivers to modules.
- **`cfg80211.ieee80211_regdom=XX` module parameter** — only sets an alpha2 code;
  it carries no per-country channel/power data, so `iw reg get` would still show
  no real country rules. It is a hint, not a regulatory database.
- **`CONFIG_FW_LOADER_USER_HELPER` + a fallback helper** — still requires a
  mounted rootfs plus userspace running before the request; it replaces a
  deterministic module load with a race.

### 1.4 Change (chosen fix)

`os-build/kernel/aether.fragment`, wireless block:

```
-CONFIG_CFG80211=y
-CONFIG_MAC80211=y
+CONFIG_CFG80211=m
+CONFIG_MAC80211=m
 CONFIG_RFKILL=y
-CONFIG_IWLWIFI=y
+CONFIG_IWLWIFI=m
```

`CONFIG_ATH10K=m` / `CONFIG_ATH10K_PCI=m` were already modules and stay so.
Module loading and firmware-from-rootfs on this image are already proven by
`i915` (`DRM_I915=m`, DMC firmware v2.12 loaded, `card0-eDP-1` connected) and
`ath10k` (QCA6174 `WLAN.RM.4.4.1-00309`), so this introduces no new mechanism.

---

## 2. Defect — touchpad stays `PS/2 Generic Mouse` instead of I2C-HID

### 2.1 Device evidence

The device enumerates correctly as an I2C client, but no driver is bound:

```
$ ls /sys/bus/i2c/devices/
2-0050  2-0052  i2c-0 ... i2c-13  i2c-DLL09D9:00

$ ls /sys/bus/i2c/drivers/i2c_hid_acpi/
bind  uevent  unbind                       # <- no client symlink
$ ls -l /sys/bus/i2c/devices/i2c-DLL09D9:00/driver
ls: /sys/bus/i2c/devices/i2c-DLL09D9:00/driver: No such file or directory

$ dmesg | grep -iE "i2c|hid"
[    0.965248] i2c_hid_acpi i2c-DLL09D9:00: HID over i2c has not been provided an Int IRQ
[    0.967624] i2c_hid_acpi i2c-DLL09D9:00: probe with driver i2c_hid_acpi failed with error -22

$ cat /sys/bus/i2c/devices/i2c-DLL09D9:00/firmware_node/path ; cat .../uevent
\_SB_.PC00.I2C1.TPD0
MODALIAS=acpi:DLL09D9:PNP0C50:

$ readlink -f /sys/bus/i2c/devices/i2c-1/device
/sys/devices/pci0000:00/0000:00:15.1/i2c_designware.1      # Tiger Lake I2C #1
```

So the touchpad is the expected `DLL09D9` / `PNP0C50` HID-over-I2C device on
`00:15.1` — the bus, the ACPI companion and `i2c_designware` are all fine. What
is missing is the **interrupt**, and the reason is that the system has no GPIO
controller at all:

```
$ ls /sys/class/gpio/            -> No such file or directory
$ ls /sys/bus/gpio/devices/      -> No such file or directory
$ ls /sys/bus/platform/drivers/ | grep -iE "pinctrl|gpio"   -> gpio-clk   (only)
$ grep -ic gpio /proc/interrupts -> 0

$ cat /sys/bus/acpi/devices/INT34C5:00/path ; cat .../status
\_SB_.GPI0
15                                  # present + enabled + functioning
```

`INT34C5` is the Tiger Lake PCH-LP GPIO block, it is present in the ACPI
namespace, and **no driver claims it**.

### 2.2 Kernel evidence

| Where | What it proves |
| --- | --- |
| `drivers/pinctrl/intel/pinctrl-tigerlake.c:739-745` — `acpi_device_id tgl_pinctrl_acpi_match[] = { { "INT34C5", &tgllp_soc_data } }` | `pinctrl-tigerlake` is exactly the driver for the `INT34C5` block that provides the touchpad's `GpioInt`. |
| `drivers/pinctrl/intel/Kconfig:157` `config PINCTRL_TIGERLAKE` `select PINCTRL_INTEL`; `:32` `config PINCTRL_INTEL` `select GPIOLIB`, `select GPIOLIB_IRQCHIP`; `drivers/gpio/Kconfig:37` `config GPIO_ACPI` `def_bool y depends on ACPI` | One symbol enables the whole pin/GPIO stack; `GPIO_ACPI` follows automatically. |
| `drivers/pinctrl/Kconfig:6` `menuconfig PINCTRL` (a **bool**) | The x86_64 defconfig leaves `PINCTRL` off (`# CONFIG_PINCTRL is not set`), so *no* pin controller driver is even visible to Kconfig. This is the actual root cause: the fragment asked for `I2C_HID=y` but never enabled `PINCTRL`. |
| `drivers/i2c/i2c-core-base.c:519-531` — `irq = i2c_acpi_get_irq(...)`, `if (irq == -EPROBE_DEFER) { dev_err_probe(...); goto put_sync_adapter; }`, `if (irq < 0) irq = 0; client->irq = irq;` | The IRQ is resolved at **probe** time; any non-defer error is flattened to `client->irq = 0`. |
| `drivers/hid/i2c-hid/i2c-hid-core.c:1231-1235` — `if (!client->irq) { dev_err(..., "HID over i2c has not been provided an Int IRQ"); return -EINVAL; }` | Exactly the two dmesg lines observed (`-EINVAL` = `-22`). |
| `drivers/gpio/gpiolib-acpi-core.c` `acpi_get_gpiod()` returns `-EPROBE_DEFER` when no registered `gpio_device` matches the GpioInt's resource source | With the pinctrl driver present, the same lookup resolves `\_SB.GPI0` and yields a real IRQ. |

With no pin controller the ACPI `GpioInt` cannot be turned into an IRQ, i2c-hid
declines the device, and the firmware keeps the touchpad in PS/2 emulation —
which is why `libinput`/evdev only ever saw `PS/2 Generic Mouse` (`Bus=0011`,
`/proc/bus/input/devices`).

### 2.3 Change

`os-build/kernel/aether.fragment`, input block:

```
+CONFIG_PINCTRL=y                 # menuconfig the x86_64 defconfig leaves off
+CONFIG_PINCTRL_TIGERLAKE=y       # INT34C5 -> selects PINCTRL_INTEL/GPIOLIB/IRQCHIP
-CONFIG_I2C_HID=y
-CONFIG_I2C_HID_ACPI=y
+CONFIG_I2C_HID=m
+CONFIG_I2C_HID_ACPI=m            # probe after the built-in pinctrl/GPIO is up
```

`CONFIG_HID_MULTITOUCH=y` and `CONFIG_I2C_DESIGNWARE_PLATFORM=y` were already
present and are unchanged. `i2c-hid-acpi` binds `04F3:3147` and `hid-multitouch`
turns it into a touchpad; making the pair modules matches how Ubuntu delivers
them and guarantees the probe happens after the GPIO controllers are registered
(no dependence on link order inside `device_initcall`).

---

## 3. Verification performed (macOS, no device reboot)

Real Kconfig merge of `x86_64_defconfig` + the updated fragment against
linux-6.18 (cached tarball), then `olddefconfig`:

```
$ ARCH=x86_64 gmake defconfig LD=<gnu-ld-wrapper>     # host gmake 4.4.1
$ cat os-build/kernel/aether.fragment >> .config       # merge_config.sh needs GNU sed
$ ARCH=x86_64 gmake olddefconfig LD=<gnu-ld-wrapper>
```

Resulting `.config` (excerpt):

```
CONFIG_CFG80211=m          CONFIG_MAC80211=m         CONFIG_IWLWIFI=m
CONFIG_ATH10K=m            CONFIG_ATH10K_PCI=m
CONFIG_PINCTRL=y           CONFIG_PINCTRL_INTEL=y    CONFIG_PINCTRL_TIGERLAKE=y
CONFIG_GPIOLIB=y           CONFIG_GPIOLIB_IRQCHIP=y  CONFIG_GPIO_ACPI=y
CONFIG_I2C_HID=m           CONFIG_I2C_HID_ACPI=m     CONFIG_I2C_HID_CORE=m
CONFIG_HID_MULTITOUCH=y    CONFIG_I2C_DESIGNWARE_PLATFORM=y
```

- All **17 P1-required options** are still present (`PSI CGROUPS CGROUP_FREEZER
  MEMCG ZRAM SQUASHFS EXT4_FS BTRFS_FS EFI EFI_STUB DRM DRM_I915 DRM_AMDGPU
  NO_HZ_IDLE SUSPEND VIRTIO ANDROID_BINDER_IPC`) plus the binder device string.
- No `CONFIG_CFG80211=y` regression (asserted by the check script).
- `bash -n os-build/scripts/kernel-config-check.sh` → syntax OK.
- Note: `CONFIG_PINCTRL_TIGERLAKE=y` alone is silently dropped by `olddefconfig`
  when `PINCTRL` is off; the first merge attempt proved this, which is why
  `CONFIG_PINCTRL=y` is now explicit.

The authoritative end-to-end check remains the CI image build (S1): look for
`i2c-hid-acpi`/`cfg80211` modules in the rootfs and the boot-time acceptance
items below.

---

## 4. What a good boot looks like (acceptance)

Regulatory domain:

```
$ iw reg get
global
country IN: DFS-ETSI            # <- any real country, no longer "00"
        (2402 - 2482 @ 40), (N/A, 20), (N/A), PASSIVE-SCAN
        (5170 - 5250 @ 80), (N/A, 20), (N/A), AUTO-BW, PASSIVE-SCAN ...

$ dmesg | grep -i regulatory
cfg80211: Loading compiled-in X.509 certificates for regulatory database
# and NO "Direct firmware load for regulatory.db failed" / "failed to load regulatory.db"

$ lsmod | grep -E "cfg80211|mac80211"     # both loaded as modules
```

Touchpad:

```
$ libinput list-devices | grep -A2 -i touchpad
Device:           DLL09D9:00 04F3:3147 Touchpad
Kernel:           /dev/input/event4

$ grep -B2 -A4 "04f3" /proc/bus/input/devices
N: Name="DLL09D9:00 04F3:3147 Touchpad"     # Bus=0018 (I2C), not Bus=0011
# "PS/2 Generic Mouse" must be gone

$ ls -l /sys/bus/i2c/drivers/i2c_hid_acpi/ | grep DLL09D9   # bound symlink
$ cat /sys/class/gpio/gpiochip*/label | head                # INT34C5 gpiochips exist
```

If the touchpad still fails, capture `dmesg | grep -iE "DLL09D9|i2c_hid|pinctrl|
gpio"` and `lsmod`: a missing `gpiochip` means `PINCTRL_TIGERLAKE` did not reach
the image; a repeated `-EINVAL` with gpiochips present points at the ACPI
`GpioInt` description and would need a device-specific quirk instead.

---

## 5. Requests for the image session (S1) — `aetheros_defconfig`

`usb.ids` is missing (`lsusb`: "unable to initialize usb spec"). The defconfig is
owned by the ui-image session, so the exact lines to add are:

```
BR2_PACKAGE_HWDATA=y
BR2_PACKAGE_HWDATA_USB_IDS=y
```

- `package/hwdata/Config.in` (Buildroot 2025.11): `BR2_PACKAGE_HWDATA_USB_IDS`
  installs `usb.ids`; `BR2_PACKAGE_HWDATA_PCI_IDS` defaults to `y` and installs
  `pci.ids` (leave as is, or list it explicitly for clarity).
- `package/hwdata/hwdata.mk` installs the files into **`/usr/share/hwdata/`**,
  which is where `usbutils` looks for `usb.ids`, so `lsusb` will print real
  vendor/product names (`8087:0026` etc.) after the next build.

Also for S1: this fragment makes `cfg80211`, `mac80211`, `iwlwifi`, `i2c-hid` and
`i2c-hid-acpi` modules, so the image must keep installing kernel modules (it
already does for `i915`/`ath10k`). Regression signal to watch after the build:
`lsmod | grep -c cfg80211` must be 1 and Wi-Fi scanning must still work.

---

## 6. Recorded, not fixed — branding gap

`hostname` = `buildroot`, the Bluetooth adapter name = `buildroot`, and
`/etc/os-release` says `Buildroot 2025.11`. This is owner territory
(`docs/2026/11/17/branding/`) and was left untouched.

---

## 7. Related script change

`os-build/scripts/kernel-config-check.sh` (local stand-in for
`kernel-config.yml`, S1 owns the workflow) now:

- pins `ARCH=x86_64` and starts from `defconfig` (the old first
  `make olddefconfig` failed — there is no `.config` yet);
- prefers `gmake` when the host `make` is older than 4.0 (macOS ships 3.81);
- falls back to appending the fragment when `merge_config.sh` cannot run (it
  needs GNU `sed`);
- accepts `DRM_I915=y|m` (matching the workflow) and adds the P2 assertions
  `CFG80211=m MAC80211=m IWLWIFI=m ATH10K=m ATH10K_PCI=m PINCTRL=y
  PINCTRL_TIGERLAKE=y GPIOLIB=y GPIO_ACPI=y GPIOLIB_IRQCHIP=y I2C_HID=m
  I2C_HID_ACPI=m I2C_HID_CORE=m HID_MULTITOUCH=y I2C_DESIGNWARE_PLATFORM=y`
  plus a "cfg80211 must not go back to `=y`" check.
- It still needs a GNU-compatible `ld` for Kbuild's linker version probe; on this
  macOS host the merge was run with a `--version` stub in `$TMPDIR` (not
  committed). The same script runs unmodified on the CI runner.
