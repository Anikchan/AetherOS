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

.PHONY: ci-local
ci-local: ## Run the local CI checks (build + test + budgets)
	bash $(OS_BUILD_DIR)/scripts/ci-local.sh

.PHONY: clean
clean: ## Remove build artifacts and temp dirs
	cd $(ROOT_DIR) && cargo clean
	rm -rf $(OS_BUILD_DIR)/.cache