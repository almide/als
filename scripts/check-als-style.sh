#!/usr/bin/env bash
# ALS REQUIREMENTS-STANDARD GATE — the machine-checkable half of
# docs/specs/als/STANDARD.md. Fails on: a section id whose prefix is not in
# the registry, a prefix used outside its registered chapter, a duplicate id
# (the 2026-08-20 class: 12 ids ambiguous across chapters, one three-ways,
# citations already split across meanings), a chapter without a
# `> Last updated:` line, or forbidden vocabulary in a normative chapter.
set -uo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.." || exit 2
python3 - <<'PY'
import re, glob, sys, collections
std = open("docs/specs/als/STANDARD.md", encoding="utf-8").read()
registry = {}
for m in re.finditer(r'^\|\s*([A-Z]+)\s*\|\s*(\S+\.md)\s*\|', std, re.M):
    registry[m.group(1)] = m.group(2)
if not registry:
    print("::error::STANDARD.md prefix registry table not found"); sys.exit(2)
errs, seen = [], {}
chapters = [f for f in sorted(glob.glob("docs/specs/als/*.md"))
            if not f.endswith(("README.md", "STANDARD.md"))]
forbidden = ["TODO", "FIXME", "たぶん", "そのうち", "予定"]
for f in chapters:
    base = f.split("/")[-1]
    text = open(f, encoding="utf-8").read()
    if not re.search(r'^> Last updated: \d{4}-\d{2}-\d{2}', text, re.M):
        errs.append(f"{base}: missing `> Last updated: YYYY-MM-DD` line")
    for w in forbidden:
        for i, line in enumerate(text.splitlines(), 1):
            if w in line:
                errs.append(f"{base}:{i}: forbidden vocabulary {w!r} in a normative chapter")
    for m in re.finditer(r'^##\s+ALS-([A-Z]+)(\d+[a-z]?)\s', text, re.M):
        prefix, sid = m.group(1), f"ALS-{m.group(1)}{m.group(2)}"
        if prefix not in registry:
            errs.append(f"{base}: {sid} uses unregistered prefix {prefix!r} (register it in STANDARD.md or renumber)")
        elif registry[prefix] != base:
            errs.append(f"{base}: {sid} — prefix {prefix} belongs to {registry[prefix]} (one prefix, one chapter)")
        if sid in seen:
            errs.append(f"{base}: {sid} duplicates {seen[sid]} — ids are unique repo-wide")
        seen[sid] = base
    # a heading that LOOKS like a section but fails the id grammar
    for m in re.finditer(r'^##\s+ALS-(\S+)', text, re.M):
        if not re.match(r'^[A-Z]+\d+[a-z]?$', m.group(1)):
            errs.append(f"{base}: malformed section id ALS-{m.group(1)}")
# registry rows must point at real chapters, and every chapter must appear
for prefix, chap in registry.items():
    if f"docs/specs/als/{chap}" not in chapters:
        errs.append(f"STANDARD.md registry: prefix {prefix} names missing chapter {chap}")
for f in chapters:
    if f.split("/")[-1] not in registry.values():
        errs.append(f"{f}: chapter owns no registered prefix — add its row to STANDARD.md")
# ── fixture naming discipline (ADR-0013): subject-first snake_case, never a
# bare number or an issue-number name — the mechanical key belongs in the
# contract ledger and doc comments, not the filename.
import os
for d in ("spec/wasm_cross", "spec/wasm_fail", "spec/lang", "spec/stdlib", "spec/integration", "spec/programs"):
    if not os.path.isdir(d):
        continue
    for base, _, files in os.walk(d):
        for fn in files:
            if not fn.endswith(".almd"):
                continue
            if not re.fullmatch(r'[a-z0-9_]+\.almd', fn):
                errs.append(f"{base}/{fn}: fixture names are lowercase snake_case (ADR-0013)")
            if re.match(r'^(issue|bug|fix)?[_-]?\d+(_test)?\.almd$', fn):
                errs.append(f"{base}/{fn}: a number is not a subject — name what the fixture exercises (ADR-0013)")
for e in errs:
    print(f"::error::{e}")
if errs: sys.exit(1)
print(f"als-style OK: {len(seen)} unique section ids across {len(chapters)} chapters; prefix registry bijective; vocabulary clean.")
PY
