#!/usr/bin/env python3
"""Edition readiness — the baseline instrument, as a program.

    scripts/edition-readiness.py [--tag vX.Y.Z] [--almide <bin>]

An edition (tag) of the specification is an audit baseline; whether one may
be cut is a MEASURED verdict, not a skimmed checklist (the reference field's
lesson: a release checklist that is not a script is not a checklist). Every
check reports one of:

  OK     holds
  WARN   noted, does not block (recorded so it cannot be unseen)
  FAIL   blocks the edition; accumulate and keep measuring
  FATAL  blocks and stops immediately

Exit 0 only with zero FAIL/FATAL. The first tag is the semantics freeze;
this instrument is how the freeze precondition list stays honest.

READY includes the measurement itself: the full conformance corpus judged
against the --almide binary (scripts/conformance.py; needs wasmtime on PATH
and the reference evaluator built). A run without --almide cannot be READY —
verifying the instrument is not taking the measurement (#59).
"""
import argparse, json, re, subprocess, sys

RESULTS = []
def verdict(kind, name, detail=""):
    RESULTS.append((kind, name, detail))
    print(f"  [{kind:5s}] {name}" + (f" — {detail}" if detail else ""))
    if kind == "FATAL":
        finish()

def finish():
    fails = [r for r in RESULTS if r[0] in ("FAIL", "FATAL")]
    warns = [r for r in RESULTS if r[0] == "WARN"]
    print(f"\nedition-readiness: {'NOT READY' if fails else 'READY'} — "
          f"{len(fails)} blocking, {len(warns)} warning(s), {len(RESULTS)} check(s)")
    sys.exit(1 if fails else 0)

def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, timeout=kw.pop("timeout", 600), **kw)

def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--tag")
    ap.add_argument("--almide", help="binary to judge: full corpus (blocking) + doctest; without it the corpus check FAILs")
    args = ap.parse_args()

    # 1. configuration state
    br = run(["git", "branch", "--show-current"]).stdout.strip()
    verdict("OK" if br == "main" else "WARN", "on main", br or "detached")
    dirty = run(["git", "status", "--porcelain"]).stdout.strip()
    verdict("OK" if not dirty else "WARN", "working tree clean", f"{len(dirty.splitlines())} modified" if dirty else "")
    if args.tag:
        if not re.fullmatch(r"v\d+\.\d+\.\d+(-rc\d+)?", args.tag):
            verdict("FATAL", "tag shape", f"{args.tag!r} is not vMAJOR.MINOR.PATCH[-rcN]")
        if args.tag in run(["git", "tag", "-l"]).stdout.split():
            verdict("FATAL", "tag is new", f"{args.tag} already exists — a tag never moves")
        verdict("OK", "tag shape", args.tag)

    # 2. every gate, re-run now
    for gate in ["scripts/check-contracts.sh", "scripts/check-contract-provenance.py",
                 "scripts/check-als-element-coverage.sh",
                 "scripts/check-als-style.sh", "scripts/check-als-validation.sh", "scripts/check-links.sh",
                 "scripts/check-workflows.sh", "scripts/check-gate-verification.sh"]:
        r = run(["python3" if gate.endswith(".py") else "bash", gate])
        verdict("OK" if r.returncode == 0 else "FAIL", gate,
                "" if r.returncode == 0 else (r.stdout + r.stderr).strip().splitlines()[0][:120])

    # 3. the judge's own qualification
    r = run(["python3", "scripts/selftest-conformance.py"])
    verdict("OK" if r.returncode == 0 else "FAIL", "runner self-test",
            "" if r.returncode == 0 else "the runner's verdicts cannot be trusted")
    r = run(["python3", "scripts/check-runner-coverage.py"])
    verdict("OK" if r.returncode == 0 else "FAIL", "runner coverage floor",
            (r.stdout.strip().splitlines() or ["?"])[-1][:120])

    # 4. spec examples compile
    if args.almide:
        r = run(["python3", "scripts/doctest.py", "--almide", args.almide])
        verdict("OK" if r.returncode == 0 else "FAIL", "spec doctest",
                (r.stdout.strip().splitlines() or ["?"])[-1][:120])
    else:
        verdict("WARN", "spec doctest", "not run — pass --almide <bin>")

    # 5. the measurement — judge the binary against the full corpus (#59:
    # verifying the instrument is not taking the measurement)
    if args.almide:
        try:
            r = run(["python3", "scripts/conformance.py", "--almide", args.almide], timeout=7200)
        except subprocess.TimeoutExpired:
            r = None
        if r is None:
            verdict("FAIL", "corpus judgment", "timed out after 7200s")
        elif r.returncode == 0:
            tot = [0, 0, 0, 0, 0]
            for m in re.finditer(r"^\[\w+\] total=(\d+) passed=(\d+) allowed=(\d+) stale=(\d+) failed=(\d+)",
                                 r.stdout, re.M):
                tot = [a + int(b) for a, b in zip(tot, m.groups())]
            verdict("OK", "corpus judgment",
                    f"judged {tot[0]}: passed={tot[1]} allowed={tot[2]} stale={tot[3]} failed={tot[4]}")
        else:
            red = [l.strip() for l in r.stdout.splitlines()
                   if l.lstrip().startswith(("x FAIL:", "! stale:"))]
            detail = (f"{len(red)} red row(s); first: {red[0]}" if red
                      else ((r.stdout + r.stderr).strip().splitlines() or ["?"])[-1])
            verdict("FAIL", "corpus judgment", detail[:200])
    else:
        verdict("FAIL", "corpus judgment", "not run — READY requires judging a binary; pass --almide <bin>")

    # 6. freeze preconditions in the ledgers
    cov = open("proofs/als-element-coverage.toml").read()
    unwritten = cov.count('section = "UNWRITTEN"')
    verdict("OK" if unwritten == 0 else "FAIL", "element coverage UNWRITTEN = 0", str(unwritten))
    flagged = open("docs/contracts/contracts.toml").read().count('status    = "flagged-for-revision"')
    verdict("OK" if flagged == 0 else "FAIL", "flagged-for-revision = 0", str(flagged))

    # 7. edition-blocking problem reports (docs/ISSUE-TAXONOMY.md)
    blocking = 0
    try:
        for label in ["S-unsound", "S-ambiguous", "S-untestable", "S-divergence"]:
            out = run(["gh", "api", f"repos/almide/als/issues?state=open&labels={label}"], timeout=60)
            if out.returncode != 0:
                raise RuntimeError(out.stderr.strip()[:80])
            n = len(json.loads(out.stdout))
            blocking += n
            if n:
                verdict("FAIL", f"open {label}", f"{n} open — edition-blocking (docs/ISSUE-TAXONOMY.md)")
        if blocking == 0:
            verdict("OK", "no edition-blocking issues open")
    except Exception as e:
        verdict("WARN", "issue check", f"gh unavailable ({e}) — verify by hand before tagging")

    finish()

if __name__ == "__main__":
    main()
