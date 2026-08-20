#!/usr/bin/env bash
# ALS VALIDATION-RECORD GATE — the DO-178C A-3 half the style gate cannot hold.
#
# check-als-style.sh holds what a machine can: ids, prefixes, vocabulary.
# Whether a normative section is ACCURATE, CONSISTENT and COMPLETE is a
# reviewer's judgment, and a judgment that is not recorded does not exist.
# proofs/als-validation.toml records, per `## ALS-<id>` section, who reviewed
# it, when, whether independently of its author, with what verdict — bound to
# a hash of the section's text, so a review cannot outlive the words it
# covered. Sections with no row are UNVALIDATED, held under a shrink-only
# ceiling (STANDARD.md, Validation record).
#
#   bash scripts/check-als-validation.sh              # the gate
#   bash scripts/check-als-validation.sh --stamp ALS-T1   # print a row skeleton
#                                                     # with the section's current hash
# Fails on: a row whose section no longer exists; a row whose hash no longer
# matches the text (STALE — re-review, or delete the row and raise the ceiling
# with a justification); a malformed row (missing field, unknown verdict,
# `revise` without an issue); an unvalidated count that is not exactly the
# ceiling (four-direction law); zero sections enumerated.
set -uo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.." || exit 2
python3 - "$@" <<'PY'
import re, glob, sys, hashlib
LEDGER = "proofs/als-validation.toml"
VERDICTS = ("accurate", "revise")

def sections():
    out = {}
    chapters = [f for f in sorted(glob.glob("docs/specs/als/*.md"))
                if not f.endswith(("README.md", "STANDARD.md"))]
    for f in chapters:
        text = open(f, encoding="utf-8").read()
        heads = [m for m in re.finditer(r'^##\s+(ALS-[A-Z]+\d+[a-z]?)\s', text, re.M)]
        for i, m in enumerate(heads):
            end = heads[i + 1].start() if i + 1 < len(heads) else len(text)
            body = "\n".join(l.rstrip() for l in text[m.start():end].splitlines()).strip()
            out[m.group(1)] = (f.split("/")[-1], "sha256:" + hashlib.sha256(body.encode()).hexdigest()[:12])
    return out

secs = sections()
args = sys.argv[1:]
if args[:1] == ["--stamp"]:
    sid = args[1] if len(args) > 1 else ""
    if sid not in secs:
        print(f"::error::{sid!r} is not a section id (see docs/specs/als/README.md)"); sys.exit(2)
    print(f'[[section]]\nid = "{sid}"\nhash = "{secs[sid][1]}"\nreviewed = "YYYY-MM-DD"\n'
          f'by = "<github handle>"\nindependent = "no"\nverdict = "accurate"\n'
          f'# verdict: accurate | revise (revise requires issue = "#N"); independent: yes only for a reviewer who is not the author')
    sys.exit(0)

errs = []
if not secs:
    print("::error::zero ALS sections enumerated — a broken instrument is not a pass"); sys.exit(1)
try:
    src = open(LEDGER, encoding="utf-8").read()
except FileNotFoundError:
    print(f"::error::{LEDGER} missing"); sys.exit(1)
m = re.search(r'^#\s*unvalidated_ceiling\s*=\s*"(\d+)"', src, re.M)
if not m:
    errs.append('ledger header is missing # unvalidated_ceiling = "N"')
ceiling = int(m.group(1)) if m else -1
rows = {}
for block in re.split(r'^\[\[section\]\]', src, flags=re.M)[1:]:
    f = dict(re.findall(r'^(\w+)\s*=\s*"([^"]*)"', block, re.M))
    sid = f.get("id", "")
    if not sid:
        errs.append(f"row without id: {block.strip()[:50]!r}"); continue
    if sid in rows:
        errs.append(f"{sid}: duplicate row"); continue
    rows[sid] = f
    if sid not in secs:
        errs.append(f"{sid}: STALE row — section no longer exists"); continue
    for k in ("hash", "reviewed", "by", "independent", "verdict"):
        if not f.get(k):
            errs.append(f"{sid}: row missing {k}")
    if f.get("hash") and f["hash"] != secs[sid][1]:
        errs.append(f"{sid}: STALE — text changed since review ({f['hash']} → {secs[sid][1]}); "
                    f"re-review (--stamp {sid}) or delete the row and raise the ceiling with a justification")
    if f.get("reviewed") and not re.fullmatch(r"\d{4}-\d{2}-\d{2}", f["reviewed"]):
        errs.append(f"{sid}: reviewed must be YYYY-MM-DD")
    if f.get("independent") not in ("yes", "no"):
        errs.append(f"{sid}: independent must be yes|no")
    if f.get("verdict") not in VERDICTS:
        errs.append(f"{sid}: verdict must be one of {VERDICTS}")
    elif f["verdict"] == "revise" and not re.fullmatch(r"#\d+", f.get("issue", "")):
        errs.append(f'{sid}: verdict revise requires issue = "#N"')
validated = [s for s in secs if s in rows]
unvalidated = [s for s in secs if s not in rows]
if ceiling >= 0:
    if len(unvalidated) > ceiling:
        errs.append(f"unvalidated sections {len(unvalidated)} exceed ceiling {ceiling} — a new section lands with its "
                    f"review, or the ceiling is raised by exactly that with a justification")
    elif len(unvalidated) < ceiling:
        errs.append(f"unvalidated sections {len(unvalidated)} BELOW the ceiling {ceiling} — ratchet it down")
for e in errs:
    print(f"::error::{e}")
if errs:
    sys.exit(1)
indep = sum(1 for s in validated if rows[s].get("independent") == "yes")
print(f"als-validation OK: {len(secs)} sections — validated {len(validated)} (independent {indep}), "
      f"unvalidated {len(unvalidated)} (ceiling {ceiling}, shrink-only).")
PY
