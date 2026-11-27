# os-build/scripts — developer helper scripts (S4 territory)

| Script | Purpose |
| --- | --- |
| `type-check.sh` | Local Rust type-check recipe for macOS host -> Linux x86-64 (rustup). |
| `fetch-kernel.sh` | Download/cache the upstream kernel tarball for kernel-config checks. |
| `ci-local.sh` | Run the same gates as `.github/workflows/ci.yml` locally, including the Linux-parity test summary; host-dependent gates print why they were skipped. |
| `metrics-export.sh` | Export a metrics file in D6 fixed text format. |
| `artifact-clean.sh` | Remove local build artifacts and caches. |
| `kernel-config-check.sh` | Cached reimplementation of the kernel-config workflow (optimization proposal for S1). |
| `spike-runner.sh` | Reusable harness for running a spike script with timeout, log capture, and marker verification (supports S2). |
| `image-checksum.sh` | Create/verify SHA-256 sidecar files for ISO/IMG artifacts (P9-T05; signing deferred to public release). |
| `make-live-usb.sh` | Assemble the GPT live USB image (ESP + squashfs root + persistence) rootlessly (P8-T01). `--dry-run` prints the plan anywhere. |
| `rauc-bundle.sh` | Render `manifest.raucm` and build a signed RAUC update bundle (P9-T01). `--dry-run` prints the plan anywhere. |
| `check-defconfig-drop.sh` | Fail when a `=y` entry in `aetheros_defconfig` did not survive `make defconfig` (the WirePlumber class of bug). `--self-test` runs fixtures anywhere. |
| `check-image-capabilities.sh` | Static scan: no in-image script may call a tool from the `absent` list in `os-build/test/capabilities.expected` without guarding it. |
| `check-test-summary.sh` | Turn a `cargo test` log into a CI summary that names failing tests and their captured output (Linux-parity job). |

## Local type-check recipe

```sh
# One-time setup:
brew install rustup
rustup toolchain install stable -t x86_64-unknown-linux-gnu

# Run:
os-build/scripts/type-check.sh            # whole workspace
os-build/scripts/type-check.sh spikes/    # a sub-crate
# or: make -C os-build type-check
```

Cargo picks up the host rustc by default; the `RUSTC=` override in
`type-check.sh` is required so the linux std is found (see the 2026-11-18
plan entry "LOCAL LINUX TYPE-CHECK ESTABLISHED").