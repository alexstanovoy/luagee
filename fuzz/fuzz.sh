#!/usr/bin/env bash
# Builds the fuzz targets with libFuzzer's coverage instrumentation on stable Rust (no sanitizer: that needs nightly)
# and runs them.
#
#   fuzz/fuzz.sh build                      build every target
#   fuzz/fuzz.sh check                      only type-check every target (seconds; no instrumented build)
#   fuzz/fuzz.sh run <target> [args...]     fuzz one target; extra arguments go to libFuzzer (e.g. -max_total_time=3600)
#   fuzz/fuzz.sh smoke [seconds]            every target at once for that long (default 20), each on one process: what
#                                           the merge gate runs; fails on any finding, prints each target's coverage
#   fuzz/fuzz.sh campaign <seconds> [jobs]  every target at once for that long, each in fork mode with `jobs` processes
#                                           (default 1), past crashes, hangs and memory blowups, all of which land in
#                                           artifacts/; an input slower than FUZZ_TIMEOUT seconds (default 30) is a hang
#   fuzz/fuzz.sh replay                     every corpus and artifact input once through its target (no fuzzing)
#
# Targets: parse, run_bytes, run_structured, stdlib, differential, determinism (see each fuzz_targets/*.rs). Each keeps
# its corpus in fuzz/corpus/<target>, seeded from the test corpora, and writes what it finds to fuzz/artifacts/<target>.
# The byte-input targets use the Lua token dictionary lua.dict. `differential` runs the reference interpreter that
# tools/ref/build.sh builds (or $LUAGEE_REFERENCE).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
export RUSTFLAGS="-Cpasses=sancov-module -Cllvm-args=-sanitizer-coverage-level=4 \
-Cllvm-args=-sanitizer-coverage-inline-8bit-counters -Cllvm-args=-sanitizer-coverage-pc-table \
-Cllvm-args=-sanitizer-coverage-trace-compares --cfg fuzzing -Cdebug-assertions -Coverflow-checks"
# An explicit target keeps the flags off build scripts and proc macros.
triple=$(rustc -vV | sed -n 's/^host: //p')
build() {
    cargo build --manifest-path "$here/Cargo.toml" --release --bins --target "$triple"
}
targets=(parse run_bytes run_structured stdlib differential determinism host_api)
dictionary() { # target
    case $1 in parse | run_bytes | determinism | host_api) echo "-dict=$here/lua.dict" ;; esac
}
seed() { # target
    mkdir -p "$here/corpus/$1" "$here/artifacts/$1"
    if [ "$1" = host_api ]; then
        cp -n "$here"/../crates/luagee/tests/host_api/*.lua "$here/corpus/$1/"
    fi
    cp -n "$here"/../crates/luagee/tests/conformance/*.lua "$here"/../crates/luagee/tests/hostile/*.lua "$here/corpus/$1/"
}
reference() {
    if [ -z "${LUAGEE_REFERENCE:-}" ]; then
        LUAGEE_REFERENCE=$("$here/../tools/ref/build.sh" | head -n 1)
        export LUAGEE_REFERENCE
    fi
}
binary() { # target
    echo "${CARGO_TARGET_DIR:-$here/target}/$triple/release/$1"
}
case "${1:-}" in
build) build ;;
check) RUSTFLAGS="--cfg fuzzing" cargo check --manifest-path "$here/Cargo.toml" --all-targets ;;
run)
    target=${2:?which target}
    shift 2
    build
    reference
    seed "$target"
    # shellcheck disable=SC2046
    exec "$(binary "$target")" -artifact_prefix="$here/artifacts/$target/" $(dictionary "$target") \
        "$here/corpus/$target" "$@"
    ;;
smoke)
    seconds=${2:-20}
    build
    reference
    logs=$(mktemp -d)
    pids=()
    for target in "${targets[@]}"; do
        seed "$target"
        # shellcheck disable=SC2046
        "$(binary "$target")" -artifact_prefix="$here/artifacts/$target/" $(dictionary "$target") \
            -max_total_time="$seconds" -timeout=20 -rss_limit_mb=2048 "$here/corpus/$target" >"$logs/$target.log" 2>&1 &
        pids+=($!)
    done
    failed=0
    for i in "${!targets[@]}"; do
        target=${targets[$i]}
        if wait "${pids[$i]}"; then
            stats=$(grep -E '^(#[0-9]+[[:space:]]+DONE|Done [0-9]+ runs)' "$logs/$target.log" | tail -n 2 | tr '\n' ' ')
            echo "$target: $stats"
        else
            failed=1
            echo "$target: FINDING"
            tail -n 60 "$logs/$target.log"
        fi
    done
    rm -rf "$logs"
    exit $failed
    ;;
campaign)
    seconds=${2:?how long}
    jobs=${3:-1}
    build
    reference
    for target in "${targets[@]}"; do
        seed "$target"
        # shellcheck disable=SC2046
        "$(binary "$target")" -artifact_prefix="$here/artifacts/$target/" $(dictionary "$target") \
            -fork="$jobs" -ignore_crashes=1 -ignore_timeouts=1 -ignore_ooms=1 \
            -max_total_time="$seconds" -timeout="${FUZZ_TIMEOUT:-30}" -rss_limit_mb=2048 -malloc_limit_mb=2048 \
            "$here/corpus/$target" >"$here/artifacts/$target.log" 2>&1 &
    done
    wait
    find "$here/artifacts" -mindepth 2 -type f | sort
    ;;
replay)
    build
    reference
    for target in "${targets[@]}"; do
        seed "$target"
        inputs=("$here/corpus/$target")
        if [ -n "$(find "$here/artifacts/$target" -type f -print -quit)" ]; then
            inputs+=("$here/artifacts/$target")
        fi
        "$(binary "$target")" -runs=0 -timeout=20 -rss_limit_mb=2048 "${inputs[@]}" >/dev/null 2>&1 || {
            echo "$target: an input fails"
            exit 1
        }
        echo "$target: every input passes"
    done
    ;;
*)
    sed -n '2,22p' "$0"
    exit 2
    ;;
esac
