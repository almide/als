#!/usr/bin/env python3
"""SELF-TEST of the conformance runner — the judge's own DO-330 evidence.

`scripts/conformance.py` is a verification tool: its verdict justifies
trusting an implementation. A verification tool that has never been seen to
fail is possibly decorative, so this harness proves, per leg and per verdict
class, that the runner turns RED for the right reasons and GREEN only for
the right ones. It builds a miniature judge tree and a scripted stub
`almide` (+ stub `wasmtime`) whose behaviour per (subcommand, file, target)
is data, then drives the real runner through every scenario below and
asserts BOTH the process exit code and the per-leg counters parsed from the
machine-readable conformance statement the runner wrote.

Every scenario is a deliberate mutation of implementation behaviour; a
scenario that stops mattering (because the runner stopped checking it) fails
here, which is the point.
"""
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

RUNNER = os.path.join(os.path.dirname(os.path.abspath(__file__)), "conformance.py")

STUB_ALMIDE = r'''#!/usr/bin/env python3
import json, os, sys
S = json.load(open(os.environ["ALS_STUB_SCENARIO"]))
argv = sys.argv[1:]
if argv[:1] == ["--version"]:
    print("almide 0.0.0-stub"); sys.exit(0)
cmd = argv[0]
if cmd == "build":
    src = os.path.basename(argv[1]); target = "wasm" if "--target" in argv else "rust"
    out = argv[argv.index("-o") + 1]
    beh = S["programs"].get(src, {}).get(target, {"exit": 0, "stdout": "", "stderr": ""})
    if beh.get("build_fail"):
        sys.stderr.write(beh.get("build_stderr", "stub build error")); sys.exit(1)
    if target == "wasm":
        json.dump(beh, open(out, "w"))
    else:
        with open(out, "w") as f:
            f.write("#!/usr/bin/env python3\nimport sys\n"
                    f"sys.stdout.write({beh.get('stdout', '')!r})\n"
                    f"sys.stderr.write({beh.get('stderr', '')!r})\n"
                    f"sys.exit({beh.get('exit', 0)})\n")
        os.chmod(out, 0o755)
    sys.exit(0)
if cmd == "check":
    src = os.path.basename(os.path.dirname(argv[1])) + "/" + os.path.basename(argv[1])
    beh = S["checks"].get(src, {"exit": 0, "stdout": "", "stderr": ""})
    sys.stdout.write(beh.get("stdout", "")); sys.stderr.write(beh.get("stderr", ""))
    sys.exit(beh.get("exit", 0))
if cmd == "test":
    key = argv[1] + ("/wasm" if ["--target", "wasm"] == argv[2:4] else "/rust")
    beh = S["suites"].get(key, {"exit": 0, "stdout": "All 1 test file(s) passed"})
    sys.stdout.write(beh.get("stdout", "")); sys.exit(beh.get("exit", 0))
sys.exit(2)
'''

STUB_REF = r'''#!/usr/bin/env python3
import json, os, sys
if sys.argv[1:2] == ["--version"]:
    print("als-ref 0.0-stub"); sys.exit(0)
S = json.load(open(os.environ["ALS_STUB_SCENARIO"]))
# argv: run <path> --json
name = os.path.basename(sys.argv[2])
doc = S.get("refs", {}).get(name)
if doc is None:
    print(json.dumps({"abstain": {"class": "stub:unscripted", "reason": "no scripted reply"}})); sys.exit(0)
if doc == "MALFORMED":
    print("this is not json"); sys.exit(0)
print(json.dumps(doc)); sys.exit(0)
'''

STUB_WASMTIME = r'''#!/usr/bin/env python3
import json, sys
if sys.argv[1:2] == ["--version"]:
    print("wasmtime 0.0-stub"); sys.exit(0)
beh = json.load(open(sys.argv[-1]))
sys.stdout.write(beh.get("stdout", "")); sys.stderr.write(beh.get("stderr", ""))
sys.exit(beh.get("exit", 0))
'''


def make_root(td):
    root = os.path.join(td, "judge")
    for d in ["spec/wasm_cross", "spec/wasm_cross_pkg", "spec/programs", "spec/wasm_fail",
              "spec/lang", "spec/stdlib", "spec/integration", "tests/diagnostics"]:
        os.makedirs(os.path.join(root, d), exist_ok=True)
    subprocess.run(["git", "init", "-q", root], check=True)
    return root


def write(root, rel, text):
    p = os.path.join(root, rel)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "w") as f:
        f.write(text)


def run_case(td, name, legs, build, expect_exit, expect):
    """build(root, scenario) populates the tree + stub behaviour."""
    root = make_root(td)
    scenario = {"programs": {}, "checks": {}, "suites": {}, "refs": {}}
    build(root, scenario)
    sdir = os.path.join(td, "stub")
    os.makedirs(sdir, exist_ok=True)
    spath = os.path.join(sdir, "scenario.json")
    json.dump(scenario, open(spath, "w"))
    for fname, body in [("almide", STUB_ALMIDE), ("wasmtime", STUB_WASMTIME), ("als-ref", STUB_REF)]:
        p = os.path.join(sdir, fname)
        open(p, "w").write(body)
        os.chmod(p, 0o755)
    report = os.path.join(td, "report.toml")
    env = dict(os.environ, ALS_STUB_SCENARIO=spath, PATH=sdir + os.pathsep + os.environ["PATH"])
    r = subprocess.run([sys.executable, RUNNER, "--almide", os.path.join(sdir, "almide"),
                        "--ref", os.path.join(sdir, "als-ref"),
                        "--legs", legs, "--jobs", "1", "--report", report, "--root", root],
                       env=env, capture_output=True, text=True, timeout=300)
    problems = []
    if r.returncode != expect_exit:
        problems.append(f"exit={r.returncode}, want {expect_exit}\n{r.stdout[-800:]}\n{r.stderr[-400:]}")
    got = {}
    for block in re.split(r'\[\[leg\]\]', open(report).read())[1:]:
        n = re.search(r'name = "(\w+)"', block).group(1)
        got[n] = {k: int(re.search(rf'{k} = (\d+)', block).group(1))
                  for k in ("total", "passed", "allowed", "stale", "failed", "skipped")}
    for legname, want in expect.items():
        for k, v in want.items():
            if got.get(legname, {}).get(k) != v:
                problems.append(f"{legname}.{k}={got.get(legname, {}).get(k)}, want {v}")
    shutil.rmtree(root); shutil.rmtree(sdir)
    return problems


CASES = []
def case(name, legs, expect_exit, expect):
    def deco(fn):
        CASES.append((name, legs, fn, expect_exit, expect))
        return fn
    return deco


def cross_fixture(root, s, name, native, wasm, header=""):
    write(root, f"spec/wasm_cross/{name}.almd", header + "fn main() -> Unit = println(\"x\")\n")
    s["programs"][f"{name}.almd"] = {"rust": native, "wasm": wasm}

OK = {"exit": 0, "stdout": "x\n", "stderr": ""}

@case("cross agreement is the only green", "cross", 0, {"cross": {"total": 1, "passed": 1, "failed": 0}})
def _(root, s): cross_fixture(root, s, "agree", OK, OK)

@case("cross stdout divergence is red", "cross", 1, {"cross": {"passed": 0, "failed": 1}})
def _(root, s): cross_fixture(root, s, "d_out", OK, dict(OK, stdout="y\n"))

@case("cross stderr divergence is red", "cross", 1, {"cross": {"failed": 1}})
def _(root, s): cross_fixture(root, s, "d_err", OK, dict(OK, stderr="warning\n"))

@case("cross exit divergence is red", "cross", 1, {"cross": {"failed": 1}})
def _(root, s): cross_fixture(root, s, "d_exit", OK, dict(OK, exit=1))

@case("a build refusal is red, not a comparison", "cross", 1, {"cross": {"failed": 1}})
def _(root, s): cross_fixture(root, s, "wall", OK, {"build_fail": True, "build_stderr": "renderer wall"})

@case("xt-allow shields a live divergence but counts it", "cross", 0, {"cross": {"allowed": 1, "passed": 0, "failed": 0}})
def _(root, s): cross_fixture(root, s, "track", OK, dict(OK, stdout="y\n"), header="// @xt-allow: known #0 divergence\n")

@case("a healed xt-allow is STALE and red", "cross", 1, {"cross": {"stale": 1, "failed": 0}})
def _(root, s): cross_fixture(root, s, "stale", OK, OK, header="// @xt-allow: was divergent\n")

def fail_fixture(root, s, name, native, wasm, header):
    write(root, f"spec/wasm_fail/{name}.almd", header + "effect fn main() -> Unit = die()\n")
    s["programs"][f"{name}.almd"] = {"rust": native, "wasm": wasm}

BOOM = {"exit": 1, "stdout": "", "stderr": "Error: boom\n"}

@case("fail: declared failure on both legs is green", "fail", 0, {"fail": {"passed": 1}})
def _(root, s): fail_fixture(root, s, "boom", BOOM, BOOM, "// @expect-fail: boom\n")

@case("fail: running to success is itself the failure", "fail", 1, {"fail": {"failed": 1}})
def _(root, s): fail_fixture(root, s, "ok", OK, OK, "// @expect-fail: boom\n")

@case("fail: wrong message is red", "fail", 1, {"fail": {"failed": 1}})
def _(root, s): fail_fixture(root, s, "msg", BOOM, BOOM, "// @expect-fail: different words\n")

@case("fail: legs breaking differently is red", "fail", 1, {"fail": {"failed": 1}})
def _(root, s): fail_fixture(root, s, "split", BOOM, dict(BOOM, exit=2), "// @expect-fail: boom\n")

@case("fail: a header-less fixture is red, never skipped", "fail", 1, {"fail": {"failed": 1}})
def _(root, s): fail_fixture(root, s, "nohdr", BOOM, BOOM, "")

@case("fail: xf-allow tracks a split without passing it", "fail", 0, {"fail": {"allowed": 1}})
def _(root, s): fail_fixture(root, s, "xf", BOOM, dict(BOOM, exit=2), "// @expect-fail: boom\n// @xf-allow: tracked #0\n")

@case("fail: a healed xf-allow is stale and red", "fail", 1, {"fail": {"stale": 1}})
def _(root, s): fail_fixture(root, s, "xfstale", BOOM, BOOM, "// @expect-fail: boom\n// @xf-allow: was split\n")

def diag_case(root, s, broken, fixed, meta='expects_code = "E001"\nexpects_error = "bad thing"\nhint_substring = "try this"\n'):
    write(root, "tests/diagnostics/case-a/broken.almd", "let\n")
    write(root, "tests/diagnostics/case-a/fixed.almd", "fn main() -> Unit = ()\n")
    write(root, "tests/diagnostics/case-a/meta.toml", meta)
    s["checks"]["case-a/broken.almd"] = broken
    s["checks"]["case-a/fixed.almd"] = fixed

REJECT = {"exit": 1, "stdout": "error[E001]: bad thing\n  hint: try this\n"}
CLEAN = {"exit": 0, "stdout": ""}

@case("diag: pinned rejection + clean fix is green", "diag", 0, {"diag": {"passed": 1}})
def _(root, s): diag_case(root, s, REJECT, CLEAN)

@case("diag: an accepted broken file is red", "diag", 1, {"diag": {"failed": 1}})
def _(root, s): diag_case(root, s, CLEAN, CLEAN)

@case("diag: missing code is red", "diag", 1, {"diag": {"failed": 1}})
def _(root, s): diag_case(root, s, {"exit": 1, "stdout": "error[E999]: bad thing\n  hint: try this\n"}, CLEAN)

@case("diag: missing hint is red", "diag", 1, {"diag": {"failed": 1}})
def _(root, s): diag_case(root, s, {"exit": 1, "stdout": "error[E001]: bad thing\n"}, CLEAN)

@case("diag: a fix that does not compile is red", "diag", 1, {"diag": {"failed": 1}})
def _(root, s): diag_case(root, s, REJECT, {"exit": 1, "stdout": "error[E001]: still bad\n"})

@case("suite: 0 failed on both targets is green", "suite", 0, {"suite": {"passed": 6, "failed": 0}})
def _(root, s):
    for d in ["spec/lang", "spec/stdlib", "spec/integration"]:
        write(root, f"{d}/t_test.almd", "test \"t\" { assert(true) }\n")
        s["suites"][f"{d}/rust"] = {"exit": 0, "stdout": "All 1 test file(s) passed"}
        s["suites"][f"{d}/wasm"] = {"exit": 0, "stdout": "1 passed, 0 failed (of 1 files)"}

@case("suite: a failed count is red even at exit 0", "suite", 1, {"suite": {"failed": 1}})
def _(root, s):
    for d in ["spec/lang", "spec/stdlib", "spec/integration"]:
        write(root, f"{d}/t_test.almd", "test \"t\" { assert(true) }\n")
        s["suites"][f"{d}/rust"] = {"exit": 0, "stdout": "All 1 test file(s) passed"}
        s["suites"][f"{d}/wasm"] = {"exit": 0, "stdout": "1 passed, 0 failed (of 1 files)"}
    s["suites"]["spec/lang/wasm"] = {"exit": 0, "stdout": "0 passed, 2 failed (of 2 files)"}


# ── the ref leg: the verdict is legs == REFERENCE, not legs agree ─────────

REF_OK = {"exit": 0, "stdout": "x\n", "stderr": ""}

def ref_fixture(root, s, name, native, wasm, ref, header=""):
    cross_fixture(root, s, name, native, wasm, header=header)
    s["refs"][f"{name}.almd"] = ref

@case("ref: both legs equal the reference is the only green", "ref", 0, {"ref": {"total": 1, "passed": 1, "failed": 0, "skipped": 0}})
def _(root, s): ref_fixture(root, s, "r_ok", OK, OK, REF_OK)

@case("ref: native differing from the reference is red", "ref", 1, {"ref": {"failed": 1}})
def _(root, s): ref_fixture(root, s, "r_nat", dict(OK, stdout="y\n"), OK, REF_OK)

@case("ref: wasm differing from the reference is red", "ref", 1, {"ref": {"failed": 1}})
def _(root, s): ref_fixture(root, s, "r_wasm", OK, dict(OK, stdout="y\n"), REF_OK)

@case("ref: CO-DRIFT — the targets agree with each other and both differ from the reference — is red", "ref", 1, {"ref": {"failed": 1, "passed": 0}})
def _(root, s): ref_fixture(root, s, "r_codrift", dict(OK, stdout="y\n"), dict(OK, stdout="y\n"), REF_OK)

@case("ref: a reference abstain is counted and skipped, never a verdict", "ref", 0, {"ref": {"total": 1, "passed": 0, "failed": 0, "skipped": 1}})
def _(root, s): ref_fixture(root, s, "r_abst", OK, OK, {"abstain": {"class": "stdlib:x.y", "reason": "scripted"}})

@case("ref: a malformed protocol reply is red, never a verdict", "ref", 1, {"ref": {"failed": 1}})
def _(root, s): ref_fixture(root, s, "r_bad", OK, OK, "MALFORMED")

@case("ref: an evaluator fault is red", "ref", 1, {"ref": {"failed": 1}})
def _(root, s): ref_fixture(root, s, "r_fault", OK, OK, {"error": "scripted fault"})

@case("ref: @ref-allow tracks an adjudicated disagreement without passing it", "ref", 0, {"ref": {"allowed": 1, "passed": 0, "failed": 0}})
def _(root, s): ref_fixture(root, s, "r_track", dict(OK, stdout="y\n"), dict(OK, stdout="y\n"), REF_OK, header="// @ref-allow: F-numbered finding under adjudication\n")

@case("ref: a healed @ref-allow is STALE and red", "ref", 1, {"ref": {"stale": 1, "failed": 0}})
def _(root, s): ref_fixture(root, s, "r_stale", OK, OK, REF_OK, header="// @ref-allow: was disagreeing\n")


def main():
    failures = []
    for name, legs, build, expect_exit, expect in CASES:
        with tempfile.TemporaryDirectory(prefix="als-selftest-") as td:
            problems = run_case(td, name, legs, build, expect_exit, expect)
        status = "ok" if not problems else "FAIL"
        print(f"  {status:4s} {name}")
        for p in problems:
            print(f"       {p}")
        if problems:
            failures.append(name)
    n = len(CASES)
    expected_n = 30
    if n != expected_n:
        failures.append(f"scenario count {n} != declared {expected_n} — a dropped scenario is a silent hole")
        print(f"  FAIL scenario count {n} != {expected_n}")
    if failures:
        print(f"selftest-conformance: {len(failures)} FAILURE(S) — the runner's verdicts cannot be trusted until this is green")
        return 1
    print(f"selftest-conformance OK: {n} scenarios — every verdict class turns red for its reason and green only for agreement (and, on the ref leg, only for agreement WITH THE REFERENCE)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
