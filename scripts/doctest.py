#!/usr/bin/env python3
"""Spec doctest — a specification cannot quote an example that does not compile.

    scripts/doctest.py --almide <bin> [--report out.toml]

Judges every fenced code block in the normative chapters (docs/specs/**/*.md,
excluding README/STANDARD/CLAUDE):

  ```almide                 must pass `almide check` standalone (the
                            docs/specs/CLAUDE.md claim, now enforced)
  ```almide check-fail=ENNN must be REJECTED with that code — negative
                            examples are assertions too
  ```almide fragment        not standalone; counted against a shrink-only
                            ceiling below — honest debt, burned down by
                            giving examples their missing context
  ```<other-lang> / ```     out of judgment; bare ``` fences are counted
                            against their own shrink-only ceiling (every
                            fence should declare what it is)

Ceilings (four-direction law, STANDARD.md): the counts may not grow; a count
below its ceiling demands the ceiling come down in the same change; zero
almide blocks measured is a failure, not a pass.
"""
import argparse, glob, os, re, subprocess, sys, tempfile

FRAGMENT_CEILING = 21   # measured 2026-08-20 at introduction; shrink-only
UNTAGGED_CEILING = 163  # bare ``` fences in normative chapters; shrink-only

def blocks():
    for f in sorted(glob.glob("docs/specs/**/*.md", recursive=True)):
        if f.endswith(("README.md", "STANDARD.md", "CLAUDE.md")):
            continue
        src = open(f, encoding="utf-8").read()
        for m in re.finditer(r'```([^\n]*)\n(.*?)```', src, re.S):
            yield f, src[:m.start()].count("\n") + 1, m.group(1).strip(), m.group(2)

def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--almide", required=True)
    args = ap.parse_args()
    os.chdir(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
    if subprocess.run([args.almide, "--version"], capture_output=True).returncode != 0:
        print(f"cannot execute {args.almide}", file=sys.stderr); return 2
    n_pass = n_fragment = n_untagged = 0
    failures = []
    for f, line, info, body in blocks():
        where = f"{f}:{line}"
        if info == "":
            n_untagged += 1; continue
        parts = info.split()
        if parts[0] != "almide":
            continue
        mode = parts[1] if len(parts) > 1 else "check"
        if mode == "fragment":
            n_fragment += 1; continue
        with tempfile.NamedTemporaryFile("w", suffix=".almd", delete=False) as t:
            t.write(body); path = t.name
        r = subprocess.run([args.almide, "check", path], capture_output=True, text=True, timeout=120)
        os.unlink(path)
        combined = r.stdout + r.stderr
        if mode == "check":
            if r.returncode == 0: n_pass += 1
            else: failures.append(f"{where}: example does not compile\n  {combined.splitlines()[0] if combined.splitlines() else '?'}")
        elif mode.startswith("check-fail="):
            code = mode.split("=", 1)[1]
            if r.returncode != 0 and f"[{code}]" in combined: n_pass += 1
            else: failures.append(f"{where}: negative example must be rejected with [{code}] (exit={r.returncode})")
        else:
            failures.append(f"{where}: unknown doctest mode {mode!r} — the vocabulary is closed")
    judged = n_pass + len(failures)
    if judged + n_fragment == 0:
        print("::error::zero almide blocks measured — the instrument is broken, not the spec clean"); return 1
    for msg in failures:
        print(f"::error::{msg}")
    ok = not failures
    for name, count, ceiling in (("fragment", n_fragment, FRAGMENT_CEILING), ("untagged-fence", n_untagged, UNTAGGED_CEILING)):
        if count > ceiling:
            print(f"::error::{name} count {count} exceeds the shrink-only ceiling {ceiling}"); ok = False
        elif count < ceiling:
            print(f"::error::{name} count {count} is BELOW the ceiling {ceiling} — ratchet it down in this change"); ok = False
    print(f"doctest: {n_pass}/{judged} judged examples hold; fragments {n_fragment}/{FRAGMENT_CEILING}, untagged fences {n_untagged}/{UNTAGGED_CEILING} (both shrink-only)")
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
