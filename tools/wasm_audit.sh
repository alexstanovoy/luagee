#!/usr/bin/env bash
# The library asks the host for nothing a wasm host may lack, and nothing that would make it nondeterministic: no
# clock, environment, threads, process, network or host randomness (std::time, std::env, std::thread, ...), and no
# files outside the `fs` feature's folder loader (src/loader/folder.rs). Every std path the library names must be on
# the list below, which grows only by a reviewed edit. Fails naming each line that breaks the rule.
#
#   tools/wasm_audit.sh
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$here/crates/luagee/src"
allowed='any|f64|borrow|boxed|cell|char|cmp|collections|convert|error|fmt|hash|iter|marker|mem|num|ops|option|rc|result|slice|str|string|vec'
files_only='fs|io|path'
bad=0
report() { echo "wasm_audit: $1"; bad=1; }
# Test modules (`tests.rs`) are no part of the library.
while IFS= read -r file; do
    extra=
    [ "$file" = loader/folder.rs ] && extra="|$files_only"
    while IFS= read -r hit; do
        report "$file:$hit: std::${hit##*std::} is outside what the library may use"
    done < <(grep -noE '\b(std|core|alloc)::[a-z_0-9]+' "$file" | grep -vE ":(std|core|alloc)::($allowed$extra)$" || true)
    while IFS= read -r hit; do
        report "$file:${hit%%:*}: a grouped std import hides its paths from this audit: name each path"
    done < <(grep -nE '\b(std|core|alloc)::\{' "$file" || true)
    while IFS= read -r hit; do
        report "$file:${hit%%:*}: ${hit#*:} (a host clock, host randomness, a seeded-by-the-OS hash map, or thread state)"
    done < <(grep -noE '\b(HashMap|HashSet|RandomState|DefaultHasher|Instant|SystemTime|thread_local|getrandom)\b' "$file" || true)
done < <(git ls-files --cached --others --exclude-standard '*.rs' | grep -v '/tests\.rs$\|^tests\.rs$')
[ "$bad" = 0 ] && echo "wasm_audit: the library uses no host clock, environment, threads or randomness"
exit "$bad"
