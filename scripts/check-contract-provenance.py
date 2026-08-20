#!/usr/bin/env python3
"""Contract provenance — did the requirement precede the behaviour, measured.

    scripts/check-contract-provenance.py            # gate: verify the committed ledger
    scripts/check-contract-provenance.py --write    # regenerate from git history
        [--impl-root <almide checkout>]             # release dates from its tags

CONTRIBUTING.md promises the two-PR order: a behaviour lands HERE before it
lands in an implementation. A promise about order is checkable: for every
contract the ledger records the committer timestamp at which its id first
appeared in docs/contracts/contracts.toml (git pickaxe, the ENTRY) and the
committer timestamp of the `since` release tag (the RELEASE), and classifies:

  requirements-first   entry in the two-repo regime (>= regime_start) and the
                       release not yet cut at entry — the implementation pins
                       a commit that already contains the contract
  contemporaneous      entry <= release, single-repo era: the contract shipped
                       with the change it certifies (the ledger header's
                       "normal case")
  retroactive          entry > release: written over behaviour that had
                       already shipped (C-099..C-115 style; the 0.24.0
                       bootstrap batch). Shrink-only: `retroactive_ceiling`
  unmeasured           `since` names a release with no recorded date

The gate needs NO history: it recomputes every class from the committed
entry/release timestamps, so a hand-edited class fails; it fails on a missing or
stale row, on a retroactive count that is not exactly the ceiling (four-
direction law, STANDARD.md), and on an empty measurement. `--write` needs the
full history (a shallow clone is refused) and the release dates — from
`--impl-root`'s tags, else from `v<since>` tags in this clone (the extraction
carried the implementation's tags along).
"""
import argparse, os, re, subprocess, sys

LEDGER = "docs/contracts/contracts.toml"
PROV = "proofs/contract-provenance.toml"
CLASSES = ("requirements-first", "contemporaneous", "retroactive", "unmeasured")

def sh(*cmd, cwd=None):
    r = subprocess.run(cmd, capture_output=True, text=True, cwd=cwd)
    return r.returncode, r.stdout.strip()

def read_contracts():
    src = open(LEDGER, encoding="utf-8").read()
    out = []
    for block in re.split(r'^\[\[contract\]\]', src, flags=re.M)[1:]:
        i = re.search(r'^id\s*=\s*"(C-\d{3})"', block, re.M)
        s = re.search(r'^since\s*=\s*"([^"]+)"', block, re.M)
        if i:
            out.append((i.group(1), s.group(1) if s else ""))
    return out

def read_prov():
    if not os.path.exists(PROV):
        return None
    src = open(PROV, encoding="utf-8").read()
    hdr = {k: v for k, v in re.findall(r'^#\s*(\w+)\s*=\s*"([^"]*)"', src, re.M)}
    releases, rows = {}, {}
    for block in re.split(r'^\[\[release\]\]', src, flags=re.M)[1:]:
        block = block.split("[[contract]]")[0]
        v = re.search(r'^version\s*=\s*"([^"]+)"', block, re.M)
        d = re.search(r'^date\s*=\s*"([^"]+)"', block, re.M)
        if v and d:
            releases[v.group(1)] = d.group(1)
    for block in re.split(r'^\[\[contract\]\]', src, flags=re.M)[1:]:
        f = {k: v for k, v in re.findall(r'^(\w+)\s*=\s*"([^"]*)"', block, re.M)}
        if "id" in f:
            rows[f["id"]] = f
    return hdr, releases, rows

def ts(iso):
    """Committer timestamp (git %cI) -> epoch seconds; offsets compared correctly."""
    from datetime import datetime
    return datetime.fromisoformat(iso).timestamp()

def classify(entry, since, releases, regime_start):
    rel = releases.get(since)
    if entry[:10] >= regime_start:
        # two-repo regime: the release must not predate the entry
        if rel is None or ts(entry) <= ts(rel):
            return "requirements-first"
        return "retroactive"
    if rel is None:
        return "unmeasured"
    return "contemporaneous" if ts(entry) <= ts(rel) else "retroactive"

# ── --write ──────────────────────────────────────────────────────────────────
def write(impl_root, regime_start):
    rc, depth = sh("git", "rev-list", "--count", "HEAD")
    if rc != 0 or int(depth or 0) < 500:
        print(f"::error::--write needs the full history (rev-list count = {depth!r}); refusing on a shallow clone")
        sys.exit(2)
    contracts = read_contracts()
    # release dates: implementation tags first, else this clone's inherited tags
    releases, source = {}, {}
    for _, since in contracts:
        if not since or since in releases:
            continue
        for where, cwd in (("implementation", impl_root), ("local", None)):
            if where == "implementation" and not impl_root:
                continue
            rc, d = sh("git", "log", "-1", "--format=%cI", f"v{since}", cwd=cwd)
            if rc == 0 and d:
                releases[since] = d
                source[since] = f"tag v{since} ({'almide/almide' if where == 'implementation' else 'inherited local tag'})"
                break
    prev = read_prov()
    prev_ceiling = None
    if prev and prev[0].get("retroactive_ceiling", "").isdigit():
        prev_ceiling = int(prev[0]["retroactive_ceiling"])
    rows = []
    for cid, since in contracts:
        rc, log = sh("git", "log", "--format=%h %cI", "--reverse", "-G",
                     rf'^id[[:space:]]*=[[:space:]]*"{cid}"', "--", LEDGER)
        first = log.splitlines()[0] if log else ""
        if not first:
            print(f"::error::{cid}: no commit introduces it in {LEDGER} (history incomplete?)"); sys.exit(2)
        commit, entry = first.split()
        rows.append((cid, since, entry, commit, classify(entry, since, releases, regime_start)))
    retro = sum(1 for r in rows if r[4] == "retroactive")
    if prev_ceiling is not None and retro > prev_ceiling:
        print(f"::warning::retroactive count {retro} exceeds the recorded ceiling {prev_ceiling} — "
              f"a new retroactive contract needs its justification named in the PR; ceiling rewritten")
    with open(PROV, "w", encoding="utf-8") as f:
        f.write(HEADER.format(retro=retro, regime=regime_start))
        for v in sorted(releases, key=lambda s: tuple(int(x) for x in s.split("."))):
            f.write(f'\n[[release]]\nversion = "{v}"\ndate = "{releases[v]}"\nsource = "{source[v]}"\n')
        for cid, since, entry, commit, cls in rows:
            f.write(f'\n[[contract]]\nid = "{cid}"\nsince = "{since}"\nentry = "{entry}"\n'
                    f'entry_commit = "{commit}"\nclass = "{cls}"\n')
    print(f"wrote {PROV}: {len(rows)} contracts, {len(releases)} releases; " + summary(rows))

HEADER = '''# CONTRACT PROVENANCE LEDGER (almide/als) — did the requirement precede the behaviour?
#
# One row per contract: the committer timestamp at which its id first appeared
# in docs/contracts/contracts.toml (ENTRY, git pickaxe) against the committer
# timestamp of the `since` release tag (RELEASE). Classes — recomputed by the
# gate from these two instants, so a hand-edited class fails:
#   requirements-first  entry in the two-repo regime (>= regime_start) and the
#                       release not yet cut at entry
#   contemporaneous     entry <= release (single-repo era: shipped together)
#   retroactive         entry > release (written over shipped behaviour)
#   unmeasured          no recorded date for the `since` release
# Regenerate: python3 scripts/check-contract-provenance.py --write [--impl-root ..]
# Gate:       python3 scripts/check-contract-provenance.py   (no history needed)
# The retroactive count is SHRINK-ONLY: growth needs a named justification in
# the PR that raises the ceiling; a count below the ceiling must ratchet it.
#
# regime_start = "{regime}"
# retroactive_ceiling = "{retro}"
'''

def summary(rows):
    n = {c: sum(1 for r in rows if r[4] == c) for c in CLASSES}
    return ", ".join(f"{c} {n[c]}" for c in CLASSES)

# ── gate ─────────────────────────────────────────────────────────────────────
def check():
    p = read_prov()
    errs = []
    if p is None:
        print(f"::error::{PROV} missing — run --write"); sys.exit(1)
    hdr, releases, rows = p
    regime = hdr.get("regime_start", "")
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", regime):
        errs.append('header is missing # regime_start = "YYYY-MM-DD"')
    try:
        ceiling = int(hdr["retroactive_ceiling"])
    except (KeyError, ValueError):
        errs.append('header is missing # retroactive_ceiling = "N"'); ceiling = -1
    contracts = read_contracts()
    ids = [c for c, _ in contracts]
    for cid, since in contracts:
        r = rows.get(cid)
        if not r:
            errs.append(f"{cid}: no provenance row — run --write"); continue
        if r.get("since") != since:
            errs.append(f"{cid}: row since={r.get('since')!r} but ledger since={since!r} — stale, run --write")
        for k in ("entry", "entry_commit", "class"):
            if not r.get(k):
                errs.append(f"{cid}: row missing {k}")
        if r.get("class") not in CLASSES:
            errs.append(f"{cid}: unknown class {r.get('class')!r}")
        elif regime and r.get("entry"):
            want = classify(r["entry"], since, releases, regime)
            if r["class"] != want:
                errs.append(f"{cid}: class {r['class']!r} does not follow from entry {r['entry']} / "
                            f"release {releases.get(since, 'none')} — recorded classes are derived, not declared")
    for cid in rows:
        if cid not in ids:
            errs.append(f"{cid}: STALE row — contract no longer in the ledger")
    classes = [rows[c]["class"] for c in ids if c in rows and rows[c].get("class") in CLASSES]
    retro = classes.count("retroactive")
    if ceiling >= 0:
        if retro > ceiling:
            errs.append(f"retroactive count {retro} exceeds ceiling {ceiling} — a contract over already-shipped "
                        f"behaviour needs its justification named and the ceiling raised by exactly that")
        elif retro < ceiling:
            errs.append(f"retroactive count {retro} is BELOW the ceiling {ceiling} — ratchet it down")
    measured = sum(1 for c in classes if c != "unmeasured")
    if not classes or measured == 0:
        errs.append("zero measured rows — a broken instrument is not a pass")
    for e in errs:
        print(f"::error::{e}")
    if errs:
        sys.exit(1)
    n = {c: classes.count(c) for c in CLASSES}
    print(f"contract-provenance OK: {len(ids)} contracts — " + ", ".join(f"{c} {n[c]}" for c in CLASSES)
          + f" (retroactive ceiling {ceiling}, regime since {regime}).")

def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--write", action="store_true", help="regenerate the ledger from git history")
    ap.add_argument("--impl-root", help="implementation checkout whose tags date the releases")
    ap.add_argument("--regime-start", default="2026-08-20",
                    help="first day of the two-repo regime (the extraction); default 2026-08-20")
    a = ap.parse_args()
    os.chdir(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
    if a.write:
        write(a.impl_root, a.regime_start)
    else:
        check()

if __name__ == "__main__":
    main()
