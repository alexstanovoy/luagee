#!/usr/bin/env bash
# luagee under WebAssembly (wasm32-wasip1, run by a pinned wasmtime): the corpora, the binary size, the speed.
#
#   tools/wasm.sh corpus   build examples/corpus.rs for wasm32-wasip1 and run it under wasmtime on the conformance and
#                          hostile corpora, the borrowed suites (tests/suites.rs) and every fuzz/corpus/* folder (each
#                          input run as a script), on one thread and less stack than wasm's defaults: what the merge
#                          gate runs
#   tools/wasm.sh size     the size of a minimal host embedding luagee (release, then wasm-opt -Oz)
#   tools/wasm.sh bench    tools/bench kernels, native against wasmtime, interleaved, median of 5
#
# Tools are pinned by version and sha256 and downloaded into a fresh folder under temp/wasm-tools; nothing runs
# before its archive's checksum matches. A missing Rust target is installed with rustup. Any missing piece fails.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$here"
tools=$here/temp/wasm-tools
target=wasm32-wasip1

# wasmtime 50.0.1: the security fixes of 2026-10-29 on 50.0.0.
wasmtime_version=v50.0.1
wasmtime_archive=wasmtime-$wasmtime_version-x86_64-linux.tar.xz
wasmtime_url=https://github.com/bytecodealliance/wasmtime/releases/download/$wasmtime_version/$wasmtime_archive
wasmtime_sha256=685466455dd42a14774a86c36c8f2e45cbb9bd6d92a850139315df60a75498a5
binaryen_version=version_133
binaryen_archive=binaryen-$binaryen_version-x86_64-linux.tar.gz
binaryen_url=https://github.com/WebAssembly/binaryen/releases/download/$binaryen_version/$binaryen_archive
binaryen_sha256=2dc9c7813f5375db93d96ead4b78222fcc3e2677bbb832297af4797782a37489

# fetch NAME URL SHA256 MEMBER: prints the path of MEMBER (a path inside the archive) once the archive is verified.
# The tool's folder appears only after its archive matched, so a folder that exists holds a verified tool.
fetch() {
    local name=$1 url=$2 sha256=$3 member=$4
    local kept=$tools/$name
    if [ ! -d "$kept" ]; then
        mkdir -p "$tools"
        local fresh
        fresh=$(mktemp -d "$tools/download.XXXXXX")
        curl --fail --silent --show-error --location --proto '=https' --user-agent 'Mozilla/5.0' \
            --output "$fresh/archive" "$url" >&2
        if ! echo "$sha256  $fresh/archive" | sha256sum --check --quiet --status; then
            echo "wasm.sh: $url does not match its pinned sha256 $sha256; refusing it" >&2
            rm -r -- "$fresh"
            exit 1
        fi
        mkdir "$fresh/tool"
        tar --extract --file "$fresh/archive" --directory "$fresh/tool" --no-same-owner
        mv -T -- "$fresh/tool" "$kept"
        rm -r -- "$fresh"
    fi
    [ -x "$kept/$member" ] || { echo "wasm.sh: $kept/$member is missing" >&2; exit 1; }
    echo "$kept/$member"
}
wasmtime() { fetch "$wasmtime_version" "$wasmtime_url" "$wasmtime_sha256" "${wasmtime_archive%.tar.xz}/wasmtime"; }
wasm_opt() { fetch "$binaryen_version" "$binaryen_url" "$binaryen_sha256" "binaryen-$binaryen_version/bin/wasm-opt"; }

need_target() {
    rustup target list --installed | grep -qx "$target" || rustup target add "$target"
}

case "${1:-}" in
corpus)
    need_target
    runtime=$(wasmtime)
    # The linear stack stays half rustc's 1 MiB default; the engine budget is 448 KiB versus wasmtime's 512 KiB.
    # Wider values and unwind metadata left the former 384 KiB engine gate under 8 KiB above the measured
    # deepest-coroutine threshold. The extra 64 KiB accommodates toolchain layout variation while retaining
    # a 64 KiB margin below the default engine budget. Keep timeout instrumentation in focused reproductions.
    cargo rustc --profile test-release -p luagee --example corpus --target "$target" -- \
        -C link-arg=-zstack-size=$((512 << 10))
    runner=${CARGO_TARGET_DIR:-target}/$target/test-release/examples/corpus.wasm
    # The folders are the guest's only files; the suites get a top-level one of their own, as the folder loader
    # resolves a folder only below one. timeout is a hang guard; fuel ends every script long before it.
    run() {
        "$runtime" run -W max-wasm-stack=$((448 << 10)) -W timeout=1200s --dir "$here::/repo" \
            --dir "$here/crates/luagee/tests/suites::/suites" "$runner" "$@"
    }
    run conformance /repo/crates/luagee/tests/conformance
    run hostile /repo/crates/luagee/tests/hostile
    run suites
    run host-api
    fuzzed=0
    for corpus in fuzz/corpus/*/; do
        if [ -d "$corpus" ]; then
            run fuzz "/repo/${corpus%/}"
            fuzzed=1
        fi
    done
    [ "$fuzzed" = 1 ] || echo "wasm.sh: no fuzz corpus yet (fuzz/fuzz.sh smoke seeds one); the gate runs that first"
    ;;
size)
    rustup target list --installed | grep -qx wasm32-unknown-unknown || rustup target add wasm32-unknown-unknown
    optimizer=$(wasm_opt)
    out=$here/temp/wasm-size
    mkdir -p "$out"
    # Each setting in a target folder of its own, so that switching settings rebuilds nothing twice.
    for level in 3 s z; do
        CARGO_PROFILE_RELEASE_OPT_LEVEL=$level CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_PANIC=abort \
            cargo build --quiet --release -p luagee --no-default-features --example embed \
            --target wasm32-unknown-unknown --target-dir "$out/target-$level"
        built=$out/target-$level/wasm32-unknown-unknown/release/examples/embed.wasm
        "$optimizer" -Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
            --enable-mutable-globals "$built" -o "$out/embed-$level-oz.wasm"
        printf 'opt-level=%s lto=fat: %8d bytes, wasm-opt -Oz: %8d bytes, gzip -9 of that: %8d bytes\n' "$level" \
            "$(stat -c %s "$built")" "$(stat -c %s "$out/embed-$level-oz.wasm")" \
            "$(gzip -9 --stdout "$out/embed-$level-oz.wasm" | wc -c)"
    done
    ;;
bench)
    need_target
    runtime=$(wasmtime)
    cargo build --quiet --release -p luagee --example lua
    cargo build --quiet --release -p luagee --example lua --target "$target"
    # Compiled ahead, so that the timings are the interpreter's, not Cranelift's. The kernels' folder is /bench in the
    # guest: under WASI the folder loader resolves a folder only below a top-level preopened one.
    # {} expands to the kernel's relative path without its suffix, including nested folders.
    "$runtime" compile -o temp/lua.cwasm "target/$target/release/examples/lua.wasm"
    python3 -I tools/bench/bench.py --output temp/wasm_bench.json --wasm "$runtime run --allow-precompiled --dir $here/tools/bench::/bench $here/temp/lua.cwasm /bench/{}.lua" 5
    ;;
*)
    sed -n '2,12p' "$0"
    exit 2
    ;;
esac
