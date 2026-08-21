#!/usr/bin/env python3
"""ALS conformance runner — the judge, run against ANY `almide` binary.

    scripts/conformance.py --almide <bin> [--jobs N] [--legs a,b,c]
                           [--limit N] [--report out.toml] [--root DIR]

This repository (almide/als) holds what an Almide implementation is judged
against; it holds no compiler. This script executes the judge-resident
evidence against a candidate binary and writes a CONFORMANCE STATEMENT — a
TOML record of exactly what was run, against which binary, with which verdict.
The implementation under test is a black box: only its CLI is used.

Legs (mirroring the implementation-side gates they were lifted from, so a
verdict here and a verdict in the compiler's own CI mean the same thing):

  suite     `almide test <dir> --target rust` and `--target wasm` over
            spec/lang, spec/stdlib, spec/integration (test blocks pass on each
            target, judged separately — never the CLI's fallback mode).
  cross     spec/wasm_cross/*.almd: `almide build` then EXECUTE on each target
            (native binary; wasm under `wasmtime --dir=/ -S inherit-env=y`) —
            the executions must agree BYTE-FOR-BYTE on (exit code, stdout,
            stderr), trimmed. Build-time diagnostics are not observables. `// @xt-allow: <reason>` marks a known divergence: exempt
            but logged, and flagged STALE once the legs agree again.
            (tests/wasm_runtime_test_parts/p2.rs::wasm_cross_target_spec)
  pkg       spec/wasm_cross_pkg: the package form of `cross` (cwd = the package).
  programs  spec/programs/*.almd: cross-target agreement, no contract header.
  fail      spec/wasm_fail/*.almd: `// @expect-fail: <stderr substring>` — both
            legs must TERMINATE UNSUCCESSFULLY with that substring in stderr and
            break the SAME way; `// @xf-allow:` mirrors @xt-allow.
            (tests/wasm_runtime_test_parts/p6_fail_corpus.rs)
  diag      tests/diagnostics/<case>/: `almide check broken.almd` must report a
            diagnostic carrying meta.toml's expects_code / expects_error /
            hint_substring; `almide check fixed.almd` must pass clean.
            (tests/diagnostic_harness_test.rs)
  ref       spec/wasm_cross + spec/programs against the REFERENCE EVALUATOR
            (ref/, ADR-0015; TOR-8's adjudication): where the reference
            evaluates a program, BOTH targets must equal ITS observables —
            agreement between the two targets is no longer sufficient. A
            reference abstain (a ledgered class, proofs/ref-abstain.toml) is
            counted and skipped, never a pass or a fail; a malformed protocol
            reply or an evaluator fault is red. `// @ref-allow: <reason>`
            tracks a known reference disagreement (a FINDING under
            adjudication, e.g. docs/ref/PARSER-NOTES.md F1) without passing
            it, and goes STALE the moment the legs match the reference again.
            Needs --ref (default: ref/target/release/als-ref).

Exit status: 0 = every leg PASS (no failure, no stale allow); 1 = a failure;
2 = usage / environment error. `--limit N` runs the first N items of each
leg (a smoke run — the statement records the limit so it can never be read
as a full verdict).
"""
import os
import sys

# ── self-coverage hook ───────────────────────────────────────────────────────
# scripts/check-runner-coverage.py runs the self-test with ALS_RUNNER_TRACE_DIR
# set; this records which lines of THIS file executed (stdlib settrace, main
# thread + worker threads) and writes them at exit. Off unless the variable is
# set; the verdict logic is untouched either way.
if os.environ.get("ALS_RUNNER_TRACE_DIR"):
    import atexit as _atexit
    import threading as _threading
    _ME = os.path.abspath(__file__)
    _HIT = set()

    def _trace(frame, event, arg):
        if os.path.abspath(frame.f_code.co_filename) != _ME:
            return None
        if event == "line":
            _HIT.add(frame.f_lineno)
        return _trace

    def _flush():
        out = os.path.join(os.environ["ALS_RUNNER_TRACE_DIR"], f"{os.getpid()}.lines")
        with open(out, "w", encoding="utf-8") as f:
            f.write("\n".join(str(n) for n in sorted(_HIT)))
    sys.settrace(_trace)
    _threading.settrace(_trace)
    _atexit.register(_flush)

import argparse
import json
import concurrent.futures as cf
import datetime as dt
import platform
import re
import subprocess
import tempfile

RUN_TIMEOUT = 300  # seconds per single compile+run; a hang is a failure, not a wait

CROSS_DIR = "spec/wasm_cross"
PKG_DIR = "spec/wasm_cross_pkg"
PROGRAMS_DIR = "spec/programs"
FAIL_DIR = "spec/wasm_fail"
DIAG_DIR = "tests/diagnostics"
SUITE_DIRS = ["spec/lang", "spec/stdlib", "spec/integration"]
ALL_LEGS = ["suite", "cross", "pkg", "programs", "fail", "diag", "ref"]


class Leg:
    def __init__(self, name):
        self.name = name
        self.total = 0
        self.passed = 0
        self.allowed = []   # (item, reason)
        self.stale = []     # (item, reason)
        self.failed = []    # (item, detail)
        self.skipped = []   # (item, reason)
        self.notes = []     # free-form, recorded in the statement

    @property
    def ok(self):
        return not self.failed and not self.stale


def run(cmd, cwd=None, timeout=RUN_TIMEOUT):
    """(exit, stdout, stderr) with the implementation gates' trim semantics.
    exit -1 = timeout, -2 = could not spawn."""
    try:
        p = subprocess.run(cmd, cwd=cwd, capture_output=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        return (-1, "", f"<timeout after {timeout}s>")
    except OSError as e:
        return (-2, "", f"<spawn failed: {e}>")
    return (p.returncode,
            p.stdout.decode("utf-8", "replace").strip(),
            p.stderr.decode("utf-8", "replace").strip())


def header_directive(path, key):
    """The value of a `// @key: value` header line, or None."""
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            s = line.strip()
            if s.startswith("//"):
                body = s[2:].strip()
                if body.startswith(f"@{key}:"):
                    return body[len(key) + 2:].strip()
            elif s:
                # headers precede code; stop at the first non-comment line
                break
    return None


def fmt_leg(tag, leg3):
    c, o, e = leg3
    return f"{tag}: exit={c} stdout={o!r} stderr={e!r}"


# ── legs ────────────────────────────────────────────────────────────────────

def leg_suite(args, root):
    leg = Leg("suite")
    for d in SUITE_DIRS:
        if not os.path.isdir(os.path.join(root, d)):
            leg.skipped.append((d, "directory absent"))
            continue
        # One leg per target, explicitly: the CLI's default is wasm-with-native-
        # fallback, which would let a native-only failure hide behind the fallback.
        for target in ("rust", "wasm"):
            cmd = [args.almide, "test", d, "--target", target]
            label = f"{d} --target {target}"
            leg.total += 1
            code, out, err = run(cmd, cwd=root, timeout=3600)
            combined = out + "\n" + err
            # Summary shapes across versions/targets:
            #   "N via WASM, M via native fallback, K failed (of T files)"
            #   "N passed, K failed (of T files)"
            #   "All T test file(s) passed"
            m = (re.search(r"(\d+) via WASM, (\d+) via native fallback, (?P<failed>\d+) failed \(of (\d+) files\)", combined)
                 or re.search(r"(\d+) passed, (?P<failed>\d+) failed \(of (\d+) files\)", combined)
                 or re.search(r"All (\d+) test file\(s\) passed", combined))
            if m:
                leg.notes.append(f"{label}: {m.group(0)}")
                nfailed = int(m.group("failed")) if "failed" in m.groupdict() and m.group("failed") is not None else 0
            else:
                nfailed = None
            if code == 0 and (nfailed in (0, None)):
                leg.passed += 1
            else:
                tail = "\n".join(combined.splitlines()[-25:])
                leg.failed.append((label, f"exit={code} failed={nfailed}\n{tail}"))
    return leg


def build_and_run(args, src, target, cwd=None):
    """BUILD then EXECUTE, as the implementation gates do (p4_corpus.rs):
    compile-time diagnostics (warnings on the build's stderr) are not the
    program's observable behaviour, so `almide run` would conflate them.
    A build failure is reported as exit -3 with the build's stderr."""
    with tempfile.TemporaryDirectory(prefix="als-") as td:
        out = os.path.join(td, "prog.wasm" if target == "wasm" else "prog")
        cmd = [args.almide, "build", src, "-o", out] + (["--target", "wasm"] if target == "wasm" else [])
        bcode, bout, berr = run(cmd, cwd=cwd)
        if bcode != 0:
            return (-3, "", f"<{target} build failed (exit {bcode})>\n{berr}")
        if target == "wasm":
            return run(["wasmtime", "--dir=/", "-S", "inherit-env=y", out], cwd=cwd)
        return run([out], cwd=cwd)


def cross_item(args, root, rel, cwd=None, allow_key="xt-allow"):
    """Run one program on both targets; classify like wasm_cross_target_spec."""
    path = os.path.join(root, rel)
    src = os.path.basename(rel) if cwd else path
    native = build_and_run(args, src, "rust", cwd=cwd)
    wasm = build_and_run(args, src, "wasm", cwd=cwd)
    allow = header_directive(path, allow_key)
    if native[0] == -2 or wasm[0] == -2:
        return ("failed", rel, fmt_leg("native", native) + "\n  " + fmt_leg("wasm", wasm))
    equal = native == wasm
    if equal and allow is None:
        return ("passed", rel, "")
    if equal and allow is not None:
        return ("stale", rel, f"@{allow_key} now MATCHES (was: {allow}) — remove the directive")
    if not equal and allow is not None:
        return ("allowed", rel, allow)
    return ("failed", rel, "cross-target divergence\n  " + fmt_leg("native", native) + "\n  " + fmt_leg("wasm", wasm))


def fail_item(args, root, rel):
    path = os.path.join(root, rel)
    expect = header_directive(path, "expect-fail")
    if expect is None:
        return ("failed", rel, "missing `// @expect-fail:` header — a wasm_fail fixture must say how it breaks")
    allow = header_directive(path, "xf-allow")
    native = build_and_run(args, path, "rust")
    wasm = build_and_run(args, path, "wasm")
    nc, _, nerr = native
    if not (nc != 0 and expect in nerr):
        return ("failed", rel, f"native did not fail as declared\n  expected: nonzero exit, stderr containing {expect!r}\n  " + fmt_leg("native", native))
    equal = native == wasm
    if equal and allow is None:
        return ("passed", rel, "")
    if equal and allow is not None:
        return ("stale", rel, f"@xf-allow now MATCHES (was: {allow}) — remove the directive")
    if not equal and allow is not None:
        return ("allowed", rel, allow)
    return ("failed", rel, "legs break differently\n  " + fmt_leg("native", native) + "\n  " + fmt_leg("wasm", wasm))


def parse_meta(path):
    meta = {}
    if not os.path.isfile(path):
        return meta
    with open(path, encoding="utf-8") as f:
        for line in f:
            m = re.match(r'^\s*(\w+)\s*=\s*"(.*)"\s*$', line)
            if m:
                meta[m.group(1)] = m.group(2).replace('\\"', '"')
    return meta


def diag_item(args, root, case):
    rel = os.path.join(DIAG_DIR, case)
    d = os.path.join(root, rel)
    broken, fixed = os.path.join(d, "broken.almd"), os.path.join(d, "fixed.almd")
    if not os.path.isfile(broken) or not os.path.isfile(fixed):
        return ("failed", rel, "case must have both broken.almd and fixed.almd")
    meta = parse_meta(os.path.join(d, "meta.toml"))
    code, out, err = run([args.almide, "check", broken], cwd=root)
    combined = out + err
    problems = []
    if code == 0 and "error" not in combined and "warning" not in combined:
        problems.append("broken.almd unexpectedly passed")
    ec = meta.get("expects_code")
    if ec and not (f"[{ec}]" in combined or f"error[{ec}]" in combined):
        problems.append(f"expected code {ec} not found")
    ee = meta.get("expects_error")
    if ee and ee not in combined:
        problems.append(f"expected error substring {ee!r} not found")
    hs = meta.get("hint_substring")
    if hs and hs.lower() not in combined.lower():
        problems.append(f"expected hint substring {hs!r} not found")
    fcode, fout, ferr = run([args.almide, "check", fixed], cwd=root)
    if fcode != 0:
        problems.append(f"fixed.almd does not compile cleanly (exit={fcode})\n  {(fout + ferr)[-400:]!r}")
    if problems:
        return ("failed", rel, "; ".join(problems) + f"\n  broken.almd output: {combined[-600:]!r}")
    return ("passed", rel, "")


def collect(leg, results):
    for status, item, detail in results:
        leg.total += 1
        if status == "passed":
            leg.passed += 1
        elif status == "allowed":
            leg.allowed.append((item, detail))
        elif status == "stale":
            leg.stale.append((item, detail))
        else:
            leg.failed.append((item, detail))


def pmap(fn, items, jobs):
    with cf.ThreadPoolExecutor(max_workers=jobs) as ex:
        return list(ex.map(fn, items))


def leg_cross(args, root):
    leg = Leg("cross")
    files = sorted(f for f in os.listdir(os.path.join(root, CROSS_DIR)) if f.endswith(".almd"))
    files = files[: args.limit] if args.limit else files
    collect(leg, pmap(lambda f: cross_item(args, root, f"{CROSS_DIR}/{f}"), files, args.jobs))
    return leg


def leg_pkg(args, root):
    leg = Leg("pkg")
    d = os.path.join(root, PKG_DIR)
    if os.path.isfile(os.path.join(d, "main.almd")):
        collect(leg, [cross_item(args, root, f"{PKG_DIR}/main.almd", cwd=d)])
    else:
        leg.skipped.append((PKG_DIR, "no main.almd"))
    return leg


def leg_programs(args, root):
    leg = Leg("programs")
    files = sorted(f for f in os.listdir(os.path.join(root, PROGRAMS_DIR)) if f.endswith(".almd"))
    files = files[: args.limit] if args.limit else files
    collect(leg, pmap(lambda f: cross_item(args, root, f"{PROGRAMS_DIR}/{f}"), files, args.jobs))
    return leg


def leg_fail(args, root):
    leg = Leg("fail")
    files = sorted(f for f in os.listdir(os.path.join(root, FAIL_DIR)) if f.endswith(".almd"))
    files = files[: args.limit] if args.limit else files
    collect(leg, pmap(lambda f: fail_item(args, root, f"{FAIL_DIR}/{f}"), files, args.jobs))
    return leg


def ref_item(args, root, rel):
    """One program against the reference evaluator: legs == ref (TOR-8)."""
    path = os.path.join(root, rel)
    code, out, err = run([args.ref, "run", path, "--json"], timeout=300)
    if code != 0:
        return ("failed", rel, f"reference protocol fault: exit {code}\n  {err[:300]}")
    try:
        doc = json.loads(out)
    except json.JSONDecodeError:
        return ("failed", rel, f"reference protocol fault: malformed reply {out[:200]!r}")
    if "error" in doc:
        return ("failed", rel, f"reference evaluator fault: {str(doc['error'])[:300]}")
    if "abstain" in doc:
        a = doc["abstain"]
        return ("abstain", rel, f"{a.get('class', '?')}: {str(a.get('reason', ''))[:160]}")
    ref3 = (doc.get("exit"), str(doc.get("stdout", "")).strip(), str(doc.get("stderr", "")).strip())
    allow = header_directive(path, "ref-allow")
    native = build_and_run(args, path, "rust")
    wasm = build_and_run(args, path, "wasm")
    mismatches = [tag for tag, leg3 in (("native", native), ("wasm", wasm)) if leg3 != ref3]
    if not mismatches and allow is None:
        return ("passed", rel, "")
    if not mismatches and allow is not None:
        return ("stale", rel, f"@ref-allow now MATCHES (was: {allow}) — remove the directive")
    if mismatches and allow is not None:
        return ("allowed", rel, allow)
    return ("failed", rel,
            f"reference disagreement ({', '.join(mismatches)})\n  " + fmt_leg("ref", ref3)
            + "\n  " + fmt_leg("native", native) + "\n  " + fmt_leg("wasm", wasm))


def leg_ref(args, root):
    leg = Leg("ref")
    items = [f"{CROSS_DIR}/{f}" for f in sorted(os.listdir(os.path.join(root, CROSS_DIR))) if f.endswith(".almd")]
    items += [f"{PROGRAMS_DIR}/{f}" for f in sorted(os.listdir(os.path.join(root, PROGRAMS_DIR))) if f.endswith(".almd")]
    items = items[: args.limit] if args.limit else items
    results = pmap(lambda rel: ref_item(args, root, rel), items, args.jobs)
    classes = {}
    plain = []
    for status, item, detail in results:
        if status == "abstain":
            leg.total += 1
            leg.skipped.append((item, f"reference abstain — {detail}"))
            classes[detail.split(":")[0]] = classes.get(detail.split(":")[0], 0) + 1
        else:
            plain.append((status, item, detail))
    collect(leg, plain)
    judged = leg.passed + len(leg.failed) + len(leg.allowed) + len(leg.stale)
    leg.notes.append(f"reference judged {judged}, abstained {len(leg.skipped)} "
                     f"(top classes: {', '.join(f'{k} {v}' for k, v in sorted(classes.items(), key=lambda kv: -kv[1])[:6])})")
    return leg


def leg_diag(args, root):
    leg = Leg("diag")
    cases = sorted(c for c in os.listdir(os.path.join(root, DIAG_DIR))
                   if os.path.isdir(os.path.join(root, DIAG_DIR, c)))
    cases = cases[: args.limit] if args.limit else cases
    collect(leg, pmap(lambda c: diag_item(args, root, c), cases, args.jobs))
    return leg


LEG_FNS = {"suite": leg_suite, "cross": leg_cross, "pkg": leg_pkg,
           "programs": leg_programs, "fail": leg_fail, "diag": leg_diag, "ref": leg_ref}


# ── statement ───────────────────────────────────────────────────────────────

def toml_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n").replace("\t", "\\t") + '"'


def statement(args, root, legs, verdict):
    als_commit = run(["git", "rev-parse", "HEAD"], cwd=root)[1] or "unknown"
    dirty = run(["git", "status", "--porcelain"], cwd=root)[1]
    almide_ver = run([args.almide, "--version"])[1] or "unknown"
    wt = run(["wasmtime", "--version"])
    lines = [
        "# ALS CONFORMANCE STATEMENT — generated by scripts/conformance.py; do not edit.",
        "# A verdict is only as wide as the legs and limit recorded here.",
        "[run]",
        f"als_commit = {toml_str(als_commit + ('+dirty' if dirty else ''))}",
        f"almide = {toml_str(almide_ver)}",
        f"almide_path = {toml_str(os.path.abspath(args.almide))}",
        f"wasmtime = {toml_str(wt[1] if wt[0] == 0 else 'absent')}",
        f"ref = {toml_str(run([args.ref, '--version'])[1] or 'absent')}",
        f"platform = {toml_str(platform.platform())}",
        f"date = {toml_str(dt.datetime.now(dt.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'))}",
        f"legs = [{', '.join(toml_str(l.name) for l in legs)}]",
        f"limit = {args.limit or 0}",
        f"jobs = {args.jobs}",
        f"verdict = {toml_str(verdict)}",
        "",
    ]
    for l in legs:
        lines += ["[[leg]]", f"name = {toml_str(l.name)}", f"total = {l.total}", f"passed = {l.passed}",
                  f"allowed = {len(l.allowed)}", f"stale = {len(l.stale)}", f"failed = {len(l.failed)}",
                  f"skipped = {len(l.skipped)}", f"ok = {'true' if l.ok else 'false'}"]
        for key, rows in (("allowed_items", l.allowed), ("stale_items", l.stale),
                          ("failed_items", l.failed), ("skipped_items", l.skipped), ("notes", [(n, "") for n in l.notes])):
            if rows:
                lines.append(f"{key} = [")
                for item, detail in rows:
                    lines.append("  " + toml_str(f"{item}: {detail}" if detail else item) + ",")
                lines.append("]")
        lines.append("")
    return "\n".join(lines)


def summary_md(legs, verdict, args):
    out = [f"## ALS conformance — **{verdict}**" + (f" (smoke: --limit {args.limit})" if args.limit else ""), "",
           "| leg | total | passed | allowed | stale | failed | skipped |", "|---|--:|--:|--:|--:|--:|--:|"]
    for l in legs:
        out.append(f"| {l.name} | {l.total} | {l.passed} | {len(l.allowed)} | {len(l.stale)} | {len(l.failed)} | {len(l.skipped)} |")
    for l in legs:
        for item, detail in l.failed[:20]:
            out += ["", f"**FAIL {l.name}** `{item}`", "```", detail[:1500], "```"]
        for item, detail in l.stale:
            out += ["", f"**STALE {l.name}** `{item}`: {detail}"]
    return "\n".join(out)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--almide", required=True, help="path to the almide binary under test")
    ap.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 2) // 2))
    ap.add_argument("--legs", default=",".join(ALL_LEGS), help=f"comma list of {ALL_LEGS}")
    ap.add_argument("--limit", type=int, default=0, help="first N items per leg (smoke run; recorded)")
    ap.add_argument("--ref", default=None,
                    help="path to the reference evaluator (als-ref); default ref/target/release/als-ref under --root")
    ap.add_argument("--report", help="write the conformance statement (TOML) here")
    ap.add_argument("--root", default=os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
    args = ap.parse_args()
    root = os.path.abspath(args.root)
    os.chdir(root)

    legs_wanted = [l for l in args.legs.split(",") if l]
    bad = [l for l in legs_wanted if l not in LEG_FNS]
    if bad:
        print(f"unknown leg(s): {bad}; choose from {ALL_LEGS}", file=sys.stderr)
        return 2
    if run([args.almide, "--version"])[0] != 0:
        print(f"cannot execute {args.almide} --version", file=sys.stderr)
        return 2
    if any(l in legs_wanted for l in ("suite", "cross", "pkg", "programs", "fail", "ref")) and run(["wasmtime", "--version"])[0] != 0:
        print("wasmtime not on PATH — the wasm leg cannot run (install it, or restrict --legs to diag)", file=sys.stderr)
        return 2
    if args.ref is None:
        args.ref = os.path.join(root, "ref", "target", "release", "als-ref")
    if "ref" in legs_wanted and run([args.ref, "--version"])[0] != 0:
        print(f"cannot execute {args.ref} --version — build the reference evaluator (cd ref && cargo build --release) or pass --ref", file=sys.stderr)
        return 2

    legs = []
    for name in legs_wanted:
        print(f"[{name}] running …", flush=True)
        leg = LEG_FNS[name](args, root)
        legs.append(leg)
        print(f"[{name}] total={leg.total} passed={leg.passed} allowed={len(leg.allowed)} "
              f"stale={len(leg.stale)} failed={len(leg.failed)} skipped={len(leg.skipped)}", flush=True)
        for item, detail in leg.allowed:
            print(f"  ~ tracked: {item}: {detail}")
        for item, detail in leg.stale:
            print(f"  ! stale:   {item}: {detail}")
        for item, detail in leg.failed:
            print(f"  x FAIL:    {item}: {detail}")
        for n in leg.notes:
            print(f"  · {n}")

    verdict = "PASS" if all(l.ok for l in legs) else "FAIL"
    print(f"\nALS conformance: {verdict}" + (f" (smoke --limit {args.limit})" if args.limit else ""))
    if args.report:
        with open(args.report, "w", encoding="utf-8") as f:
            f.write(statement(args, root, legs, verdict))
        print(f"statement written: {args.report}")
    gh = os.environ.get("GITHUB_STEP_SUMMARY")
    if gh:
        with open(gh, "a", encoding="utf-8") as f:
            f.write(summary_md(legs, verdict, args) + "\n")
    return 0 if verdict == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
