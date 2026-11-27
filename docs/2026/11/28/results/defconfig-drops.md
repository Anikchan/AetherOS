# Defconfig drops found by the new guard (2026-11-28)

The `Defconfig drop guard` step added during the overnight run (`c5cceef`,
fixed path resolution in `d60bd7d`) failed the first image build that reached
it, naming **8 symbols the defconfig asked for that kconfig never produced**.
This is the class of defect that had already cost a PipeWire session manager,
so every item is recorded here rather than silenced.

Run: build `46056906489` (commit `d60bd7d`), step "Defconfig drop guard".

| Symbol | Why it was absent | Action |
| --- | --- | --- |
| `BR2_PACKAGE_POWER_PROFILES_DAEMON` | Not a Buildroot 2025.11 package (the symbol does not exist; `package/…/Config.in` is a 404 on both mirrors). It never had any effect. | Removed, replaced by an explicit comment. `powerd` owns power policy on AetherOS. |
| `BR2_PACKAGE_MESA3D_GALLIUM_DRIVER_IRIS` | Depends on `BR2_PACKAGE_MESA3D_LLVM`, which was never enabled. | Disabled with a comment naming the LLVM dependency. |
| `BR2_PACKAGE_MESA3D_GALLIUM_DRIVER_RADEONSI` | Same (LLVM). | Same. |
| `BR2_PACKAGE_MESA3D_VULKAN_DRIVER_INTEL` | Same (LLVM). | Same. |
| `BR2_PACKAGE_MESA3D_VULKAN_DRIVER_AMD` | Same (LLVM). | Same. |
| `BR2_PACKAGE_XWAYLAND` | Not a Buildroot 2025.11 package name; XWayland is a sub-option of the Xorg server package. | Disabled with a comment; the image's apps use the native Wayland backend. |
| `BR2_PACKAGE_EUDEV` | Conflicts with systemd's udev provider; the symbol does not survive a systemd build. | Removed. |
| `BR2_PACKAGE_UDEV` | Not a symbol in a systemd build (`BR2_PACKAGE_HAS_UDEV` is the provider flag). | Removed. |

## Consequences that were true all along

- **Hardware GPU acceleration was never in the image.** Every validated "graphics"
  result (P2 run 2 and run 3, the compositor's EGL checks) used **softpipe /
  software rendering**, because iris/radeonsi/ANV/RADV were dropped silently.
  This matches the first UI image, which is built for `kms_swrast`.
- **XWayland was never in the image**, so the compositor's XWayland support
  (`P3-T03`) has not run on device. Native Wayland is what the apps use now.
- **udev works**, because systemd provides it; the two removed lines were
  claiming a package that could not exist alongside systemd.

## Decision left to the owner

Enabling hardware GL needs **one line** plus a size/time budget:

```
BR2_PACKAGE_MESA3D_LLVM=y
# then re-enable the four drivers above
```

Costs: roughly +30–45 min build time and tens of MB in the compressed image
(the P1-T06 gate is 150 MB; the current rootfs is ~39 MB and the UI binaries add
~30–35 MB compressed). Benefit: Iris Xe hardware EGL/Vulkan instead of
`kms_swrast`, which is what a day-to-day desktop needs.

The guard now makes this choice explicit: re-enabling a driver without LLVM
fails the build with the symbol's name instead of shipping a slower image.

## Second finding — a package that cannot build at all (2026-11-28 03:46)

Build `46060990220` (commit `c6e26ef`, the first run that got past the guard)
failed in `mesa3d-demos-9.0.0`:

```
output/build/mesa3d-demos-9.0.0/meson.build:158:17: ERROR:
Program 'glslangValidator' not found or not executable
```

`BR2_PACKAGE_MESA3D_DEMOS=y` (added for `eglinfo`) has an undeclared host
requirement: Buildroot 2025.11 has **no `glslang` package**, so the demos cannot
build on a clean runner. Disabled with a comment; EGL is verified from the
compositor's own `BACKEND`/`EGL` markers and from `eglinfo` only if it ever
ships. This is the same lesson as the eight drops above: an enabled symbol is
not a built feature.
