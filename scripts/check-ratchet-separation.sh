#!/usr/bin/env bash
# RATCHET-SEPARATION GATE — who moved the ratchet, and did they say why.
#
# Every shrink-only ledger in this repository has a ceiling (or a floor). The
# four-direction law (docs/specs/als/STANDARD.md) says growth needs an
# authored, named justification. This gate makes that mechanical, in the shape
# the implementation's F5 finding demanded (flight-evidence-gaps F5: the person
# changing the code must not silently move the baseline in the same commit):
#
#   A commit that LOOSENS a ratchet (ceiling up, floor down) may touch ONLY
#   that ledger (plus the generated documents that embed its numbers); for a
#   hand-edited ledger its added lines must also carry a dated justification
#   comment (`# YYYY-MM-DD: …`) — a regenerated ledger (`--write`) carries the
#   why in that solo commit's message. Tightening is free — it rides along
#   with the change that earned it.
#
#   bash scripts/check-ratchet-separation.sh --staged          # lefthook: index vs HEAD
#   bash scripts/check-ratchet-separation.sh --range A..B      # CI: every commit in A..B
#
# Ratchets watched: gate-verification unverified_ceiling, als-validation
# unvalidated_ceiling, contract-provenance retroactive_ceiling,
# als-element-coverage unwritten_ceiling (all: up = loosen), runner-coverage
# line_floor (down = loosen), ref-abstain ceiling (up = loosen). The
# flagged-for-revision count is already hard-capped at 0 by check-contracts.sh.
set -uo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.." || exit 2
python3 - "$@" <<'PY'
import re, subprocess, sys

RATCHETS = [  # (path, regex with one group, loosen-direction, hand-edited?)
    # hand-edited ledgers: a loosening must add a dated justification comment.
    ("proofs/gate-verification.toml",   r'^#\s*unverified_ceiling\s*=\s*"(\d+)"',  "up",   True),
    ("proofs/als-validation.toml",      r'^#\s*unvalidated_ceiling\s*=\s*"(\d+)"', "up",   True),
    ("proofs/contract-provenance.toml", r'^#\s*retroactive_ceiling\s*=\s*"(\d+)"', "up",   True),
    ("proofs/als-element-coverage.toml",r'^#\s*unwritten_ceiling\s*=\s*"(\d+)"',   "up",   True),
    # regenerated ledgers (`--write`, never edited): separation is the whole rule;
    # the why lives in the commit message of that solo commit.
    ("proofs/runner-coverage.toml",     r'^#\s*line_floor\s*=\s*"([\d.]+)"',       "down", False),
    ("proofs/ref-abstain.toml",         r'^ceiling\s*=\s*(\d+)',                    "up",   False),
]
GENERATED = {"docs/contracts/README.md", "docs/contracts/conformance.md", "docs/specs/als/README.md"}
DATED = re.compile(r'^\+\s*#\s*\d{4}-\d{2}-\d{2}')

def sh(*a):
    r = subprocess.run(a, capture_output=True, text=True)
    return r.returncode, r.stdout

def value(rev, path, rx):
    spec = f":{path}" if rev == ":" else f"{rev}:{path}"   # ":" = the index (staged)
    rc, src = sh("git", "show", spec)
    if rc != 0:
        return None
    m = re.search(rx, src, re.M)
    return float(m.group(1)) if m else None

def judge(label, old_rev, new_rev, files, diff_of):
    """files: changed paths; diff_of(path) -> unified diff text. Returns list of errors."""
    errs, loosened = [], []
    for path, rx, direction, hand in RATCHETS:
        if path not in files:
            continue
        old, new = value(old_rev, path, rx), value(new_rev, path, rx)
        if old is None or new is None:
            continue
        if (direction == "up" and new > old) or (direction == "down" and new < old):
            loosened.append((path, old, new, hand))
    if not loosened:
        return errs
    for path, old, new, hand in loosened:
        others = sorted(f for f in files if f != path and f not in GENERATED)
        if others:
            errs.append(f"{label}: loosens {path} ({old:g} → {new:g}) in the same commit as {others} — "
                        f"a loosening is its own commit (only the ledger + generated docs)")
        if hand and not any(DATED.match(l) for l in diff_of(path).splitlines()):
            errs.append(f"{label}: loosens {path} ({old:g} → {new:g}) without a dated justification line "
                        f"(`# YYYY-MM-DD: why`) added to the ledger")
    return errs

args = sys.argv[1:]
if not any(subprocess.run(["test", "-f", p], capture_output=True).returncode == 0 for p, _, _, _ in RATCHETS):
    print("::error::none of the ratchet ledgers exist — a broken instrument is not a pass"); sys.exit(1)
errs, judged = [], 0
if args[:1] == ["--staged"]:
    rc, out = sh("git", "diff", "--cached", "--name-only")
    files = set(out.split())
    if files:
        judged = 1
        errs += judge("staged", "HEAD", ":", files, lambda p: sh("git", "diff", "--cached", "--", p)[1])
elif args[:1] == ["--range"] and len(args) > 1:
    rng = args[1]
    if rng.startswith("0000000"):
        print("ratchet-separation: first push (no base) — nothing to compare"); sys.exit(0)
    rc, out = sh("git", "rev-list", "--reverse", "--no-merges", rng)
    if rc != 0:
        print(f"::error::cannot resolve range {rng!r}"); sys.exit(2)
    for c in out.split():
        rc2, names = sh("git", "diff-tree", "--no-commit-id", "--name-only", "-r", c)
        files = set(names.split())
        judged += 1
        errs += judge(c[:9], f"{c}^", c, files, lambda p, c=c: sh("git", "diff", f"{c}^", c, "--", p)[1])
else:
    print("usage: check-ratchet-separation.sh --staged | --range A..B"); sys.exit(2)
for e in errs:
    print(f"::error::{e}")
if errs:
    sys.exit(1)
print(f"ratchet-separation OK: {judged} commit(s) judged, no loosening outside its own dated commit.")
PY
