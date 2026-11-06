#!/usr/bin/env python3
"""Differential testing of a Lua interpreter against LuaJIT (the oracle).

Run with `python3 -I tools/lua_diff/lua_diff.py ...`; Python 3 standard library only.

  record DIR              run every DIR/*.lua under LuaJIT and write its stdout to DIR/NAME.out
  diff DIR CANDIDATE...   run `CANDIDATE... FILE.lua` and LuaJIT on every DIR/*.lua; compare stdout and exit status
  gen N SEED OUTDIR       write N random programs OUTDIR/gen_SEED_I.lua (arithmetic, comparison, concatenation,
                          tostring/tonumber, string.format, % and math.floor/fmod over awkward numbers and strings)

Every script runs with an empty temporary directory as its working directory. $LUAJIT overrides the oracle's path.
A script that exits non-zero or writes to stderr under `record` is an error, so the corpus stays clean.
"""

import os
import random
import subprocess
import sys
import tempfile
from pathlib import Path

LUAJIT = os.environ.get("LUAJIT", "luajit")
TIMEOUT_SECONDS = 30
MAX_OUTPUT_BYTES = 64 << 20


def run(command, script):
    """Returns (exit status or 'timeout', stdout bytes, stderr bytes)."""
    with tempfile.TemporaryDirectory() as scratch:
        try:
            done = subprocess.run(
                [*command, str(script.resolve())],
                cwd=scratch,
                stdin=subprocess.DEVNULL,
                capture_output=True,
                timeout=TIMEOUT_SECONDS,
            )
        except subprocess.TimeoutExpired:
            return "timeout", b"", b""
    return done.returncode, done.stdout[:MAX_OUTPUT_BYTES], done.stderr[:MAX_OUTPUT_BYTES]


def scripts(directory):
    found = sorted(Path(directory).glob("*.lua"))
    if not found:
        sys.exit(f"no *.lua files in {directory}")
    return found


def record(directory):
    failures = 0
    for script in scripts(directory):
        status, out, err = run([LUAJIT], script)
        if status != 0 or err:
            failures += 1
            print(f"FAIL {script.name}: status {status}, stderr {err[:300]!r}")
            continue
        script.with_suffix(".out").write_bytes(out)
    print(f"recorded {len(scripts(directory)) - failures}, failed {failures}")
    return 1 if failures else 0


def first_difference(expected, actual):
    expected_lines = expected.split(b"\n")
    actual_lines = actual.split(b"\n")
    for number, (left, right) in enumerate(zip(expected_lines, actual_lines), 1):
        if left != right:
            return f"line {number}: oracle {left[:120]!r} candidate {right[:120]!r}"
    return f"line {min(len(expected_lines), len(actual_lines)) + 1}: lengths differ ({len(expected_lines)} vs {len(actual_lines)} lines)"


def diff(directory, candidate):
    mismatches = 0
    for script in scripts(directory):
        oracle_status, oracle_out, _ = run([LUAJIT], script)
        status, out, err = run(candidate, script)
        if (status, out) == (oracle_status, oracle_out):
            continue
        mismatches += 1
        what = first_difference(oracle_out, out) if out != oracle_out else ""
        print(f"MISMATCH {script.name}: status oracle {oracle_status} candidate {status}; {what}")
        if err:
            print(f"    candidate stderr: {err[:200].decode(errors='replace').strip()!r}")
    print(f"{mismatches} mismatches of {len(scripts(directory))} scripts")
    return 1 if mismatches else 0


# Numbers where interpreters disagree: signed zero, infinities, NaN, extremes, the 2^53 integer boundary, and values
# whose shortest decimal form needs 17 digits.
NUMBERS = [
    "0", "1", "-1", "2", "3", "7", "10", "-0", "(-0)", "0.5", "-0.5", "1.5", "-1.5", "2.5", "0.1", "0.2", "0.3",
    "1/3", "100", "255", "256", "65535", "65536", "2^31", "2^32", "-2^31", "2^31-1", "2^52", "2^53", "2^53+1",
    "-2^53", "2^63", "2^64", "-2^63", "1e15", "1e16", "1e17", "1e100", "1e308", "1.7976931348623157e308",
    "5e-324", "1e-310", "1e-5", "123456789012", "0.1+0.2", "1/0", "-1/0", "0/0", "math.huge", "-math.huge",
    "math.pi", "4294967296.5", "3.14159265358979", "1e21", "1e-7", "12345.678", "99999999999999.9",
]
STRINGS = [
    '"0"', '"10"', '" 10 "', '"1e2"', '"0x10"', '"0x1p4"', '"1.5"', '".5"', '"5."', '"-3"', '"+3"', '"abc"', '""',
    '"1 2"', '"inf"', '"nan"', '"0x"', '"1e"', '"  "', '"3.0"', '"-0"', '"1e400"', '"9007199254740993"',
    '"0.1"', '"1_0"',
]
BINARY = ["+", "-", "*", "/", "%", "^", "..", "<", "<=", ">", ">=", "==", "~="]
FORMATS = [
    "%d", "%5d", "%-5d|", "%05d", "%+d", "% d", "%i", "%x", "%X", "%#x", "%o", "%c", "%e", "%E", "%.3e", "%f",
    "%.0f", "%.3f", "%10.2f", "%-10.2f|", "%+.1f", "%g", "%G", "%.3g", "%.10g", "%.14g", "%.17g", "%10g", "%#g",
    "%s", "%10s", "%-10s|", "%.3s", "%q", "%5.1s|", "%u",
]


def operand(rng):
    return rng.choice(NUMBERS if rng.random() < 0.7 else STRINGS)


def check(rng):
    kind = rng.randrange(9)
    if kind <= 3:
        return f"({operand(rng)}) {rng.choice(BINARY)} ({operand(rng)})"
    if kind == 4:
        return f"tostring({operand(rng)})"
    if kind == 5:
        return f"tonumber(tostring({operand(rng)}))"
    if kind == 6:
        return f"string.format('{rng.choice(FORMATS)}', {operand(rng)})"
    if kind == 7:
        function = rng.choice(["math.floor", "math.ceil", "math.abs", "math.sqrt", "math.fmod", "math.modf"])
        arguments = operand(rng) + (", " + operand(rng) if function == "math.fmod" else "")
        return f"{function}({arguments})"
    return f"-({operand(rng)}), #tostring({operand(rng)}), ({operand(rng)}) % ({operand(rng)})"


def generate(count, seed, directory):
    Path(directory).mkdir(parents=True, exist_ok=True)
    rng = random.Random(seed)
    for index in range(count):
        lines = [f"-- generated by lua_diff.py gen: seed {seed}, program {index}"]
        # The text of an error differs between interpreters, so only the fact of it is printed.
        lines.append("local function show(ok, ...) if not ok then print('error') return end local t = {} for i = 1, select('#', ...) do t[i] = tostring((select(i, ...))) end print(table.concat(t, ' ')) end")
        for _ in range(300):
            lines.append(f"show(pcall(function() return {check(rng)} end))")
        (Path(directory) / f"gen_{seed}_{index}.lua").write_text("\n".join(lines) + "\n")


def main(arguments):
    if len(arguments) == 2 and arguments[0] == "record":
        return record(arguments[1])
    if len(arguments) >= 3 and arguments[0] == "diff":
        return diff(arguments[1], arguments[2:])
    if len(arguments) == 4 and arguments[0] == "gen":
        generate(int(arguments[1]), int(arguments[2]), arguments[3])
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
