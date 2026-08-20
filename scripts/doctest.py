#!/usr/bin/env python3
"""Spec doctest — a specification cannot quote an example that does not compile.

    scripts/doctest.py --almide <bin>            judge docs/specs/**/*.md
    scripts/doctest.py --almide <bin> --selftest  prove the judge turns red

Judges every fenced code block in the normative chapters (docs/specs/**/*.md,
excluding README/STANDARD/CLAUDE). The fence vocabulary is CLOSED:

  ```almide                 a complete file: `almide test` must pass — it
                            compiles the block AND executes every `test`
                            block in it, so an example that asserts is an
                            example that has been run (the docs/specs/CLAUDE.md
                            claim "compiles as-is when pasted", enforced and
                            then some)
  ```almide check-fail=ENNN must be REJECTED by `almide check` with that code —
                            negative examples are assertions too
  ```almide project         a multi-file example: `// file: <relpath>` lines
                            split the body into files materialized in a fresh
                            directory (module dirs, almide.toml, …); every
                            .almd file must pass `almide check`, then
                            `almide test <dir>` runs whatever tests exist
  ```almide project check-fail=ENNN
                            multi-file negative: the LAST file is the consumer
                            and `almide check` on it must fail with the code
  ```almide fragment        not standalone; counted against a shrink-only
                            ceiling below — honest debt, burned down by
                            giving examples their missing context
  ```<other-lang> / ```     out of judgment (text, ebnf, toml, bash, rust…);
                            bare ``` fences are counted against their own
                            shrink-only ceiling (every fence should declare
                            what it is); the other-language tally is printed
                            so a migration of Almide code INTO `text` would
                            be visible in the summary line

Ceilings (four-direction law, STANDARD.md): the counts may not grow; a count
below its ceiling demands the ceiling come down in the same change; zero
almide blocks measured is a failure, not a pass.

`almide test` runs the blocks through wasmtime when it is on PATH (the
implementation's CI pin) and falls back to native otherwise — the verdict is
the same, only the wall-clock differs.
"""
import argparse, collections, glob, os, re, shutil, subprocess, sys, tempfile

FRAGMENT_CEILING = 21   # measured 2026-08-20 at introduction; shrink-only
UNTAGGED_CEILING = 163  # bare ``` fences in normative chapters; shrink-only

FILE_MARK = re.compile(r'^// file: (\S+)\s*$')
FAILED_RE = re.compile(r'(\d+) failed \(of (\d+) files\)')


def blocks(root):
    for f in sorted(glob.glob(os.path.join(root, "docs/specs/**/*.md"), recursive=True)):
        if f.endswith(("README.md", "STANDARD.md", "CLAUDE.md")):
            continue
        src = open(f, encoding="utf-8").read()
        for m in re.finditer(r'```([^\n]*)\n(.*?)```', src, re.S):
            yield os.path.relpath(f, root), src[:m.start()].count("\n") + 1, m.group(1).strip(), m.group(2)


def run(cmd, cwd=None):
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=300, cwd=cwd)
    return r.returncode, r.stdout + r.stderr


def first_line(out):
    lines = [l for l in out.splitlines() if l.strip()]
    return lines[0] if lines else "?"


def judge_single(almide, body, mode):
    """Return None when the example holds, else a reason string."""
    d = tempfile.mkdtemp(prefix="doctest-")
    try:
        path = os.path.join(d, "example.almd")
        open(path, "w", encoding="utf-8").write(body)
        if mode == "check":
            rc, out = run([almide, "test", path])
            m = FAILED_RE.search(out)
            failed = int(m.group(1)) if m else None
            if rc != 0 or failed is None or failed > 0:
                return f"example does not hold under `almide test`\n  {first_line(out)}"
            return None
        code = mode.split("=", 1)[1]
        rc, out = run([almide, "check", path])
        if rc != 0 and f"[{code}]" in out:
            return None
        return f"negative example must be rejected with [{code}] (exit={rc})"
    finally:
        shutil.rmtree(d, ignore_errors=True)


def split_project(body):
    """`// file: path` separators → ordered [(path, content)]; raises ValueError."""
    files, cur, buf = [], None, []
    for line in body.splitlines():
        m = FILE_MARK.match(line)
        if m:
            if cur is not None:
                files.append((cur, "\n".join(buf) + "\n"))
            cur, buf = m.group(1), []
            if cur.startswith("/") or ".." in cur.split("/"):
                raise ValueError(f"project file path must be relative and inside the project: {cur}")
        elif cur is None:
            if line.strip():
                raise ValueError("project block must begin with a `// file: <path>` line")
        else:
            buf.append(line)
    if cur is not None:
        files.append((cur, "\n".join(buf) + "\n"))
    if not files:
        raise ValueError("project block names no files")
    return files


def judge_project(almide, body, mode):
    try:
        files = split_project(body)
    except ValueError as e:
        return str(e)
    d = tempfile.mkdtemp(prefix="doctest-project-")
    try:
        for rel, content in files:
            p = os.path.join(d, rel)
            os.makedirs(os.path.dirname(p), exist_ok=True)
            open(p, "w", encoding="utf-8").write(content)
        almd = [rel for rel, _ in files if rel.endswith(".almd")]
        if not almd:
            return "project block has no .almd file"
        if mode.startswith("check-fail="):
            code = mode.split("=", 1)[1]
            rc, out = run([almide, "check", os.path.join(d, almd[-1])])
            if rc != 0 and f"[{code}]" in out:
                return None
            return f"negative project example: `{almd[-1]}` must be rejected with [{code}] (exit={rc})"
        for rel in almd:
            rc, out = run([almide, "check", os.path.join(d, rel)])
            if rc != 0:
                return f"project file `{rel}` does not compile\n  {first_line(out)}"
        rc, out = run([almide, "test", d])
        m = FAILED_RE.search(out)
        failed = int(m.group(1)) if m else None
        if rc != 0 or failed is None or failed > 0:
            return f"project tests do not hold under `almide test`\n  {first_line(out)}"
        return None
    finally:
        shutil.rmtree(d, ignore_errors=True)


def judge(root, almide, fragment_ceiling, untagged_ceiling):
    """Run the doctest over <root>/docs/specs. Returns (ok, summary_line, errors)."""
    n_pass = n_fragment = n_untagged = 0
    other = collections.Counter()
    failures = []
    for f, line, info, body in blocks(root):
        where = f"{f}:{line}"
        if info == "":
            n_untagged += 1; continue
        parts = info.split()
        if parts[0] != "almide":
            other[parts[0]] += 1; continue
        rest = parts[1:]
        if rest == ["fragment"]:
            n_fragment += 1; continue
        if rest and rest[0] == "project":
            sub = rest[1:]
            mode = "check" if not sub else sub[0]
            if len(sub) > 1 or (mode != "check" and not mode.startswith("check-fail=")):
                failures.append(f"{where}: unknown doctest mode {info!r} — the vocabulary is closed"); continue
            why = judge_project(almide, body, mode)
        else:
            mode = rest[0] if rest else "check"
            if len(rest) > 1 or (mode != "check" and not mode.startswith("check-fail=")):
                failures.append(f"{where}: unknown doctest mode {info!r} — the vocabulary is closed"); continue
            why = judge_single(almide, body, mode)
        if why is None:
            n_pass += 1
        else:
            failures.append(f"{where}: {why}")
    judged = n_pass + len(failures)
    errors = []
    if judged + n_fragment == 0:
        errors.append("zero almide blocks measured — the instrument is broken, not the spec clean")
    errors.extend(failures)
    for name, count, ceiling in (("fragment", n_fragment, fragment_ceiling), ("untagged-fence", n_untagged, untagged_ceiling)):
        if count > ceiling:
            errors.append(f"{name} count {count} exceeds the shrink-only ceiling {ceiling}")
        elif count < ceiling:
            errors.append(f"{name} count {count} is BELOW the ceiling {ceiling} — ratchet it down in this change")
    others = ", ".join(f"{k} {v}" for k, v in sorted(other.items())) or "none"
    summary = (f"doctest: {n_pass}/{judged} judged examples hold; fragments {n_fragment}/{fragment_ceiling}, "
               f"untagged fences {n_untagged}/{untagged_ceiling} (both shrink-only); other-language fences: {others}")
    return (not errors), summary, errors


# ── self-test: every verdict class must turn red for its reason ──────────
SELFTEST_CASES = [
    # (name, fence info, body, expect_red, must_mention)
    ("clean almide block holds",
     "almide", 'fn add(a: Int, b: Int) -> Int = a + b\ntest "adds" {\n  assert_eq(add(1, 2), 3)\n}\n', False, None),
    ("syntactically broken almide block is red",
     "almide", "fn broken( -> Int = 1\n", True, "does not hold"),
    ("a failing assertion is red (the block is RUN, not only checked)",
     "almide", 'test "lies" {\n  assert_eq(1, 2)\n}\n', True, "does not hold"),
    ("negative example with the right code holds",
     "almide check-fail=E003", "fn f() -> Int = undefined_name\n", False, None),
    ("negative example that compiles is red",
     "almide check-fail=E003", "fn f() -> Int = 1\n", True, "must be rejected"),
    ("negative example rejected with a DIFFERENT code is red",
     "almide check-fail=E999", "fn f() -> Int = undefined_name\n", True, "must be rejected"),
    ("project example with a sibling module holds",
     "almide project", '// file: lib/mod.almd\nfn hello() -> String = "hi"\n// file: main.almd\nimport lib\ntest "calls" {\n  assert_eq(lib.hello(), "hi")\n}\n', False, None),
    ("project example whose test lies is red",
     "almide project", '// file: lib/mod.almd\nfn hello() -> String = "hi"\n// file: main.almd\nimport lib\ntest "calls" {\n  assert_eq(lib.hello(), "bye")\n}\n', True, "do not hold"),
    ("project example with a broken module file is red",
     "almide project", '// file: lib/mod.almd\nfn hello( -> String = "hi"\n// file: main.almd\nimport lib\nfn f() -> String = lib.hello()\n', True, "does not compile"),
    ("project negative example (phantom import) with the right code holds",
     "almide project check-fail=E003", '// file: d/mod.almd\nfn shared() -> String = "d"\n// file: b/mod.almd\nimport d\nfn from_b() -> String = d.shared()\n// file: main.almd\nimport b\nfn f() -> String = d.shared()\n', False, None),
    ("project negative example that compiles is red",
     "almide project check-fail=E003", '// file: d/mod.almd\nfn shared() -> String = "d"\n// file: main.almd\nimport d\nfn f() -> String = d.shared()\n', True, "must be rejected"),
    ("project block without a leading // file: line is red",
     "almide project", 'fn f() -> Int = 1\n', True, "must begin with"),
    ("project file path escaping the directory is red",
     "almide project", '// file: ../escape.almd\nfn f() -> Int = 1\n', True, "must be relative"),
    ("unknown mode is red (closed vocabulary)",
     "almide run", "fn f() -> Int = 1\n", True, "closed"),
    ("fragment is counted, not judged",
     "almide fragment", "this is not almide at all\n", False, None),
]


def selftest(almide):
    failures = []
    for name, info, body, expect_red, must_mention in SELFTEST_CASES:
        root = tempfile.mkdtemp(prefix="doctest-selftest-")
        try:
            os.makedirs(os.path.join(root, "docs/specs"))
            n_frag = 1 if info == "almide fragment" else 0
            open(os.path.join(root, "docs/specs/case.md"), "w", encoding="utf-8").write(
                f"# case\n\n```{info}\n{body}```\n")
            ok, _, errors = judge(root, almide, fragment_ceiling=n_frag, untagged_ceiling=0)
            red = not ok
            if red != expect_red:
                failures.append(f"{name}: expected {'red' if expect_red else 'green'}, got {'red' if red else 'green'} — {errors[:1]}")
            elif expect_red and must_mention and not any(must_mention in e for e in errors):
                failures.append(f"{name}: red for the WRONG reason — {errors[:1]} lacks {must_mention!r}")
            else:
                print(f"  ok   {name}")
        finally:
            shutil.rmtree(root, ignore_errors=True)
    # the instrument-death guard and the four-direction ceiling law
    root = tempfile.mkdtemp(prefix="doctest-selftest-")
    try:
        os.makedirs(os.path.join(root, "docs/specs"))
        open(os.path.join(root, "docs/specs/case.md"), "w", encoding="utf-8").write("# case\n\n```\nbare fence\n```\n")
        ok, _, errors = judge(root, almide, fragment_ceiling=0, untagged_ceiling=1)
        if ok or not any("zero almide blocks" in e for e in errors):
            failures.append(f"zero almide blocks must be red: {errors}")
        else:
            print("  ok   zero almide blocks measured is red, not a clean pass")
        open(os.path.join(root, "docs/specs/case.md"), "a", encoding="utf-8").write("\n```almide\nfn f() -> Int = 1\n```\n")
        for ceil, word in ((0, "exceeds"), (2, "BELOW")):
            ok, _, errors = judge(root, almide, fragment_ceiling=0, untagged_ceiling=ceil)
            if ok or not any(word in e for e in errors):
                failures.append(f"untagged ceiling {ceil} must be red ({word}): {errors}")
            else:
                print(f"  ok   untagged count 1 against ceiling {ceil} is red ({word})")
        ok, _, errors = judge(root, almide, fragment_ceiling=0, untagged_ceiling=1)
        if not ok:
            failures.append(f"exact ceiling must be green: {errors}")
        else:
            print("  ok   untagged count 1 against ceiling 1 is green")
    finally:
        shutil.rmtree(root, ignore_errors=True)
    for f in failures:
        print(f"::error::selftest: {f}")
    if failures:
        return 1
    print(f"doctest selftest OK: {len(SELFTEST_CASES) + 4} scenarios — every verdict class turns red for its reason and green only when the example holds")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--almide", required=True)
    ap.add_argument("--selftest", action="store_true", help="prove the judge turns red for every verdict class, then exit")
    args = ap.parse_args()
    root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
    if subprocess.run([args.almide, "--version"], capture_output=True).returncode != 0:
        print(f"cannot execute {args.almide}", file=sys.stderr); return 2
    if args.selftest:
        return selftest(args.almide)
    ok, summary, errors = judge(root, args.almide, FRAGMENT_CEILING, UNTAGGED_CEILING)
    for e in errors:
        print(f"::error::{e}")
    print(summary)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
