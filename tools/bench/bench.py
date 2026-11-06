#!/usr/bin/env python3
"""Times luagee against reference Lua interpreters on the kernels in this folder, interleaved, median of N runs.

    python3 -I tools/bench/bench.py [N]

luagee is `target/release/examples/lua` (build it with `cargo build --release --example lua`); the references come
from $PUC_LUA and $LUAJIT when set (LuaJIT also runs with -joff, its interpreter alone). Prints seconds and the ratio
to the first reference. Python 3 standard library only.
"""
import os
import statistics
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
KERNELS = sorted(p.stem for p in HERE.glob("*.lua"))


def interpreters():
    found = {}
    if os.environ.get("PUC_LUA"):
        found["puc"] = [os.environ["PUC_LUA"]]
    if os.environ.get("LUAJIT"):
        found["luajit-joff"] = [os.environ["LUAJIT"], "-joff"]
    found["luagee"] = [str(ROOT / "target/release/examples/lua")]
    return found


def main(runs):
    vms = interpreters()
    times = {}
    for _ in range(runs):
        for kernel in KERNELS:
            for name, command in vms.items():
                started = time.perf_counter()
                subprocess.run([*command, str(HERE / f"{kernel}.lua")], check=True, capture_output=True)
                times.setdefault((name, kernel), []).append(time.perf_counter() - started)
    names = list(vms)
    print("kernel  " + "".join(f"{n:>14}" for n in names))
    for kernel in KERNELS:
        base = statistics.median(times[(names[0], kernel)])
        cells = []
        for n in names:
            t = statistics.median(times[(n, kernel)])
            cells.append(f"{t:9.3f} x{t / base:4.2f}")
        print(f"{kernel:7} " + "".join(f"{c:>14}" for c in cells))


if __name__ == "__main__":
    main(int(sys.argv[1]) if len(sys.argv) > 1 else 5)
