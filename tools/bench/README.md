# Comparing release builds

Build each revision's `lua` example with `cargo build --release --example lua`.
Run comparisons while the machine is otherwise quiet, on the same CPUs. This tool
does not reserve CPUs or stop other work; use your host's resource scheduler.

```sh
python3 tools/bench/bench.py --runs 5 --warmups 1 --output temp/samples.json \
  --candidate before=/path/to/before/target/release/examples/lua --source before=/path/to/before \
  --candidate after=/path/to/after/target/release/examples/lua --source after=/path/to/after
```

Each round reverses interpreter order. Every run must finish without diagnostics
and print the exact adjacent `.out` file; the imported kernels print their computed
checksum. A failed run atomically publishes a failed status, replacing any stale successful report. The JSON records
all warmup and measured samples, medians and minimum/maximum, source revisions and
committed tree hashes, dirty status/diff hashes and untracked content hashes, executable/kernel/output hashes, compiler version, machine
and CPU affinity. Keep the JSON beside the build logs; a compiler version identifies
the harness host, while executable hashes and separate build logs identify the builds.
Only consume reports whose status is passed. Format and commit each source tree before building and measuring; preserve build logs and check its tree hash in the report.

Fuel and memory are raised for throughput measurements (2^40 and 512 MiB);
accounting remains enabled. These are not default-limit latency measurements.
Each process has a deadline. Only run trusted, reviewed workloads and executables;
the tool is not a process sandbox. `--kernels DIRECTORY` selects a smaller set.

Without candidates, the runner uses this checkout's `target/release/examples/lua`
and optional `PUC_LUA` and `LUAJIT` references. The latter runs with `-joff`.
`--wasm COMMAND` and a positional run count support `tools/wasm.sh`.

The `dellingr` folder has its own complete MIT notice and per-file provenance.
Its queue repetition count is reduced to keep repeated measurements short.
