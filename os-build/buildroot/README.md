# os-build/buildroot — Buildroot tree and AetherOS defconfig (P1-T01)

Buildroot cannot run on macOS. Two supported ways to build:

1. **Linux test VM** (ADR-017): clone Buildroot into this directory so that
   this folder *is* the Buildroot tree, then:

   ```
   git clone https://gitlab.com/buildroot.org/buildroot.git .
   make BR2_DEFCONFIG=aetheros_defconfig defconfig
   make
   ```

   The kernel fragment `../kernel/aether.fragment` is merged automatically
   (path in the defconfig assumes this layout).

2. **GitHub Actions** (Linux runners): the `kernel-config` workflow
   (`.github/workflows/kernel-config.yml`) validates the kernel fragment
   against a real upstream kernel on every push touching `os-build/`. A full
   image build job can be added when the first image is produced (P1-T03+).

Notes:
- Package symbols (`UPOWER`, `POWER_PROFILES_DAEMON`, `NETWORKMANAGER`, ...)
  must be re-checked against the pinned Buildroot version at first build.
- The first build's image size is checked against `os-build/budgets.toml`
  (G-RES1, base image ≤ 150 MB compressed for P1-T06).
