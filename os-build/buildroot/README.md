# os-build/buildroot — Buildroot tree and AetherOS defconfig (P1-T01)

Buildroot cannot run on macOS. Two supported ways to build:

## 1. Linux test VM (ADR-017)

Clone Buildroot (pinned release) into a subdirectory of this folder, then:

```
git clone --depth 1 --branch 2025.11 https://gitlab.com/buildroot.org/buildroot.git buildroot
cd buildroot
make BR2_DEFCONFIG=../aetheros_defconfig defconfig
make -j"$(nproc)"
```

The kernel fragment `../../kernel/aether.fragment` is merged automatically
(path recorded in the defconfig; same layout as the CI job).

## 2. GitHub Actions (Linux runners)

- `buildroot-build` workflow: full image build; reports the base image size
  against the P1-T06 budget (≤ 150 MB compressed) and uploads the images.
  Runs on manual dispatch or when `os-build/buildroot/**` or
  `os-build/kernel/**` change (a full build uses about an hour of runner time).
- `kernel-config` workflow: cheap config-only fragment check on every push.

Notes:
- Package symbols (`UPOWER`, `POWER_PROFILES_DAEMON`, `NETWORKMANAGER`, ...)
  must be re-checked against the pinned Buildroot version at first build.
- The first build's image size is checked against `os-build/budgets.toml`
  (G-RES1; base image ≤ 150 MB compressed for P1-T06).
