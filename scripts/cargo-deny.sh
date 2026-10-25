#!/usr/bin/env bash
# The dependency policy in deny.toml: advisories (with yanked crates), bans (wildcard
# requirements, duplicates as warnings), licences and sources.
#
#   scripts/cargo-deny.sh check           the pinned cargo-deny, the whole policy
#   scripts/cargo-deny.sh check licenses  one part of it (scripts/licence-scan.sh)
#   scripts/cargo-deny.sh version         print the pinned version
#   scripts/cargo-deny.sh install         install the pinned version unless it is (CI)
#
# This file is the one place the cargo-deny version is written. `check` with no part
# fetches the advisory database, so `make check` runs only the offline licence part and
# CI runs the whole policy (`make deny`). CARGO_DENY_LOG_LEVEL sets cargo-deny's log
# level (default warn).
set -euo pipefail
cd "$(dirname "$0")/.."

CARGO_DENY_VERSION=0.20.2

install="cargo install --locked --version $CARGO_DENY_VERSION cargo-deny"

case "${1:-}" in
  version)
    echo "$CARGO_DENY_VERSION"
    exit 0
    ;;
  install)
    if [ "$(cargo deny --version 2>/dev/null)" != "cargo-deny $CARGO_DENY_VERSION" ]; then
      $install --force
    fi
    cargo deny --version
    exit 0
    ;;
  check) shift ;;
  *)
    echo "usage: scripts/cargo-deny.sh [check [part...]|version|install]" >&2
    exit 2
    ;;
esac

if ! installed="$(cargo deny --version 2>/dev/null)"; then
  echo "cargo deny: cargo-deny $CARGO_DENY_VERSION is not installed; install it with: $install" >&2
  exit 1
fi
if [ "$installed" != "cargo-deny $CARGO_DENY_VERSION" ]; then
  echo "cargo deny: found $installed, and this repository pins $CARGO_DENY_VERSION; install it with: $install" >&2
  exit 1
fi
cargo deny --log-level "${CARGO_DENY_LOG_LEVEL:-warn}" check "$@"
