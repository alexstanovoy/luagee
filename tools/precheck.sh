#!/usr/bin/env bash
# The quick check while working, before tools/gate.sh: clippy and the tests, stopping at the first failure.
#
#   tools/precheck.sh
#
# The gate stays the confirmation (it adds the wasm build, a fuzz smoke, deny and fmt); formatting is no part of this.
set -uo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$here" || exit 2
step() { local name=$1 began=$SECONDS; shift; echo "== $name"; "$@" < /dev/null || { echo "PRECHECK-FAIL $name ($((SECONDS - began)) s)"; exit 1; }; echo "   $name $((SECONDS - began)) s"; }
step clippy cargo clippy --all-targets --all-features -- -D warnings
step test cargo test --profile test-release --all-features --quiet
echo "PRECHECK-OK"
