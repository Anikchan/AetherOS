# os-build

Build system, kernel configuration, and image pipeline (design D4).

- P0-T11 adds `budgets.toml` (size/RAM/CPU budgets read by CI).
- P1-T01 adds the Buildroot configuration and kernel fragment.

## Common operations

See `os-build/Makefile` (`make help`) and `os-build/scripts/`.

## Local type-check recipe (macOS host -> Linux x86-64)

```sh
brew install rustup
rustup toolchain install stable -t x86_64-unknown-linux-gnu
os-build/scripts/type-check.sh            # whole workspace
os-build/scripts/type-check.sh spikes/    # a sub-crate
# or: make -C os-build type-check
```

Cargo picks up the host rustc by default; the `RUSTC=` override in
`type-check.sh` is required so the linux std is found.

## Boot testing

```sh
os-build/test/boot-test.sh <iso> <bios|uefi> [timeout]
os-build/test/disk-image-test.sh <image> <bios|uefi> [timeout]
# or via make:
make -C os-build boot-test ISO=path MODE=bios
make -C os-build disk-image-test IMAGE=path MODE=uefi
```

## Metrics

Fixed text format documented in `os-build/metrics/FORMAT.md`. Template:
`os-build/metrics/TEMPLATE.txt`. Validate with
`os-build/check-budgets.py <metrics-file>` or `make check-budgets`.