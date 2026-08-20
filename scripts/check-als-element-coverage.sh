#!/usr/bin/env bash
# ALS SYNTAX-ELEMENT COVERAGE GATE (Stage 3, the element→section direction).
# See proofs/als-element-coverage.toml's header for the model. The freeze
# precondition is UNWRITTEN = 0; until then the ceiling is shrink-only.
#
# TWO-REPO MODE (almide/als ⇄ an implementation): the ledger and the ALS
# sections live here; the ELEMENT ENUMERATOR reads an implementation's surface
# AST (scripts/lib/als-element-enumerate.py over crates/almide-syntax/src/ast.rs)
# and lives with that implementation. Pass it via
#   ALS_IMPL_ROOT=<path>   or   --impl-root <path>
# to run the full gate (stale rows, unclassified elements). Without a root the
# ledger-side checks run (well-formedness, section resolution, the ceiling
# ratchet) and the enumeration half is reported as deferred, never skipped
# silently.
set -uo pipefail
export LC_ALL=C
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LEDGER="$ROOT/proofs/als-element-coverage.toml"
IMPL_ROOT="${ALS_IMPL_ROOT:-}"
while [ $# -gt 0 ]; do
  case "$1" in
    --impl-root) IMPL_ROOT="${2:-}"; shift 2 ;;
    *) echo "::error::unknown argument '$1' (usage: $0 [--impl-root <implementation checkout>])"; exit 2 ;;
  esac
done

python3 - "$ROOT" "$LEDGER" "$IMPL_ROOT" <<'EOF'
import glob
import importlib.util
import os
import re
import sys

root, ledger_path, impl_root = sys.argv[1], sys.argv[2], sys.argv[3]
elements = None
if impl_root:
    enum_path = os.path.join(impl_root, "scripts/lib/als-element-enumerate.py")
    if not os.path.isfile(enum_path):
        print(f"ALS ELEMENT COVERAGE FAIL — enumerator not found at {enum_path}", file=sys.stderr)
        sys.exit(2)
    spec = importlib.util.spec_from_file_location("enum", enum_path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    elements = set(mod.enumerate_elements(impl_root))

# The resolvable ALS section ids: any `ALS-<id>` token in a heading line of
# the normative docs (the same resolution the contract spec-keying uses).
sections = set()
for p in glob.glob(f"{root}/docs/specs/als/*.md"):
    for line in open(p, encoding="utf-8"):
        if line.startswith("#"):
            sections.update(re.findall(r"ALS-[A-Z]*\d+[a-z]?", line))

ceiling = None
rows, cur = [], None
for raw in open(ledger_path, encoding="utf-8"):
    line = raw.strip()
    m = re.match(r'#\s*unwritten_ceiling\s*=\s*"(\d+)"', line)
    if m:
        ceiling = int(m.group(1))
    if line == "[[element]]":
        if cur:
            rows.append(cur)
        cur = {}
        continue
    m = re.match(r'(\w+)\s*=\s*"(.*)"$', line)
    if m and cur is not None:
        cur[m.group(1)] = m.group(2)
if cur:
    rows.append(cur)

errs = []
if ceiling is None:
    errs.append("ledger header is missing `# unwritten_ceiling = \"N\"`")
seen = set()
unwritten = 0
for r in rows:
    n = r.get("name")
    if not n:
        errs.append(f"row without a name: {r}")
        continue
    if n in seen:
        errs.append(f"{n}: duplicate row")
    seen.add(n)
    if elements is not None and n not in elements:
        errs.append(f"{n}: STALE row — no such variant in the surface AST any more")
    s = r.get("section", "")
    if s == "UNWRITTEN":
        unwritten += 1
    elif s not in sections:
        errs.append(f"{n}: section {s!r} does not resolve to any ALS heading in docs/specs/als/")

if elements is not None:
    for n in sorted(elements - seen):
        errs.append(f"{n}: UNCLASSIFIED — a syntax element with no coverage row. A new "
                    f"element lands with its ALS row (UNWRITTEN is honest) in the same PR.")

if ceiling is not None:
    if unwritten > ceiling:
        errs.append(f"UNWRITTEN count {unwritten} exceeds the ceiling {ceiling}")
    elif unwritten < ceiling:
        errs.append(f"UNWRITTEN count {unwritten} is BELOW the ceiling {ceiling} — ratchet it down")

if errs:
    print("ALS ELEMENT COVERAGE FAIL —", file=sys.stderr)
    for e in errs:
        print(f"  {e}", file=sys.stderr)
    sys.exit(1)
written = len(rows) - unwritten
mode = (f"enumerated against {impl_root}: {len(elements)} AST element(s)" if elements is not None
        else "two-repo mode: element enumeration DEFERRED (no ALS_IMPL_ROOT)")
print(f"als-element-coverage OK: {len(rows)} element(s) — {written} sectioned, "
      f"{unwritten} UNWRITTEN (ceiling {ceiling}; freeze precondition: 0); {mode}")
EOF
