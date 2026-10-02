#!/usr/bin/env bash
# Every third-party crate in Cargo.lock must be covered by the cargo-vet store in
# supply-chain/ (plan Part 5.12; docs/plan/ISSUES.md, issue 36).
#
#   scripts/cargo-vet.sh           check: the pinned cargo-vet, then `cargo vet --locked`
#   scripts/cargo-vet.sh version   print the pinned version
#   scripts/cargo-vet.sh install   install the pinned version unless it is installed (CI)
#
# This file is the one place the cargo-vet version is written. The check verifies
# only: it installs nothing (`make vet` runs it), and nothing here ever runs
# `cargo vet init`, `regenerate` or anything else that accepts a crate.
#
# The store began with one-time bootstrap exemptions for the crates already in
# Cargo.lock when vetting was introduced. They are exemptions, not audits: nobody is
# claimed to have reviewed those versions. Since then a new or changed crate passes
# only through one of:
#   - a real local audit (`cargo vet certify`), by someone who reviewed it;
#   - an import of another organisation's audits, decided and recorded by the owner;
#   - an exemption for that exact version, with an entry in docs/plan/ISSUES.md
#     saying why it is accepted for now.
# A failing check is never fixed by regenerating the exemptions.
set -euo pipefail
cd "$(dirname "$0")/.."

CARGO_VET_VERSION=0.10.2

install="cargo install --locked --version $CARGO_VET_VERSION cargo-vet"

case "${1:-}" in
  version)
    echo "$CARGO_VET_VERSION"
    exit 0
    ;;
  install)
    if [ "$(cargo vet --version 2>/dev/null)" != "cargo-vet $CARGO_VET_VERSION" ]; then
      $install
    fi
    cargo vet --version
    exit 0
    ;;
  "") ;;
  *)
    echo "usage: scripts/cargo-vet.sh [version|install]" >&2
    exit 2
    ;;
esac

if ! installed="$(cargo vet --version 2>/dev/null)"; then
  echo "cargo vet: cargo-vet $CARGO_VET_VERSION is not installed; install it with: $install" >&2
  exit 1
fi
if [ "$installed" != "cargo-vet $CARGO_VET_VERSION" ]; then
  echo "cargo vet: found $installed, and this repository pins $CARGO_VET_VERSION; install it with: $install" >&2
  exit 1
fi
cargo vet --locked
