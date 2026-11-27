# `test.sh` — Wi-Fi credentials as arguments (P2-T03)

- Date: 2026-11-27
- Session: S1 (`session - 1 (Nov27)`)
- File: `os-build/board/aetheros/overlay/root/test.sh`
- Reason: the connect test needed the owner's SSID and passphrase, and asking
  for them inside the script kept the credentials out of the runbook.

## Usage

```sh
sh /root/test.sh                                # prompts for SSID, then passphrase (hidden)
sh /root/test.sh "MyNetwork" "my passphrase"    # positional
sh /root/test.sh --wifi-ssid MyNetwork --wifi-password 'my passphrase'
sh /root/test.sh --no-wifi                      # scan only
WIFI_SSID=MyNetwork WIFI_PSK='…' sh /root/test.sh
sh /root/test.sh -- "MyNetwork" "-passphrase-starting-with-a-dash"
```

`--` ends option parsing, so a passphrase may start with `-`.
`WIFI_PASSWORD` is accepted as an alias of `WIFI_PSK`.

## What the connect step does

1. Finds the wireless interface from `/sys/class/net/*/wireless` (fallback:
   `wlan*`, `wlp*`, `wlx*`), brings it up, logs `iw dev` and a scan.
2. Writes `/run/aether-wpa.conf` with `umask 077` (root-only tmpfs), escaping
   `"` and `\`, and starts `wpa_supplicant -B -D nl80211` (falls back to `wext`).
3. Waits up to 30 s for `iw dev <iface> link` to report `Connected to`.
4. Takes a lease with the image's `udhcpc` (its `default.script` writes
   `/etc/resolv.conf`), then records address and default gateway.
5. Pings the gateway, then the internet / DNS, and marks the result.

Markers written to `/var/log/aether-test.log`:

```
WIFI_ASSOC: Connected to … SSID: … signal: …
WIFI_IP: 192.168.68.x/24
WIFI_GW: 192.168.68.1
WIFI_PING: gateway ok | gateway failed, internet ok | no reply
WIFI_CONNECT_OK: wlan0 ssid="…"        (or WIFI_CONNECT_FAIL: <reason>)
WIFI_CONNECT_SKIPPED: <reason>
WIFI_AFTER_SUSPEND: still connected | link down
wifi connect: ok|fail|skipped          (SUMMARY)
```

Failure reasons are explicit and distinguishable: `no wireless interface
found`, `wpa_supplicant is not installed`, `no association after 30 s`,
`associated but no usable connection`.

## Credential handling

- The passphrase is **never** printed, never part of a logged command line, and
  never written outside `/run` (tmpfs).
- The config file is `chmod 600`, created under `umask 077`, and removed on
  exit by a `trap … EXIT HUP INT TERM`.
- The log records `wifi: ssid="…" passphrase=<redacted>`.

## Verification (run for this change)

| Host | Command | Result |
| --- | --- | --- |
| macOS (stub `iw`/`ip`/`udhcpc`, fake sysfs) | positional, `--` with a dash-leading passphrase, `--no-wifi`, SSID-only, bad option, `--help` | correct marker in every case; `grep -c 'S3cret' log` → **0** |
| Dell Ubuntu, `dash` | `--wifi-ssid … --wifi-password …` | `WIFI_CONNECT_OK`, `wifi connect: ok`, 0 leaks |
| Dell Ubuntu, **the image's own busybox 1.37** (`/bin/busybox` extracted from the squashfs) | same | `WIFI_ASSOC`, `WIFI_IP 192.168.68.151/24`, `WIFI_GW`, `WIFI_PING: gateway ok`, `WIFI_CONNECT_OK`, `WIFI_AFTER_SUSPEND: still connected` |
| Syntax | `sh -n` | OK |

Note: Ubuntu's own `/usr/bin/busybox` is built with a standalone shell, so it
runs applets instead of the stubs and cannot be used for this test; the image's
busybox respects `PATH` and was used instead.

## Deployment

The flashed USB still carries the previous image, so the new script was copied
to the FAT ESP as `/test.sh` as well:

```sh
sudo mount /dev/sda1 /mnt/esp && sudo cp test.sh /mnt/esp/test.sh
# on AetherOS: mount /dev/sda1 /mnt && sh /mnt/test.sh "SSID" "passphrase"
```

The `/root/test.sh` copy inside the squashfs is replaced by the next image
build (the same build that will carry the UI binaries).

## Follow-ups

- Consider `wpa_passphrase` in the defconfig so the PSK can be stored hashed
  instead of plaintext in the tmpfs file.
- `rfkill` is not in the image; a soft-blocked radio would currently show up as
  "no association after 30 s". Adding `BR2_PACKAGE_RFKILL` would let the script
  report the block instead.
