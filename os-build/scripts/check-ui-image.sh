#!/bin/sh
# check-ui-image.sh — static assertions for the AetherOS graphical image.
#
# Runs on macOS (POSIX sh, no Buildroot, no Linux tools, no GNU-only flags):
# it checks that the image *would* carry and start what the UI needs, without
# building anything. Buildroot itself cannot run on the macOS dev host, so
# "survives olddefconfig" is approximated with an explicit dependency table
# that mirrors the 2025.11 Kconfig rules for the symbols this image sets.
#
# Usage: os-build/scripts/check-ui-image.sh [-v|--verbose]
#
# Exit status: 0 = every assertion passed, 1 = at least one failed.
# NOTES are printed for known-inactive symbols; they do not fail the check.
#
# What is asserted:
#   1. defconfig: required symbols, uncommented, with their Kconfig dependencies
#      satisfied by other symbols in the same file (so `make defconfig` keeps
#      them), plus the untouched BR2_PACKAGE_LUA_5_4=y line.
#   2. rootfs overlay: session wrapper, both units, tty1 masked, tty2 getty
#      enabled, aether-session reachable from multi-user.target.
#   3. post-build: script present, syntax-clean, and installing exactly the
#      core binaries the units start.
#   4. binaries.txt: every listed package exists in the workspace, and every
#      core binary is listed.
#   5. silent dependency drops: (a) a child symbol (for example
#      BR2_PACKAGE_LUA_5_4) without its package (BR2_PACKAGE_LUA) fails even on
#      macOS, and (b) with `--config <generated .config>` every `=y` symbol in
#      the defconfig must appear as `=y` in the generated config, which is the
#      real olddefconfig result. Run it inside Buildroot after `make defconfig`
#      (or let post-build.sh do it) to make the guard authoritative.

set -u

VERBOSE=0
CONFIG_FILE=""
while [ $# -gt 0 ]; do
    case "$1" in
        -v | --verbose) VERBOSE=1 ;;
        --config)
            shift
            CONFIG_FILE="${1:-}"
            if [ -z "$CONFIG_FILE" ]; then
                printf 'check-ui-image: --config needs a path to the generated .config\n' >&2
                exit 2
            fi
            ;;
        --config=*) CONFIG_FILE="${1#--config=}" ;;
        -h | --help)
            sed -n '2,34p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            printf 'check-ui-image: unknown argument: %s\n' "$1" >&2
            exit 2
            ;;
    esac
    shift
done

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
DEFCONFIG="$REPO_ROOT/os-build/buildroot/aetheros_defconfig"
OVERLAY="$REPO_ROOT/os-build/board/aetheros/overlay"
POST_BUILD="$REPO_ROOT/os-build/post-build.sh"
BINARY_LIST="$REPO_ROOT/os-build/buildroot/ui/binaries.txt"
UNITS="$OVERLAY/etc/systemd/system"

PASS=0
FAIL=0
NOTE=0

pass() {
    PASS=$((PASS + 1))
    if [ "$VERBOSE" = "1" ]; then
        printf '  ok    %s\n' "$*"
    fi
}

fail() {
    FAIL=$((FAIL + 1))
    printf '  FAIL  %s\n' "$*"
}

note() {
    NOTE=$((NOTE + 1))
    printf '  note  %s\n' "$*"
}

section() { printf '%s\n' "$*"; }

# defconfig_symbol VALUE_NAME — prints the symbol line if it is set to VALUE.
defconfig_has() {
    grep -qxF -- "$1" "$DEFCONFIG" 2>/dev/null
}

defconfig_enabled() {
    defconfig_has "$1=y"
}

# ------------------------------------------------------------------ defconfig ---

section "defconfig ($DEFCONFIG)"

if [ ! -f "$DEFCONFIG" ]; then
    fail "defconfig missing: $DEFCONFIG"
    printf '\ncheck-ui-image: 0 passed, 1 failed (cannot continue without the defconfig)\n'
    exit 1
fi

# udev comes from systemd here: BR2_PACKAGE_EUDEV/BR2_PACKAGE_UDEV are not
# symbols that survive a systemd build (2026-11-28 defconfig drops), so the
# contract asserts the udev *provider* instead.
REQUIRED_SYMBOLS='
BR2_TOOLCHAIN_BUILDROOT_CXX=y
BR2_PACKAGE_MESA3D=y
BR2_PACKAGE_MESA3D_OPENGL_EGL=y
BR2_PACKAGE_MESA3D_OPENGL_ES=y
BR2_PACKAGE_MESA3D_GBM=y
BR2_PACKAGE_SEATD=y
BR2_PACKAGE_SEATD_BUILTIN=y
BR2_PACKAGE_FONTCONFIG=y
BR2_PACKAGE_FREETYPE=y
BR2_PACKAGE_DEJAVU=y
BR2_PACKAGE_LIBXKBCOMMON=y
BR2_PACKAGE_XKEYBOARD_CONFIG=y
BR2_PACKAGE_LIBINPUT=y
BR2_PACKAGE_SYSTEMD=y
BR2_PACKAGE_LIBDRM=y
BR2_PACKAGE_WAYLAND=y
BR2_PACKAGE_LUA=y
BR2_PACKAGE_LUA_5_4=y
BR2_PACKAGE_WIREPLUMBER=y
BR2_PACKAGE_HWDATA=y
BR2_PACKAGE_HWDATA_USB_IDS=y
BR2_PACKAGE_EFIBOOTMGR=y
BR2_TARGET_GENERIC_GETTY_PORT="tty2"
BR2_ROOTFS_POST_BUILD_SCRIPT="../../post-build.sh"
'

for line in $REQUIRED_SYMBOLS; do
    if defconfig_has "$line"; then
        pass "defconfig has $line"
    else
        fail "defconfig missing (or commented): $line"
    fi
done

# The owner's WirePlumber fix: without Lua >= 5.3 the WirePlumber entry is
# dropped silently. It has to survive every edit of this file, and the Lua
# interpreter itself (BR2_PACKAGE_LUA) is what makes the version choice exist.
if defconfig_enabled BR2_PACKAGE_WIREPLUMBER && defconfig_enabled BR2_PACKAGE_LUA \
    && defconfig_enabled BR2_PACKAGE_LUA_5_4; then
    pass "WirePlumber + Lua interpreter + Lua 5.4 enabled"
else
    fail "WirePlumber needs BR2_PACKAGE_LUA=y AND BR2_PACKAGE_LUA_5_4=y (image build 46041689625 built WirePlumber: NO)"
fi

# --- silent dependency drops: a child symbol needs its package menu -------------
# Buildroot puts hundreds of sub-options inside `if BR2_PACKAGE_X`, so setting
# the child without the package is a no-op that olddefconfig deletes without a
# word. Every family used by this image is listed here; adding a child symbol to
# the defconfig means adding its row.
CHILD_REQUIREMENTS='
BR2_PACKAGE_LUA_5_4|BR2_PACKAGE_LUA
BR2_PACKAGE_SEATD_BUILTIN|BR2_PACKAGE_SEATD
BR2_PACKAGE_SEATD_DAEMON|BR2_PACKAGE_SEATD
BR2_PACKAGE_DEJAVU_MONO|BR2_PACKAGE_DEJAVU
BR2_PACKAGE_DEJAVU_SANS|BR2_PACKAGE_DEJAVU
BR2_PACKAGE_DEJAVU_SERIF|BR2_PACKAGE_DEJAVU
BR2_PACKAGE_DEJAVU_SANS_CONDENSED|BR2_PACKAGE_DEJAVU
BR2_PACKAGE_DEJAVU_SERIF_CONDENSED|BR2_PACKAGE_DEJAVU
BR2_PACKAGE_LIBXKBCOMMON_TOOLS|BR2_PACKAGE_LIBXKBCOMMON
BR2_PACKAGE_MESA3D_OPENGL_EGL|BR2_PACKAGE_MESA3D
BR2_PACKAGE_MESA3D_OPENGL_ES|BR2_PACKAGE_MESA3D
BR2_PACKAGE_MESA3D_GBM|BR2_PACKAGE_MESA3D
BR2_PACKAGE_MESA3D_OPENGL_GLX|BR2_PACKAGE_MESA3D
BR2_PACKAGE_MESA3D_GALLIUM_DRIVER_IRIS|BR2_PACKAGE_MESA3D
BR2_PACKAGE_MESA3D_GALLIUM_DRIVER_RADEONSI|BR2_PACKAGE_MESA3D
BR2_PACKAGE_MESA3D_GALLIUM_DRIVER_SOFTPIPE|BR2_PACKAGE_MESA3D
BR2_PACKAGE_MESA3D_VULKAN_DRIVER_INTEL|BR2_PACKAGE_MESA3D
BR2_PACKAGE_MESA3D_VULKAN_DRIVER_AMD|BR2_PACKAGE_MESA3D
BR2_PACKAGE_ALSA_UTILS_APLAY|BR2_PACKAGE_ALSA_UTILS
BR2_PACKAGE_ALSA_UTILS_AMIXER|BR2_PACKAGE_ALSA_UTILS
BR2_PACKAGE_ALSA_UTILS_BAT|BR2_PACKAGE_ALSA_UTILS
BR2_PACKAGE_ALSA_UTILS_SPEAKER_TEST|BR2_PACKAGE_ALSA_UTILS
BR2_PACKAGE_BLUEZ5_UTILS_CLIENT|BR2_PACKAGE_BLUEZ5_UTILS
BR2_PACKAGE_BLUEZ5_UTILS_TOOLS|BR2_PACKAGE_BLUEZ5_UTILS
BR2_PACKAGE_BLUEZ5_UTILS_DEPRECATED|BR2_PACKAGE_BLUEZ5_UTILS
BR2_PACKAGE_BLUEZ5_UTILS_PLUGINS_AUDIO|BR2_PACKAGE_BLUEZ5_UTILS
BR2_PACKAGE_WPA_SUPPLICANT_NL80211|BR2_PACKAGE_WPA_SUPPLICANT
BR2_PACKAGE_OPENSSH_SERVER|BR2_PACKAGE_OPENSSH
BR2_PACKAGE_OPENSSH_KEY_UTILS|BR2_PACKAGE_OPENSSH
BR2_PACKAGE_HWDATA_USB_IDS|BR2_PACKAGE_HWDATA
BR2_PACKAGE_HWDATA_PCI_IDS|BR2_PACKAGE_HWDATA
BR2_PACKAGE_HWDATA_PNP_IDS|BR2_PACKAGE_HWDATA
BR2_PACKAGE_LINUX_FIRMWARE_I915|BR2_PACKAGE_LINUX_FIRMWARE
BR2_PACKAGE_LINUX_FIRMWARE_QUALCOMM_6174|BR2_PACKAGE_LINUX_FIRMWARE
BR2_PACKAGE_LINUX_FIRMWARE_QUALCOMM_6174A_BT|BR2_PACKAGE_LINUX_FIRMWARE
BR2_PACKAGE_LINUX_FIRMWARE_AR3012_USB|BR2_PACKAGE_LINUX_FIRMWARE
'

while IFS='|' read -r child parent; do
    case "$child" in
        '' | '#'*) continue ;;
    esac
    if defconfig_enabled "$child" && ! defconfig_enabled "$parent"; then
        fail "$child is set but $parent is not: the child is dropped silently (same class as WirePlumber/Lua)"
    fi
done <<EOF
$CHILD_REQUIREMENTS
EOF
pass "checked $(printf '%s\n' "$CHILD_REQUIREMENTS" | grep -c '|') child/parent pairs for silent drops"

# --- olddefconfig survival: the dependency rules that matter for this image ---
# Each rule is "symbol requires at least one of these symbols", following the
# 2025.11 package Config.in files. A symbol whose dependency is missing is
# silently dropped by `make defconfig`.
require_any() {
    required="$1"
    shift
    if defconfig_enabled "$required"; then
        found=""
        for candidate in "$@"; do
            if defconfig_enabled "$candidate"; then
                found="$candidate"
                break
            fi
        done
        if [ -n "$found" ]; then
            pass "$required resolves ($found)"
        else
            fail "$required is set but none of its Kconfig dependencies is: $* (dropped by olddefconfig)"
        fi
    fi
}

# Mesa3d depends on BR2_INSTALL_LIBSTDCPP, selected only by the C++ symbol.
if defconfig_enabled BR2_PACKAGE_MESA3D && ! defconfig_enabled BR2_TOOLCHAIN_BUILDROOT_CXX; then
    fail "BR2_PACKAGE_MESA3D needs BR2_TOOLCHAIN_BUILDROOT_CXX (no libstdc++ -> Mesa dropped)"
else
    pass "Mesa C++ toolchain dependency satisfied"
fi
# EGL needs a gallium driver; GBM needs EGL + a gallium driver.
require_any BR2_PACKAGE_MESA3D_OPENGL_EGL \
    BR2_PACKAGE_MESA3D_GALLIUM_DRIVER_SOFTPIPE \
    BR2_PACKAGE_MESA3D_GALLIUM_DRIVER_IRIS \
    BR2_PACKAGE_MESA3D_GALLIUM_DRIVER_RADEONSI
require_any BR2_PACKAGE_MESA3D_GBM \
    BR2_PACKAGE_MESA3D_GALLIUM_DRIVER_SOFTPIPE \
    BR2_PACKAGE_MESA3D_GALLIUM_DRIVER_IRIS \
    BR2_PACKAGE_MESA3D_GALLIUM_DRIVER_RADEONSI
# seatd's sub-options only exist inside `if BR2_PACKAGE_SEATD`.
if defconfig_enabled BR2_PACKAGE_SEATD_BUILTIN && ! defconfig_enabled BR2_PACKAGE_SEATD; then
    fail "BR2_PACKAGE_SEATD_BUILTIN needs BR2_PACKAGE_SEATD (dropped: the builtin seat is inside the seatd menu)"
else
    pass "seatd builtin seat dependency satisfied"
fi
# xkbcommon compiles keymaps from the xkeyboard-config data at runtime.
if defconfig_enabled BR2_PACKAGE_LIBXKBCOMMON && ! defconfig_enabled BR2_PACKAGE_XKEYBOARD_CONFIG; then
    fail "libxkbcommon without BR2_PACKAGE_XKEYBOARD_CONFIG: no /usr/share/X11/xkb, the compositor cannot build a keymap"
else
    pass "xkbcommon data dependency satisfied"
fi
# The post-build script is resolved relative to the Buildroot top directory
# (os-build/buildroot/buildroot); ../../post-build.sh is os-build/post-build.sh.
if ! defconfig_has 'BR2_ROOTFS_POST_BUILD_SCRIPT="../../post-build.sh"'; then
    fail 'BR2_ROOTFS_POST_BUILD_SCRIPT is not "../../post-build.sh"'
elif [ -f "$POST_BUILD" ]; then
    pass "post-build script path resolves to $POST_BUILD"
else
    fail "BR2_ROOTFS_POST_BUILD_SCRIPT points at ../../post-build.sh but $POST_BUILD does not exist"
fi

# Known-inactive symbols: set, but dropped by 2025.11 because they need LLVM.
if defconfig_enabled BR2_PACKAGE_MESA3D_LLVM; then
    note "BR2_PACKAGE_MESA3D_LLVM is enabled: iris/radeonsi/ANV/RADV are active (large image, long build)"
else
    for symbol in BR2_PACKAGE_MESA3D_GALLIUM_DRIVER_IRIS \
        BR2_PACKAGE_MESA3D_GALLIUM_DRIVER_RADEONSI \
        BR2_PACKAGE_MESA3D_VULKAN_DRIVER_INTEL; do
        if defconfig_enabled "$symbol"; then
            note "$symbol is set but inactive (needs BR2_PACKAGE_MESA3D_LLVM); EGL ships via softpipe only"
        fi
    done
fi
if defconfig_enabled BR2_PACKAGE_MESA3D_VULKAN_DRIVER_AMD; then
    note "BR2_PACKAGE_MESA3D_VULKAN_DRIVER_AMD is not a Buildroot 2025.11 symbol (no RADV entry); ignored by Kconfig"
fi

# --------------------------------------------------- generated .config (Linux) ---
# Optional but authoritative: with `--config <output/.config>` every `=y` symbol
# in the defconfig must appear as `=y` in the config Buildroot actually resolved
# (`make defconfig` / `make olddefconfig`). This is the real silent-drop check;
# the child/parent table above only approximates Kconfig on macOS.

if [ -n "$CONFIG_FILE" ]; then
    section "generated .config ($CONFIG_FILE)"
    if [ ! -f "$CONFIG_FILE" ]; then
        fail "config file not found: $CONFIG_FILE (run make defconfig first)"
    else
        # Symbols that are set in the defconfig but are known to be inactive in
        # Buildroot 2025.11 for a documented reason. They are NOTES, not drops.
        DROP_ALLOWED='BR2_PACKAGE_MESA3D_GALLIUM_DRIVER_IRIS
BR2_PACKAGE_MESA3D_GALLIUM_DRIVER_RADEONSI
BR2_PACKAGE_MESA3D_VULKAN_DRIVER_INTEL
BR2_PACKAGE_MESA3D_VULKAN_DRIVER_AMD'

        DROPPED=0
        CHECKED=0
        while IFS= read -r line; do
            case "$line" in
                *"=y") ;;
                *) continue ;;
            esac
            symbol="${line%=y}"
            CHECKED=$((CHECKED + 1))
            if grep -qxF "$symbol=y" "$CONFIG_FILE"; then
                pass "resolved: $symbol"
                continue
            fi
            if printf '%s\n' "$DROP_ALLOWED" | grep -qxF "$symbol"; then
                note "$symbol is inactive by design (see the defconfig comment)"
            else
                fail "$symbol=y in the defconfig is MISSING from the generated .config (silent drop)"
                DROPPED=$((DROPPED + 1))
            fi
        done <"$DEFCONFIG"

        # String symbols the session depends on do not show up as `=y`.
        if grep -qxF 'BR2_TARGET_GENERIC_GETTY_PORT="tty2"' "$CONFIG_FILE"; then
            pass "resolved: BR2_TARGET_GENERIC_GETTY_PORT=\"tty2\""
        else
            fail "BR2_TARGET_GENERIC_GETTY_PORT=\"tty2\" is missing from the generated .config"
        fi

        # The package that started this guard: a config with LUA_5_4 but no Lua
        # interpreter builds WirePlumber: NO without a single warning.
        if grep -qxF 'BR2_PACKAGE_WIREPLUMBER=y' "$CONFIG_FILE" \
            && grep -qxF 'BR2_PACKAGE_LUA=y' "$CONFIG_FILE" \
            && grep -qxF 'BR2_PACKAGE_LUA_5_4=y' "$CONFIG_FILE"; then
            pass "resolved: WirePlumber with the Lua interpreter and Lua 5.4"
        else
            fail "WirePlumber/Lua did not resolve in the generated .config (build would log 'Build wireplumber : NO')"
        fi

        if [ "$DROPPED" -eq 0 ]; then
            pass "all $CHECKED =y defconfig symbols resolved"
        else
            printf '  FAIL  %s of %s =y defconfig symbols were dropped\n' "$DROPPED" "$CHECKED"
        fi
    fi
fi

# ------------------------------------------------------------- rootfs overlay ---

section "rootfs overlay ($OVERLAY)"

WRAPPER="$OVERLAY/usr/bin/aether-session"
if [ -f "$WRAPPER" ] && [ -x "$WRAPPER" ]; then
    pass "/usr/bin/aether-session present and executable"
else
    fail "/usr/bin/aether-session missing or not executable"
fi
if [ -f "$WRAPPER" ] && sh -n "$WRAPPER" 2>/dev/null; then
    pass "aether-session parses (sh -n)"
else
    fail "aether-session does not parse with sh -n"
fi
if [ -f "$WRAPPER" ] && grep -q 'LIBSEAT_BACKEND' "$WRAPPER" && grep -q 'AETHER_COMPOSITOR_BACKEND' "$WRAPPER"; then
    pass "aether-session exports the session environment"
else
    fail "aether-session does not export AETHER_COMPOSITOR_BACKEND/LIBSEAT_BACKEND"
fi
if [ -f "$WRAPPER" ] && grep -q 'exec /usr/bin/aether-compositor' "$WRAPPER"; then
    pass "aether-session execs /usr/bin/aether-compositor"
else
    fail "aether-session does not exec /usr/bin/aether-compositor"
fi

for unit in "$UNITS/aether-session.service" "$UNITS/aether-shell.service"; do
    if [ -f "$unit" ]; then
        pass "unit present: $(basename "$unit")"
    else
        fail "unit missing: $unit"
    fi
done

SESSION_UNIT="$UNITS/aether-session.service"
if [ -f "$SESSION_UNIT" ]; then
    for setting in 'ExecStart=/usr/bin/aether-session' 'TTYPath=/dev/tty1' \
        'StandardInput=tty' 'Restart=on-failure' 'ConditionPathExists=/dev/dri' \
        'Wants=aether-shell.service' 'LogsDirectory=aether' \
        'After=systemd-user-sessions.service systemd-udev-settle.service'; do
        if grep -q -- "$setting" "$SESSION_UNIT"; then
            pass "aether-session.service has $setting"
        else
            fail "aether-session.service lacks $setting"
        fi
    done
    # Ordered After the target that Wants it is the classic way to be accused of
    # an ordering cycle; the unit relies on wants + udev-settle instead.
    if grep -q '^After=.*multi-user\.target' "$SESSION_UNIT"; then
        fail "aether-session.service is ordered After=multi-user.target (use the systemd-user-sessions/udev-settle ordering)"
    else
        pass "aether-session.service is not ordered after the target that wants it"
    fi
    if grep -q '^WantedBy=multi-user.target' "$SESSION_UNIT"; then
        pass "aether-session.service is installable into multi-user.target"
    else
        fail "aether-session.service has no WantedBy=multi-user.target"
    fi
fi

SHELL_UNIT="$UNITS/aether-shell.service"
if [ -f "$SHELL_UNIT" ]; then
    for setting in 'ExecStart=/usr/bin/aether-shell-host' \
        'After=aether-session.service' 'PartOf=aether-session.service' \
        'XDG_RUNTIME_DIR=/run/aether' 'WAYLAND_DISPLAY=wayland-0' \
        'AETHER_COMPOSITOR_LOG=/var/log/aether/compositor.log'; do
        if grep -q -- "$setting" "$SHELL_UNIT"; then
            pass "aether-shell.service has $setting"
        else
            fail "aether-shell.service lacks $setting"
        fi
    done
fi

# The shell host wrapper: waits for the socket, then --connect.
SHELL_HOST="$OVERLAY/usr/bin/aether-shell-host"
if [ -f "$SHELL_HOST" ]; then
    pass "/usr/bin/aether-shell-host present"
    if sh -n "$SHELL_HOST" 2>/dev/null; then
        pass "aether-shell-host parses (sh -n)"
    else
        fail "aether-shell-host does not parse with sh -n"
    fi
    if grep -q -- '--connect' "$SHELL_HOST" && grep -q -- '--interactive' "$SHELL_HOST"; then
        pass "aether-shell-host prefers --connect and falls back to --interactive"
    else
        fail "aether-shell-host does not handle both --connect and --interactive"
    fi
else
    fail "/usr/bin/aether-shell-host missing"
fi

# The unit, the shell unit and the wrappers must agree on the three paths they
# share, otherwise the shell waits on the wrong socket or tails the wrong log.
SESSION_LOG=$(sed -n 's/^StandardOutput=append:\(.*\)$/\1/p' "$SESSION_UNIT" 2>/dev/null | head -1)
SHELL_LOG=$(sed -n 's/^Environment=AETHER_COMPOSITOR_LOG=//p' "$SHELL_UNIT" 2>/dev/null | head -1)
WRAPPER_LOG=$(sed -n 's/.*AETHER_COMPOSITOR_LOG:-\([^}]*\)}.*/\1/p' "$WRAPPER" 2>/dev/null | head -1)
if [ -n "$SESSION_LOG" ] && [ "$SESSION_LOG" = "$SHELL_LOG" ] && [ "$SESSION_LOG" = "$WRAPPER_LOG" ]; then
    pass "compositor log path agrees everywhere: $SESSION_LOG"
else
    fail "compositor log path disagrees (unit append: '${SESSION_LOG:-none}', shell env: '${SHELL_LOG:-none}', wrapper default: '${WRAPPER_LOG:-none}')"
fi

SESSION_RUNTIME=$(sed -n 's/^RuntimeDirectory=//p' "$SESSION_UNIT" 2>/dev/null | head -1)
SHELL_RUNTIME=$(sed -n 's/^Environment=XDG_RUNTIME_DIR=\/run\/\(.*\)$/\1/p' "$SHELL_UNIT" 2>/dev/null | head -1)
WRAPPER_RUNTIME=$(sed -n 's/.*XDG_RUNTIME_DIR:-\(\/run\/[^}]*\)}.*/\1/p' "$WRAPPER" 2>/dev/null | head -1)
if [ -n "$SESSION_RUNTIME" ] && [ "$SESSION_RUNTIME" = "$SHELL_RUNTIME" ] && [ "/run/$SESSION_RUNTIME" = "$WRAPPER_RUNTIME" ]; then
    pass "wayland runtime dir agrees everywhere: /run/$SESSION_RUNTIME"
else
    fail "runtime dir disagrees (unit RuntimeDirectory: '${SESSION_RUNTIME:-none}', shell XDG_RUNTIME_DIR: '${SHELL_RUNTIME:-none}', wrapper default: '${WRAPPER_RUNTIME:-none}')"
fi

SHELL_DISPLAY=$(sed -n 's/^Environment=WAYLAND_DISPLAY=//p' "$SHELL_UNIT" 2>/dev/null | head -1)
WRAPPER_DISPLAY=$(sed -n 's/.*WAYLAND_DISPLAY:-\([^}]*\)}.*/\1/p' "$SHELL_HOST" 2>/dev/null | head -1)
if [ -n "$SHELL_DISPLAY" ] && [ "$SHELL_DISPLAY" = "$WRAPPER_DISPLAY" ]; then
    pass "WAYLAND_DISPLAY agrees: $SHELL_DISPLAY"
else
    fail "WAYLAND_DISPLAY disagrees (unit: '${SHELL_DISPLAY:-none}', wrapper: '${WRAPPER_DISPLAY:-none}')"
fi

# systemd enable/mask wiring (symlinks in the overlay, copied as-is).
WANTS_SESSION="$UNITS/multi-user.target.wants/aether-session.service"
if [ -L "$WANTS_SESSION" ]; then
    target=$(readlink "$WANTS_SESSION")
    case "$target" in
        /etc/systemd/system/aether-session.service | ../aether-session.service)
            if [ -f "$UNITS/aether-session.service" ]; then
                pass "aether-session.service enabled in multi-user.target"
            else
                fail "multi-user.target.wants/aether-session.service points at $target, which does not exist in the overlay"
            fi
            ;;
        *)
            fail "multi-user.target.wants/aether-session.service points at $target (expected /etc/systemd/system/aether-session.service)"
            ;;
    esac
else
    fail "aether-session.service is not enabled (no multi-user.target.wants symlink)"
fi

GETTY_WANTS="$UNITS/getty.target.wants/getty@tty2.service"
if [ -L "$GETTY_WANTS" ]; then
    # The enablement symlink is an instance marker: systemd resolves the unit
    # named getty@tty2.service through the getty@.service template. Pointing at
    # the template is what `systemctl enable getty@tty2.service` produces (and
    # keeps the link from dangling); the instance comes from the link name.
    target=$(readlink "$GETTY_WANTS")
    case "$target" in
        /usr/lib/systemd/system/getty@.service | ../getty@.service)
            pass "console getty enabled on tty2 (instance marker -> getty@.service template)"
            ;;
        *)
            fail "getty.target.wants/getty@tty2.service points at $target (expected /usr/lib/systemd/system/getty@.service)"
            ;;
    esac
else
    fail "console getty is not enabled on tty2 (no getty.target.wants/getty@tty2.service)"
fi
if [ -L "$UNITS/getty.target.wants/getty@tty1.service" ]; then
    fail "getty@tty1.service is still wanted: the compositor would not own tty1"
else
    pass "no getty wanted on tty1"
fi
MASK="$UNITS/getty@tty1.service"
if [ -L "$MASK" ] && [ "$(readlink "$MASK")" = "/dev/null" ]; then
    pass "getty@tty1.service masked (the kernel-console getty generator cannot take tty1)"
else
    fail "getty@tty1.service is not masked to /dev/null (the console getty generator would spawn one on tty1)"
fi
# The harness address dump must follow the login VT, not the compositor's.
if grep -q '/dev/tty1' "$UNITS/show-ip.service" 2>/dev/null; then
    fail "show-ip.service writes to /dev/tty1, which the compositor owns"
elif grep -q '/dev/tty2' "$UNITS/show-ip.service" 2>/dev/null; then
    pass "show-ip.service writes to the tty2 login VT"
else
    fail "show-ip.service does not write the address dump to /dev/tty2"
fi

# --- boot switching (unit owned by the boot-switch session) --------------------
# AetherOS may only set BootNext back to Ubuntu if it is actually enabled on
# every boot, and a wants symlink without its unit fails silently at boot.
BOOT_RETURN_UNIT="$UNITS/aether-boot-return.service"
BOOT_RETURN_WANTS="$UNITS/multi-user.target.wants/aether-boot-return.service"
if [ -L "$BOOT_RETURN_WANTS" ]; then
    target=$(readlink "$BOOT_RETURN_WANTS")
    case "$target" in
        ../aether-boot-return.service | aether-boot-return.service | /etc/systemd/system/aether-boot-return.service)
            pass "aether-boot-return.service enabled in multi-user.target"
            ;;
        *)
            fail "multi-user.target.wants/aether-boot-return.service points at $target (expected ../aether-boot-return.service)"
            ;;
    esac
else
    fail "aether-boot-return.service is not enabled (no multi-user.target.wants symlink)"
fi
if [ -f "$BOOT_RETURN_UNIT" ]; then
    pass "unit present: aether-boot-return.service"
    if grep -q '^ExecStart=' "$BOOT_RETURN_UNIT"; then
        pass "aether-boot-return.service has an ExecStart"
    else
        fail "aether-boot-return.service has no ExecStart"
    fi
else
    fail "aether-boot-return.service unit missing: /etc/systemd/system/aether-boot-return.service (owned by the boot-switch session; the wants symlink above dangles until it lands)"
fi
if defconfig_enabled BR2_PACKAGE_EFIBOOTMGR; then
    pass "efibootmgr is in the image (BR2_PACKAGE_EFIBOOTMGR=y)"
else
    fail "BR2_PACKAGE_EFIBOOTMGR=y missing: aether-boot-return has no efibootmgr to call"
fi

# --------------------------------------------------------------- post-build ---

section "post-build ($POST_BUILD)"

if [ -f "$POST_BUILD" ]; then
    pass "post-build script present"
    if [ -x "$POST_BUILD" ]; then
        pass "post-build script executable"
    else
        fail "post-build script is not executable (Buildroot runs it directly)"
    fi
    if sh -n "$POST_BUILD" 2>/dev/null; then
        pass "post-build script parses (sh -n)"
    else
        fail "post-build script does not parse with sh -n"
    fi
    for marker in 'AETHER_SKIP_RUST_BUILD' 'cargo build --release -p aether-compositor --features drm' \
        'cargo build --release -p aether-shell' 'etc/aether/ui-build.txt' 'binaries.txt'; do
        if grep -q -- "$marker" "$POST_BUILD"; then
            pass "post-build script mentions $marker"
        else
            fail "post-build script does not mention $marker"
        fi
    done
else
    fail "post-build script missing: $POST_BUILD"
fi

# ------------------------------------------------------------- binaries list ---

section "binary list ($BINARY_LIST)"

if [ ! -f "$BINARY_LIST" ]; then
    fail "binary list missing: $BINARY_LIST"
else
    pass "binary list present"
    CORE=$(awk -F '\t' '$3 == "core" { print $2 }' "$BINARY_LIST" | sort)
    for core_bin in aether-compositor aether-shell aether-greeter; do
        if printf '%s\n' "$CORE" | grep -qx "$core_bin"; then
            pass "core binary listed: $core_bin"
        else
            fail "core binary not listed: $core_bin"
        fi
    done
    # Every package named in the list must exist in the workspace.
    for package in $(awk -F '\t' 'NF >= 2 && $1 !~ /^#/ { print $1 }' "$BINARY_LIST" | sort -u); do
        if grep -rqs -- "^name = \"$package\"" \
            "$REPO_ROOT/Cargo.toml" "$REPO_ROOT/compositor" "$REPO_ROOT/shell" \
            "$REPO_ROOT/apps" 2>/dev/null; then
            pass "workspace provides $package"
        else
            fail "no workspace crate named $package (listed in binaries.txt)"
        fi
    done
    # The units start these paths; they must be in the list.
    if [ -f "$POST_BUILD" ]; then
        for binary in aether-compositor aether-shell; do
            if grep -q "/usr/bin/$binary" "$UNITS/aether-session.service" "$UNITS/aether-shell.service" 2>/dev/null; then
                if awk -F '\t' '{ print $2 }' "$BINARY_LIST" | grep -qx "$binary"; then
                    pass "$binary (started by a unit) is installed"
                else
                    fail "$binary is started by a unit but not in binaries.txt"
                fi
            fi
        done
    fi
fi

# -------------------------------------------------------------------- summary ---

printf '\n'
if [ "$FAIL" -eq 0 ]; then
    printf 'check-ui-image: %s passed, 0 failed, %s note(s)\n' "$PASS" "$NOTE"
    exit 0
fi
printf 'check-ui-image: %s passed, %s FAILED, %s note(s)\n' "$PASS" "$FAIL" "$NOTE"
printf 'check-ui-image: one or more image wiring assertions failed (see FAIL above)\n'
exit 1
