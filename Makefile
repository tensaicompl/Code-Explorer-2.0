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

.PHONY: all check check-full fmt fmt-check lint build test consts-sync engine engine-clean \
        engine-test engine-typed-reference \
        ui-install ui-lint ui-test ui-build bundle-budget \
        licence-scan provenance-scan open-binary-check \
        golden oracle e2e check-asan asan determinism engine-differential perf \
        vet bindgen vendor-refresh vendor-verify clean

all: check

# --- the gate every task passes -------------------------------------------

check: fmt-check lint build test engine-test consts-sync open-binary-check licence-scan provenance-scan ui-lint ui-test bundle-budget

check-full: check golden oracle e2e check-asan determinism engine-differential perf

# --- Rust ------------------------------------------------------------------

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

lint:
	$(CARGO) clippy --all-targets --all-features -- -D warnings

build: engine
	$(CARGO) build --workspace --all-targets
	$(CARGO) build -p pdx --features enterprise

# The engine is C and is built by CMake. It joins the Rust build in the task that
# adds its bindings; until then it is built here so that every platform compiles it.
ENGINE_BUILD ?= target/engine
# The engine's warning policy is stated on every configure. CMake keeps an option's
# last value in its cache, so a build directory once configured with it off would stay
# off, silently. Turning it off takes asking: make engine PDXE_VENDORED_WERROR=OFF.
PDXE_VENDORED_WERROR ?= ON
# Parallel to CMake's own CMAKE_BUILD_PARALLEL_LEVEL when it is set, else one job per
# online processor: never `-j` alone, which the Makefile generator runs unbounded.
engine:
	@cmake -S engine -B $(ENGINE_BUILD) -DCMAKE_BUILD_TYPE=Release -DPDXE_BUILD_TESTS=ON -DPDXE_TEST_SEAMS=ON \
	  -DPDXE_VENDORED_WERROR=$(PDXE_VENDORED_WERROR) > /dev/null
	@cmake --build $(ENGINE_BUILD) --parallel "$${CMAKE_BUILD_PARALLEL_LEVEL:-$$(getconf _NPROCESSORS_ONLN)}"
	@echo "engine: built $(ENGINE_BUILD)/libpdxe.a"

# The interface's own tests: C programs over the archive, and typed resolution
# against the reference's recorded answers.
engine-test: engine
	@ctest --test-dir $(ENGINE_BUILD) --output-on-failure

# Re-records the reference's answers for every resolution fixture. Needs the pinned
# reference checkout; builds the reference outside the tree.
engine-typed-reference: engine
	@python3 scripts/engine/typed-differential.py --reference --update \
	  --dump $(ENGINE_BUILD)/tests/resolve_dump engine/tests/fixtures/resolve/*

engine-clean:
	rm -rf $(ENGINE_BUILD)

# The engine is built first, and its archive checks are required rather than
# skipped: here the archive is expected to exist (crates/pdx-bench/tests/engine_build.rs).
test: engine
	PDX_REQUIRE_ENGINE=1 $(CARGO) test --workspace --all-features

# Every third-party crate in Cargo.lock is covered by supply-chain/: audited, imported
# by an owner's decision, or exempted on the record (Part 5.12, issue 36). Needs the
# pinned cargo-vet, which this does not install; the message says how.
vet:
	@scripts/cargo-vet.sh

# The two constant definitions must agree.
consts-sync:
	@python3 scripts/consts-sync.py --root .

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

# Targets whose owning tasks have not landed stand in with an explicit skip, so a
# red nightly means a real regression (scripts/pending-target.sh).
golden:
	@scripts/pending-target.sh golden P2-12

oracle:
	@scripts/pending-target.sh oracle P5-06

e2e:
	@scripts/pending-target.sh e2e P7-01

# The engine built with the address, undefined-behaviour and leak sanitizers, its tests
# and the sanitizer corpus (bench/corpus) run under them. check-full runs it, so the
# nightly build cannot pass while either fails. `asan` is the earlier name, kept.
check-asan:
	@scripts/check-asan.sh

asan: check-asan

determinism:
	@scripts/pending-target.sh determinism P2-14

engine-differential:
	@scripts/pending-target.sh engine-differential P2-16

perf:
	@scripts/pending-target.sh perf P2-14

# --- maintenance -----------------------------------------------------------

# Regenerates crates/pdx-engine-sys/src/bindings.rs from engine/include/pdxe.h.
# Needs libclang. Any other build with the feature checks the file is current.
bindgen:
	PDX_WRITE_BINDINGS=1 $(CARGO) build -p pdx-engine-sys --features regenerate-bindings

# Re-runs the vendoring at the pinned commit and fails unless it reproduces the
# committed engine exactly. Needs the pinned reference checkout.
vendor-verify:
	@scripts/vendor/verify-refresh.sh

vendor-refresh:
	@scripts/vendor/fetch-engine.sh
	@scripts/vendor/copy-engine.sh
	@scripts/vendor/rename-engine.sh
	@scripts/vendor/strip-engine.sh
	@scripts/vendor/apply-patches.sh
	$(MAKE) build golden

clean: engine-clean
	$(CARGO) clean
	rm -rf $(UI)/node_modules $(UI)/dist
