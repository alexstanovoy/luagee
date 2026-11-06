#!/usr/bin/env bash
# luagee under WebAssembly (wasm32-wasip1, run by a pinned wasmtime): the corpora.
#
#   tools/wasm.sh corpus   build examples/corpus.rs for wasm32-wasip1 and run it under wasmtime on the conformance and
#                          hostile corpora, on one thread and less stack than wasm's defaults, then on every
#                          fuzz/corpus/* folder (each input run as a script): what the merge gate runs
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

need_target() {
    rustup target list --installed | grep -qx "$target" || rustup target add "$target"
}

case "${1:-}" in
corpus)
    need_target
    runtime=$(wasmtime)
    # On less stack than wasm's defaults, so that a margin is checked, not hoped for: the module's own (linear
    # memory) stack is half rustc's default of 1 MiB, wasmtime's stack three quarters of its default of 512 KiB.
    # tests/hostile/deepest_native_nesting_coroutines.lua takes the most of both a script can (some 340 KiB each).
    cargo rustc --profile test-release -p luagee --example corpus --target "$target" -- \
        -C link-arg=-zstack-size=$((512 << 10))
    runner=target/$target/test-release/examples/corpus.wasm
    # The folders are the guest's only files. timeout is a hang guard; fuel ends every script long before it.
    run() { "$runtime" run -W max-wasm-stack=$((384 << 10)) -W timeout=600s --dir "$here::/repo" "$runner" "$@"; }
    run conformance /repo/crates/luagee/tests/conformance
    run hostile /repo/crates/luagee/tests/hostile
    fuzzed=0
    for corpus in fuzz/corpus/*/; do
        if [ -d "$corpus" ]; then
            run fuzz "/repo/${corpus%/}"
            fuzzed=1
        fi
    done
    [ "$fuzzed" = 1 ] || echo "wasm.sh: no fuzz corpus yet (fuzz/fuzz.sh smoke seeds one); the gate runs that first"
    ;;
*)
    sed -n '2,12p' "$0"
    exit 2
    ;;
esac
