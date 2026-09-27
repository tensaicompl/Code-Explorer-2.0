# Build and verification entry points.
#
# check      runs on every push and must be green before any task is merged.
# check-full runs nightly and on release tags.
#
# The licence, provenance and bundle-budget steps are added by the task that
# creates the scanners; the targets below already call them so that task only
# has to supply the scripts.

SHELL := /bin/bash
CARGO ?= cargo
PNPM  ?= pnpm
UI    := ui

.PHONY: all check check-full fmt fmt-check lint build test \
        ui-install ui-lint ui-test ui-build bundle-budget \
        licence-scan provenance-scan open-binary-check \
        golden oracle e2e asan determinism engine-differential perf \
        bindgen vendor-refresh clean

all: check

# --- the gate every task passes -------------------------------------------

# The scanner task adds licence-scan, provenance-scan and bundle-budget here; the
# targets exist already so that task only supplies the scripts.
check: fmt-check lint build test open-binary-check ui-lint ui-test

check-full: check golden oracle e2e asan determinism engine-differential perf

# --- Rust ------------------------------------------------------------------

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

lint:
	$(CARGO) clippy --all-targets --all-features -- -D warnings

build:
	$(CARGO) build --workspace --all-targets
	$(CARGO) build -p pdx --features enterprise

test:
	$(CARGO) test --workspace --all-features

# The public binary must not link an enterprise crate.
open-binary-check:
	@scripts/open-binary-check.sh

# --- interface -------------------------------------------------------------

ui-install:
	cd $(UI) && $(PNPM) install --frozen-lockfile

ui-lint:
	@if [ -d $(UI)/node_modules ]; then cd $(UI) && $(PNPM) run lint; \
	else echo "ui: dependencies absent, run make ui-install"; exit 1; fi

ui-test:
	@if [ -d $(UI)/node_modules ]; then cd $(UI) && $(PNPM) run test:run; \
	else echo "ui: dependencies absent, run make ui-install"; exit 1; fi

ui-build:
	cd $(UI) && $(PNPM) run build

bundle-budget:
	@scripts/bundle-budget.sh

# --- scanners (scripts arrive with the scanner task) -----------------------

licence-scan:
	@scripts/licence-scan.sh

provenance-scan:
	@scripts/provenance-scan.sh

# --- nightly ---------------------------------------------------------------

golden:
	$(CARGO) run -p pdx-bench -- golden

oracle:
	$(CARGO) run -p pdx-bench -- oracle

e2e:
	cd $(UI) && $(PNPM) run e2e

asan:
	@scripts/check-asan.sh

determinism:
	$(CARGO) run -p pdx-bench -- determinism

engine-differential:
	@scripts/engine-differential.sh

perf:
	$(CARGO) run -p pdx-bench -- perf

# --- maintenance -----------------------------------------------------------

bindgen:
	$(CARGO) build -p pdx-engine-sys --features regenerate-bindings

vendor-refresh:
	@scripts/vendor/fetch-engine.sh
	@scripts/vendor/copy-engine.sh
	@scripts/vendor/rename-engine.sh
	@scripts/vendor/strip-engine.sh
	@scripts/vendor/apply-patches.sh
	$(MAKE) build golden

clean:
	$(CARGO) clean
	rm -rf $(UI)/node_modules $(UI)/dist
