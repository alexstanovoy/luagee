#!/usr/bin/env python3
"""Compare release interpreters on checked workloads, interleaved, with raw JSON evidence.

python3 tools/bench/bench.py --output temp/samples.json --candidate before=PATH --source before=REPO \
    --candidate after=PATH --source after=REPO

Without candidates, use target/release/examples/lua and optional PUC_LUA/LUAJIT references.
The positional run count and --wasm COMMAND remain available to tools/wasm.sh.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shlex
import selectors
import statistics
import stat
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
OUTPUT_LIMIT = 1 << 20


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def source_provenance(root):
    def git(*arguments):
        return subprocess.check_output(["git", "-C", str(root), *arguments], timeout=10)
    untracked = {}
    total = 0
    paths = git("ls-files", "--others", "--exclude-standard", "-z").split(b"\0")
    if len(paths) > 10001:
        raise ValueError("source provenance exceeds 10000 untracked files")
    for name in filter(None, paths):
        relative = os.fsdecode(name)
        path = root / relative
        metadata = path.lstat()
        if stat.S_ISLNK(metadata.st_mode):
            data = os.fsencode(os.readlink(path))
            kind = "symlink"
        elif stat.S_ISREG(metadata.st_mode):
            total += metadata.st_size
            if total > 128 << 20:
                raise ValueError("source provenance exceeds 128 MiB of untracked content")
            data = path.read_bytes()
            kind = "file"
        else:
            raise ValueError(f"source provenance requires a regular file or symlink: {relative}")
        untracked[relative] = {"kind": kind, "sha256": sha256(data)}
    return {"revision": git("rev-parse", "HEAD").decode().strip(),
            "tree": git("rev-parse", "HEAD^{tree}").decode().strip(),
            "status": git("status", "--porcelain", "--untracked-files=all").decode(),
            "diff_sha256": sha256(git("diff", "--binary", "HEAD")), "untracked": untracked}


def assignments(values):
    found = {}
    for value in values:
        name, separator, path = value.partition("=")
        if not separator or not name or name in found:
            raise ValueError(f"expected a unique NAME=PATH, got {value!r}")
        found[name] = Path(path).resolve()
    return found


def interpreters(args):
    sources = assignments(args.source)
    candidates = assignments(args.candidate)
    if candidates:
        if sources.keys() != candidates.keys():
            raise ValueError("each candidate needs one --source NAME=REPO")
        commands = {name: [str(path)] for name, path in candidates.items()}
    else:
        commands = {}
        if os.environ.get("PUC_LUA"):
            commands["puc"] = [os.environ["PUC_LUA"]]
        if os.environ.get("LUAJIT"):
            commands["luajit_joff"] = [os.environ["LUAJIT"], "-joff"]
        commands["luagee"] = [str(ROOT / "target/release/examples/lua")]
        sources["luagee"] = ROOT
    if args.wasm:
        commands["luagee_wasm"] = shlex.split(args.wasm)
        sources["luagee_wasm"] = ROOT
    provenance = {}
    for name, command in commands.items():
        executable = Path(command[0])
        if not executable.is_file():
            import shutil
            resolved = shutil.which(command[0])
            if not resolved:
                raise ValueError(f"executable not found: {command[0]}")
            executable = Path(resolved)
        provenance[name] = {"command": command, "binary_sha256": sha256(executable.read_bytes()),
                            "source": source_provenance(sources[name]) if name in sources else None}
    return commands, provenance


def run(command, script, kernel, expected, timeout):
    relative = Path(kernel).with_suffix("").as_posix()
    command = [part.replace("{}", relative) for part in command] if any("{}" in p for p in command) else [*command, str(script)]
    environment = dict(os.environ, LUA_FUEL=str(1 << 40), LUA_MEMORY=str(512 << 20))
    started = time.perf_counter()
    with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          env=environment, stdin=subprocess.DEVNULL) as process:
        try:
            actual, diagnostic = read_output(process, started + timeout)
            status = process.wait(timeout=max(0.001, started + timeout - time.perf_counter()))
        except BaseException:
            process.kill()
            process.wait()
            raise
    elapsed = time.perf_counter() - started
    if status != 0 or diagnostic:
        raise ValueError(f"process failed ({status}): {diagnostic[:400]!r}")
    if actual != expected:
        raise ValueError(f"output mismatch: expected {expected[:160]!r}, got {actual[:160]!r}")
    return elapsed


def read_output(process, deadline):
    buffers = [bytearray(), bytearray()]
    with selectors.DefaultSelector() as selector:
        selector.register(process.stdout, selectors.EVENT_READ, 0)
        selector.register(process.stderr, selectors.EVENT_READ, 1)
        while selector.get_map():
            remaining = deadline - time.perf_counter()
            if remaining <= 0:
                raise ValueError("process deadline exceeded")
            for key, _ in selector.select(remaining):
                data = os.read(key.fd, 65536)
                if not data:
                    selector.unregister(key.fileobj)
                    continue
                buffer = buffers[key.data]
                if len(buffer) + len(data) > OUTPUT_LIMIT:
                    raise ValueError("output exceeds limit")
                buffer.extend(data)
    return tuple(bytes(buffer) for buffer in buffers)


def summarize(samples, kernels, names):
    summaries = {}
    for kernel in kernels:
        summaries[kernel] = {}
        for name in names:
            values = [s["seconds"] for s in samples if s["kernel"] == kernel and s["interpreter"] == name and not s["warmup"]]
            summaries[kernel][name] = {"runs": len(values), "median_seconds": statistics.median(values),
                                       "min_seconds": min(values), "max_seconds": max(values)}
    return summaries


def measure(args):
    commands, provenance = interpreters(args)
    scripts = sorted(args.kernels.resolve().rglob("*.lua"))
    if not scripts:
        raise ValueError("no kernels found")
    kernels, expected = {}, {}
    for script in scripts:
        name = script.relative_to(args.kernels.resolve()).as_posix()
        expected[name] = script.with_suffix(".out").read_bytes()
        kernels[name] = {"sha256": sha256(script.read_bytes()), "stdout_sha256": sha256(expected[name])}
    samples = []
    names = list(commands)
    for round_number in range(args.warmups + args.runs):
        order = names if round_number % 2 == 0 else names[::-1]
        for script, kernel in zip(scripts, kernels):
            for name in order:
                try:
                    elapsed = run(commands[name], script, kernel, expected[kernel], args.timeout)
                except (ValueError, subprocess.TimeoutExpired) as error:
                    raise ValueError(f"{kernel}: {name}: {error}") from error
                samples.append({"round": round_number, "warmup": round_number < args.warmups,
                                "kernel": kernel, "interpreter": name, "seconds": elapsed,
                                "output_checked": True})
    return {"status": "passed", "compiler": subprocess.check_output(["rustc", "-vV"], text=True, timeout=10),
            "machine": {"system": platform.system(), "release": platform.release(), "architecture": platform.machine(),
                        "cpu_affinity": sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else None},
            "settings": {"fuel": 1 << 40, "memory_bytes": 512 << 20, "runs": args.runs, "warmups": args.warmups},
            "interpreters": provenance, "kernels": kernels, "samples": samples,
            "summaries": summarize(samples, kernels, names)}


def write_report(path, report):
    if path is None:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, name = tempfile.mkstemp(prefix=".samples_", dir=path.parent)
    try:
        with os.fdopen(descriptor, "w") as output:
            json.dump(report, output, indent=2)
            output.write("\n")
        os.replace(name, path)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("count", nargs="?", type=int)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--warmups", type=int, default=1)
    parser.add_argument("--timeout", type=float, default=60)
    parser.add_argument("--kernels", type=Path, default=HERE)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--candidate", action="append", default=[])
    parser.add_argument("--source", action="append", default=[])
    parser.add_argument("--wasm")
    args = parser.parse_args()
    args.runs = args.count if args.count is not None else args.runs
    if not 1 <= args.runs <= 100 or not 0 <= args.warmups <= 20 or not 0 < args.timeout <= 300:
        parser.error("runs must be 1..100, warmups 0..20, timeout (0, 300]")
    try:
        report = measure(args)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        write_report(args.output, {"status": "failed", "error": str(error)})
        parser.exit(1, f"bench: {error}\n")
    write_report(args.output, report)
    for kernel, candidates in report["summaries"].items():
        print(kernel, " | ".join(f"{name}: {s['median_seconds']:.6f} [{s['min_seconds']:.6f}, {s['max_seconds']:.6f}]" for name, s in candidates.items()))


if __name__ == "__main__":
    main()
