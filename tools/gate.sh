#!/usr/bin/env bash
# The merge gate of this repository: every check a change must pass, in order, stopping at the first failure.
#
#   tools/gate.sh
#
# Steps: ignored-audit (every #[ignore]d test has a step that runs it), wasm-audit (the library uses no host clock,
# environment, threads, randomness or files outside the `fs` loader), fuzz-check (the fuzz targets compile), clippy
# (all targets and features, -D warnings), test (profile test-release: unit tests, the conformance and hostile corpora,
# the borrowed suites, the sandbox's bounds), wasm (the crate builds for wasm32-unknown-unknown without features,
# warning-free),
# fuzz-smoke (every fuzz target at once, GATE_FUZZ_SECONDS each, seeded from the corpora; the differential target
# against the reference LuaJIT that tools/ref/build.sh builds from pinned sources), deny, then fmt (last: it changes
# no code).
# A step that does not run fails the gate; nothing is skipped quietly.
# Environment:
#   GATE_STEP_LOG=FILE   append the seconds each step took to FILE (tab separated)
#   GATE_LOGS=DIR        where each step's output goes (default: temp/gate under the repository)
#   GATE_FUZZ_SECONDS=N  how long each fuzz target runs in fuzz-smoke (default 20)
# Prints GATE-OK when every step ran and passed, GATE-FAIL <step> (with the tail of its output) otherwise.
set -uo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$here" || exit 2
logs=${GATE_LOGS:-$here/temp/gate}
mkdir -p "$logs"

step() { # name command...
    local name=$1
    shift
    echo "== $name"
    local began=$SECONDS
    "$@" > "$logs/$name.log" 2>&1
    local rc=$?
    [ -z "${GATE_STEP_LOG:-}" ] || printf '%s\t%s\t%s\t%s\t%s\n' "$(date +%FT%T)" "$(basename -- "$here")" "$name" "$((SECONDS - began))" "$rc" >> "$GATE_STEP_LOG"
    if [ $rc -ne 0 ]; then
        tail -n 40 "$logs/$name.log"
        echo "GATE-FAIL $name (full output: $logs/$name.log)"
        exit 1
    fi
    grep -E '^test result' "$logs/$name.log" | awk '{p += $4; f += $6; i += $8} END {if (NR) print "   " p " passed, " f " failed, " i " ignored"}'
}

step ignored-audit tools/ignored_audit.sh
step wasm-audit tools/wasm_audit.sh
step fuzz-check fuzz/fuzz.sh check
step clippy cargo clippy --all-targets --all-features -- -D warnings
step test cargo test --profile test-release --all-features
# A build, not a check: some lints (a shift past 32 bits) fire only in code generation.
step wasm env RUSTFLAGS="-D warnings" cargo build --release -p luagee --no-default-features --target wasm32-unknown-unknown
step fuzz-smoke fuzz/fuzz.sh smoke "${GATE_FUZZ_SECONDS:-20}"
step deny cargo deny check
step fmt cargo fmt --check
echo "GATE-OK"
