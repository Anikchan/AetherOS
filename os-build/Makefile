# os-build/Makefile — common developer operations for AetherOS build/test tooling.
# Run from the os-build/ directory: `make <target>`.
#
# Targets are intentionally thin wrappers around the scripts in os-build/test/
# and os-build/scripts/ so the same commands work locally and in CI.

SHELL := /bin/bash
.DEFAULT_GOAL := help

OS_BUILD_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
ROOT_DIR := $(abspath $(OS_BUILD_DIR)/..)
PYTHON ?= python3
QEMU ?= qemu-system-x86_64
TIMEOUT ?= 180

.PHONY: help
help: ## Show available targets
	@awk 'BEGIN {FS = ":.*##"; printf "Usage: make <target>\n\nTargets:\n"} /^[a-zA-Z_-]+:.*##/ {printf "  %-24s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

.PHONY: build
build: ## Build the Rust workspace
	cd $(ROOT_DIR) && cargo build --workspace

.PHONY: test
test: ## Run the Rust workspace tests
	cd $(ROOT_DIR) && cargo test --workspace

.PHONY: landlock-probe
landlock-probe: ## Probe Landlock on this host (exit 126 = unavailable, fail-closed)
	cargo run -q -p aether-sandbox --bin aether-landlock -- --probe

.PHONY: check-budgets
check-budgets: ## Compare metrics/sample.txt against budgets.toml
	cd $(ROOT_DIR) && $(PYTHON) os-build/check-budgets.py os-build/metrics/sample.txt

.PHONY: check-budgets-file
check-budgets-file: ## Compare a metrics file: make check-budgets-file FILE=path
	cd $(ROOT_DIR) && $(PYTHON) os-build/check-budgets.py $(FILE)

.PHONY: type-check
type-check: ## Local Linux type-check recipe (host macOS -> x86_64-unknown-linux-gnu)
	bash $(OS_BUILD_DIR)/scripts/type-check.sh

.PHONY: boot-test
boot-test: ## Boot an ISO in QEMU: make boot-test ISO=path MODE=bios|uefi
	bash $(OS_BUILD_DIR)/test/boot-test.sh $(ISO) $(MODE) $(TIMEOUT)

.PHONY: disk-image-test
disk-image-test: ## Boot a raw disk image in QEMU: make disk-image-test IMAGE=path MODE=bios|uefi
	bash $(OS_BUILD_DIR)/test/disk-image-test.sh $(IMAGE) $(MODE) $(TIMEOUT)

.PHONY: fetch-kernel
fetch-kernel: ## Download/cache the upstream kernel tarball for kernel-config checks
	bash $(OS_BUILD_DIR)/scripts/fetch-kernel.sh

.PHONY: live-usb
live-usb: ## Build the live USB image (Linux host): make live-usb [PERSIST_MB=1024]
	bash $(OS_BUILD_DIR)/scripts/make-live-usb.sh \
		--binaries $(OS_BUILD_DIR)/buildroot/output/images \
		--persist-mb $(or $(PERSIST_MB),1024)

.PHONY: live-usb-plan
live-usb-plan: ## Print the live USB layout without writing (works on macOS)
	bash $(OS_BUILD_DIR)/scripts/make-live-usb.sh \
		--binaries $(OS_BUILD_DIR)/buildroot/output/images --dry-run

.PHONY: rauc-bundle
rauc-bundle: ## Build a RAUC bundle: make rauc-bundle VERSION=0.1.0 [IMAGE=rootfs]
	bash $(OS_BUILD_DIR)/scripts/rauc-bundle.sh \
		--image $(or $(IMAGE),$(OS_BUILD_DIR)/buildroot/output/images/rootfs.squashfs) \
		--version $(or $(VERSION),0.1.0)

.PHONY: soak-smoke
soak-smoke: ## Compressed soak smoke test (G-PM5 harness, model host)
	cd $(ROOT_DIR) && cargo build -q -p aether-lifecycled --bin aether-bench
	cd $(ROOT_DIR) && PATH="$(ROOT_DIR)/target/debug:$$PATH" SOAK_HOURS=0.01 \
		CYCLES=$(or $(CYCLES),2) CYCLE_SECONDS=0 APPS=$(or $(APPS),50) \
		OUT_DIR=$(or $(OUT_DIR),/tmp/aether-soak) \
		bash lifecycled/bench/soak-72h.sh

.PHONY: gate-report-selftest
gate-report-selftest: ## Self-test the G-PM1/G-PM5 gate report generator
	cd $(ROOT_DIR) && python3 lifecycled/bench/gate-report.py --self-test

.PHONY: energy-contract
energy-contract: ## Print the Activity Manager Energy column contract (JSON)
	cd $(ROOT_DIR) && cargo run -q -p aether-taskmanager -- --energy-contract

.PHONY: ci-local
ci-local: ## Run the local CI checks (build + test + budgets)
	bash $(OS_BUILD_DIR)/scripts/ci-local.sh

.PHONY: clean
clean: ## Remove build artifacts and temp dirs
	cd $(ROOT_DIR) && cargo clean
	rm -rf $(OS_BUILD_DIR)/.cache