#!/usr/bin/env bash
# GATE-VERIFICATION LEDGER GATE — every verdict-bearing tool in this
# repository must carry a row in proofs/gate-verification.toml classifying
# how we know the tool itself can fail correctly. See that file's header.
set -euo pipefail
export LC_ALL=C
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LEDGER="$ROOT/proofs/gate-verification.toml"

enumerate() {
  (cd "$ROOT" && ls scripts/check-*.sh scripts/conformance.py scripts/selftest-conformance.py scripts/doctest.py scripts/edition-readiness.py) | sort -u
}

python3 - "$LEDGER" <<PYEOF
import re, sys
ledger_path = sys.argv[1]
tools = """$(enumerate)""".split()
src = open(ledger_path, encoding="utf-8").read()
m = re.search(r'#\s*unverified_ceiling\s*=\s*"(\d+)"', src)
errs = []
if not m:
    errs.append('ledger header is missing # unverified_ceiling = "N"')
ceiling = int(m.group(1)) if m else 0
rows = {}
for block in re.split(r'\[\[gate\]\]', src)[1:]:
    p = re.search(r'path = "([^"]+)"', block)
    c = re.search(r'class = "([^"]+)"', block)
    e = re.search(r'evidence = "', block)
    if not p or not c:
        errs.append(f"row missing path/class: {block[:60]!r}"); continue
    if p.group(1) in rows:
        errs.append(f"{p.group(1)}: duplicate row")
    cls = c.group(1)
    if cls not in ("MUTATION_TESTED", "NEGATIVE_TESTED", "EXERCISED", "UNVERIFIED"):
        errs.append(f"{p.group(1)}: unknown class {cls!r}")
    if cls != "UNVERIFIED" and not e:
        errs.append(f"{p.group(1)}: class {cls} requires evidence")
    rows[p.group(1)] = cls
for t in tools:
    if t not in rows:
        errs.append(f"{t}: verdict-bearing tool with NO verification row (UNCLASSIFIED)")
for t in rows:
    if t not in tools:
        errs.append(f"{t}: STALE row — tool no longer exists (or is no longer enumerated)")
unverified = sum(1 for c in rows.values() if c == "UNVERIFIED")
if unverified > ceiling:
    errs.append(f"UNVERIFIED count {unverified} exceeds ceiling {ceiling}")
elif unverified < ceiling:
    errs.append(f"UNVERIFIED count {unverified} is BELOW the ceiling {ceiling} — ratchet it down")
for e in errs:
    print(f"::error::{e}")
if errs:
    sys.exit(1)
print(f"gate-verification OK: {len(tools)} tool(s), {unverified} UNVERIFIED (ceiling {ceiling}).")
PYEOF
