#!/usr/bin/env bash
# LINK GATE — every relative link in every Markdown file must resolve.
#
# A specification's cross-references ARE part of its quality: a dead link in
# normative text sends the reader to nothing, and the extraction from the
# implementation repo left 21 of them (all pointing at implementation files;
# repaired to stable GitHub URLs in the same commit that added this gate).
# Code fences and inline code are masked first — `[x](y)` inside an example
# is code, not a link. Anchors into Markdown targets are checked against the
# target's headings (GitHub slugger rules, ASCII case-fold).
set -uo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.." || exit 2
python3 - <<'PY'
import re, os, glob, sys
def strip_code(text):
    t = re.sub(r'```.*?```', lambda m: '\x00' * len(m.group(0)), text, flags=re.S)
    return re.sub(r'`[^`\n]*`', lambda m: '\x00' * len(m.group(0)), t)
def slug(h):
    h = re.sub(r'[!"#$%&\'()*+,./:;<=>?@\[\]^{|}~`]', '', h.strip().lower())
    return re.sub(r'\s', '-', h)
def anchors(path):
    out = set()
    for line in open(path, encoding='utf-8'):
        m = re.match(r'#{1,6}\s+(.*)', line)
        if m: out.add(slug(m.group(1)))
    return out
errs = []
for p in sorted(glob.glob("**/*.md", recursive=True)):
    for m in re.finditer(r'\]\(([^)\s]+?)(#[^)]*)?\)', strip_code(open(p, encoding='utf-8').read())):
        t, frag = m.group(1), m.group(2)
        if t.startswith(("http://", "https://", "mailto:")): continue
        tp = os.path.normpath(os.path.join(os.path.dirname(p), t))
        if not os.path.exists(tp):
            errs.append(f"{p}: dead link -> {t}")
        elif frag and tp.endswith(".md") and slug(frag[1:]) not in anchors(tp):
            errs.append(f"{p}: dead anchor -> {t}{frag}")
n = 0
for e in errs:
    print(f"::error::{e}"); n += 1
if n: sys.exit(1)
print("links: every relative link and anchor resolves.")
PY
