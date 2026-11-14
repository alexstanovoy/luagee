# Developing luagee

How to check, fuzz, benchmark and wasm-test the crate, and what each script does.
For what the crate promises see [guarantees.md](guarantees.md).

`tools/precheck.sh` is the quick loop (clippy, tests); `tools/gate.sh` is the merge gate (adds the wasm32 build and
its audit, a fuzz smoke, the corpora under wasm, `cargo deny`, fmt). `fuzz/fuzz.sh` builds and runs the
coverage-guided fuzz targets on stable Rust; `tools/bench/bench.py` times luagee against reference interpreters.

`tools/wasm.sh corpus` runs the conformance, hostile and fuzz corpora and the borrowed suites under wasmtime for
`wasm32-wasip1`, on one thread and less stack than wasm's defaults (`examples/corpus.rs`, which checks what
`tests/corpus.rs` checks, for any folder of scripts of a kind, and runs `tests/suites.rs` without its threads). `tools/wasm.sh size` reports the size of a minimal embedding
(`examples/embed.rs`), `tools/wasm.sh bench` the speed under wasmtime against native. Its tools (wasmtime,
binaryen's `wasm-opt`) are pinned by version and sha256 and downloaded into `temp/`; the Rust targets come from
rustup.
