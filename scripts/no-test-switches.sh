#!/usr/bin/env bash
# A binary built for use must not contain the engine's fault-injection switches.
#
# The switches make the engine abort, hang or skip work on a file named in an
# environment variable. They exist for tests and are compiled in only by the
# test-seams feature of crates/pdx-engine-sys, which only dev-dependencies enable, so
# an ordinary build never has them. This checks the binary itself, by the names the
# switches read, rather than trusting the feature wiring.
#
#     scripts/no-test-switches.sh <binary>
set -euo pipefail

bin="${1:?usage: no-test-switches.sh <binary>}"
[ -f "$bin" ] || { echo "error: no binary at $bin" >&2; exit 1; }

# The engine's own configuration variable, always present: without it the binary has
# no engine in it and the check below would prove nothing.
if ! grep -a -q "PDX_ENGINE_WALK_MAX_NODES" "$bin"; then
  echo "error: $bin does not contain the engine; nothing to check" >&2
  exit 1
fi
if grep -a -q "PDX_ENGINE_TEST_" "$bin"; then
  echo "error: $bin contains the engine's test switches; it was built with test-seams" >&2
  exit 1
fi
echo "$bin: no test switches"
