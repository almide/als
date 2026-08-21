#!/usr/bin/env python3
"""REF-KERNEL GATE — the reference evaluator reproduces the λ_almd kernel
corpus byte for byte (ADR-0015 decision 4: the seed oracle, a floor of 1.0).

    scripts/check-ref-kernel.py [--ref <als-ref binary>] [--no-build]

Runs `als-ref run --json` over every program in proofs/kernel-conformance/
(48 generated programs, evaluator-pinned traces — PROVENANCE.toml) and over
spec/wasm_cross/kernel_conformance.almd (the nine kernel-checked lines), and
requires stdout == .expected, exit 0, empty stderr for every one. An abstain
is a failure here: the kernel fragment is the one place where truth, not
agreement, is available, and the evaluator must cover all of it.

Clause 1 (determinism): the whole corpus is run TWICE and the two trace
sets must be byte-identical.

Exit 0 = agreement 1.0 twice; 1 = any disagreement/abstain/fault; 2 = usage.
"""
import glob, json, os, subprocess, sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")


def build(ref_arg):
    if ref_arg:
        return ref_arg
    r = subprocess.run(["cargo", "build", "--release", "--quiet"], cwd=os.path.join(ROOT, "ref"))
    if r.returncode != 0:
        print("::error::ref crate does not build"); sys.exit(1)
    return os.path.join(ROOT, "ref", "target", "release", "als-ref")


def run_one(ref, path):
    r = subprocess.run([ref, "run", path, "--json"], capture_output=True, text=True, timeout=120)
    if r.returncode != 0:
        return {"error": f"als-ref exited {r.returncode}: {r.stderr.strip()[:200]}"}
    try:
        return json.loads(r.stdout)
    except json.JSONDecodeError:
        return {"error": f"malformed protocol output: {r.stdout[:200]!r}"}


def main():
    args = sys.argv[1:]
    ref_arg = None
    if "--ref" in args:
        ref_arg = args[args.index("--ref") + 1]
    ref = build(ref_arg)
    corpus = sorted(glob.glob(os.path.join(ROOT, "proofs/kernel-conformance/gen_*.almd")))
    corpus.append(os.path.join(ROOT, "spec/wasm_cross/kernel_conformance.almd"))
    if len(corpus) < 2:
        print("::error::kernel corpus missing — proofs/kernel-conformance/ is empty"); sys.exit(1)
    errs = []
    traces = []
    for round_no in (1, 2):
        round_traces = []
        for path in corpus:
            rel = os.path.relpath(path, ROOT)
            exp_path = path[:-5] + ".expected" if rel.startswith("proofs/") else os.path.join(ROOT, "proofs/kernel-conformance/kernel_conformance.expected")
            expected = open(exp_path, encoding="utf-8").read()
            out = run_one(ref, path)
            round_traces.append((rel, json.dumps(out, sort_keys=True)))
            if round_no == 2:
                continue
            if "abstain" in out:
                errs.append(f"{rel}: ABSTAIN {out['abstain']['class']} — {out['abstain']['reason']}")
            elif "error" in out:
                errs.append(f"{rel}: evaluator fault — {out['error']}")
            else:
                if out.get("exit") != 0 or out.get("stderr"):
                    errs.append(f"{rel}: exit {out.get('exit')} stderr {out.get('stderr')!r} (expected a clean run)")
                if out.get("stdout") != expected:
                    errs.append(f"{rel}: trace differs\n  expected: {expected!r}\n  got:      {out.get('stdout')!r}")
        traces.append(round_traces)
    if traces[0] != traces[1]:
        for (a, ta), (b, tb) in zip(traces[0], traces[1]):
            if ta != tb:
                errs.append(f"{a}: NON-DETERMINISTIC — two runs differ (clause 1)")
    for e in errs:
        print(f"::error::{e}")
    n = len(corpus)
    bad = len({e.split(':')[0] for e in errs})
    agreement = (n - bad) / n
    if errs:
        print(f"ref-kernel FAILED: agreement {agreement:.3f} over {n} programs (floor 1.0)")
        sys.exit(1)
    print(f"ref-kernel OK: {n}/{n} kernel programs reproduced byte-for-byte, twice (agreement 1.0, deterministic).")


if __name__ == "__main__":
    main()
