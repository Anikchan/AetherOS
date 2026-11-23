# os-build/scripts — developer helper scripts (S4 territory)

| Script | Purpose |
| --- | --- |
| `type-check.sh` | Local Rust type-check recipe for macOS host -> Linux x86-64 (rustup). |
| `fetch-kernel.sh` | Download/cache the upstream kernel tarball for kernel-config checks. |
| `ci-local.sh` | Run the local CI checks (build, test, budgets). |
| `metrics-export.sh` | Export a metrics file in D6 fixed text format. |
| `artifact-clean.sh` | Remove local build artifacts and caches. |
| `kernel-config-check.sh` | Cached reimplementation of the kernel-config workflow (optimization proposal for S1). |
| `spike-runner.sh` | Reusable harness for running a spike script with timeout, log capture, and marker verification (supports S2). |
| `image-checksum.sh` | Create/verify SHA-256 sidecar files for ISO/IMG artifacts (P9-T05; signing deferred to public release). |

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