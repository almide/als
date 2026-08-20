#!/usr/bin/env python3
"""Runner structural coverage — how much of the judge's verdict code its self-test exercises.

    scripts/check-runner-coverage.py            # gate: measure, compare with the recorded floor
    scripts/check-runner-coverage.py --write    # record the measured value as the floor

DO-330 asks of a verification tool not only that it can fail correctly
(proofs/gate-verification.toml) but how much of it the verification of the
tool actually exercised. `scripts/selftest-conformance.py` drives the real
runner over stub processes through every verdict class; this instrument runs
that self-test with `ALS_RUNNER_TRACE_DIR` set, collects the lines of
`scripts/conformance.py` that executed (the runner's own stdlib `settrace`
hook, main thread and worker threads), and divides by the executable lines
(every line reachable inside the module's functions and classes; module-level
statements run in the frame that installs the hook and are excluded, counted).
LINE coverage, not branch — stated here so it cannot be read as more; no third-party module, so
it runs where the gates run (no network).

`proofs/runner-coverage.toml` holds the floor. Four directions: a measured
value below the floor is a regression; above it, the floor must be ratcheted
up (the recorded figure is exact, not "at least"); a red self-test makes the
measurement meaningless (error); zero executable lines is a broken instrument.
The uncovered lines are printed so the burn-down has a list, not a number.
"""
import argparse, os, re, subprocess, sys, tempfile

RUNNER = "scripts/conformance.py"
SELFTEST = "scripts/selftest-conformance.py"
LEDGER = "proofs/runner-coverage.toml"

def executable_lines(path):
    """Every line reachable inside the module's functions, classes, lambdas and
    comprehensions. MODULE-LEVEL statements (imports, constants, the hook, the
    `if __name__` tail) are excluded and counted separately: `settrace` only
    sees frames created after it is installed, so the already-running module
    frame is structurally untraceable — reported as excluded, not silently
    counted against the runner. The hook's own two functions are excluded too
    (Python disables tracing inside a trace function)."""
    code = compile(open(path, encoding="utf-8").read(), path, "exec")
    top = {ln for _, _, ln in code.co_lines() if ln is not None}
    lines, stack = set(), [k for k in code.co_consts if hasattr(k, "co_code")]
    while stack:
        c = stack.pop()
        if c.co_name in ("_trace", "_flush"):   # the hook itself: tracing is off inside a trace function
            continue
        for _, _, ln in c.co_lines():
            if ln is not None:
                lines.add(ln)
        stack.extend(k for k in c.co_consts if hasattr(k, "co_code"))
    return lines - top, len(top - lines)

def measure():
    with tempfile.TemporaryDirectory() as td:
        env = dict(os.environ, ALS_RUNNER_TRACE_DIR=td)
        env.pop("GITHUB_STEP_SUMMARY", None)   # the same lines on a laptop and in CI
        r = subprocess.run([sys.executable, SELFTEST], env=env, capture_output=True, text=True, timeout=900)
        if r.returncode != 0:
            print("::error::the runner self-test is red — coverage of a failing verification is not evidence")
            print(r.stdout[-2000:] + r.stderr[-2000:])
            sys.exit(1)
        hit = set()
        for fn in os.listdir(td):
            if fn.endswith(".lines"):
                hit.update(int(l) for l in open(os.path.join(td, fn), encoding="utf-8").read().split())
    exe, toplevel = executable_lines(RUNNER)
    if not exe:
        print("::error::zero executable lines found in the runner — a broken instrument is not a pass"); sys.exit(1)
    covered = hit & exe
    pct = round(100.0 * len(covered) / len(exe), 1)
    return pct, sorted(exe - covered), len(exe), toplevel

def ranges(ns):
    out, start, prev = [], None, None
    for n in ns:
        if start is None:
            start = prev = n
        elif n == prev + 1:
            prev = n
        else:
            out.append(f"{start}" if start == prev else f"{start}-{prev}"); start = prev = n
    if start is not None:
        out.append(f"{start}" if start == prev else f"{start}-{prev}")
    return out

def read_floor():
    try:
        src = open(LEDGER, encoding="utf-8").read()
    except FileNotFoundError:
        return None
    m = re.search(r'^#\s*line_floor\s*=\s*"([\d.]+)"', src, re.M)
    return float(m.group(1)) if m else None

HEADER = '''# RUNNER COVERAGE LEDGER (almide/als) — how much of scripts/conformance.py the
# self-test exercises. Measured by scripts/check-runner-coverage.py: the 21
# selftest scenarios run with ALS_RUNNER_TRACE_DIR set, the runner's stdlib
# settrace hook records executed lines, divided by the executable lines of the
# compiled module. LINE coverage (not branch, not MC/DC — said plainly).
# The floor is EXACT: below it is a regression, above it the floor must be
# ratcheted up (--write records the measured value).
#
# line_floor = "{pct}"
# executable_lines = "{n}"
# module_level_lines_excluded = "{top}"
# uncovered = "{unc}"
'''

def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--write", action="store_true", help="record the measured value as the floor")
    a = ap.parse_args()
    os.chdir(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
    pct, uncovered, n, toplevel = measure()
    unc = " ".join(ranges(uncovered))
    if a.write:
        with open(LEDGER, "w", encoding="utf-8") as f:
            f.write(HEADER.format(pct=pct, n=n, unc=unc, top=toplevel))
        print(f"wrote {LEDGER}: line coverage {pct}% of {n} executable lines ({toplevel} module-level lines excluded); uncovered: {unc}")
        return 0
    floor = read_floor()
    if floor is None:
        print(f"::error::{LEDGER} missing or without line_floor — run --write"); return 1
    if pct < floor:
        print(f"::error::runner line coverage {pct}% is BELOW the floor {floor}% — the self-test exercises less of the runner than it did; "
              f"add the scenario that covers the new verdict path (uncovered: {unc})"); return 1
    if pct > floor:
        print(f"::error::runner line coverage {pct}% is ABOVE the floor {floor}% — ratchet it up (--write)"); return 1
    print(f"runner-coverage OK: line {pct}% of {n} executable lines (floor {floor}%, exact; line, not branch; "
          f"{toplevel} module-level lines excluded). uncovered: {unc}")
    return 0

if __name__ == "__main__":
    sys.exit(main())
