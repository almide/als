#!/usr/bin/env bash
# REF-INDEPENDENCE GATE — the reference evaluator is structurally what
# ADR-0015 says it is (clauses 1, 4, 5, 6 made checkable):
#   - a standalone crate: `cargo tree` names no almide-* crate and no
#     path/git dependency into the implementation repository;
#   - stable toolchain only: rust-toolchain.toml pins a release channel and
#     no source file enables a nightly feature;
#   - the clippy clauses hold: forbidden host types (HashMap/HashSet) and
#     forbidden host methods (the std string/number/sort operations whose
#     behaviour the ALS specifies) do not appear — `cargo clippy -D warnings`
#     with ref/clippy.toml;
#   - the float newtype carries no Display impl (a host `{}` cannot render
#     an Almide float);
#   - rustfmt is clean (one shape, reviewable diffs).
# Exit 0 = all hold; 1 = a violation (named); 2 = environment.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/ref"
fail=0
err() { echo "::error::$*"; fail=1; }

command -v cargo >/dev/null || { echo "::error::cargo not found"; exit 2; }

# clause 6 — independence
if cargo tree --quiet 2>/dev/null | grep -E 'almide-[a-z]' ; then
  err "ref/ depends on an almide-* crate (ADR-0015 clause 6)"
fi
deps="$(awk '/^\[dependencies\]/{f=1;next} /^\[/{f=0} f && NF' Cargo.toml | grep -v '^#' || true)"
if echo "$deps" | grep -qE '(path|git)\s*='; then
  err "ref/Cargo.toml carries a path/git dependency (ADR-0015 clause 6)"
fi
if [ -n "$deps" ]; then
  echo "note: ref/ has external dependencies:"; echo "$deps"
  echo "::warning::ref/ is no longer dependency-free — gates.yml's header must say so (network during build)"
fi

# clause 4 — pinned stable channel, no nightly features
if ! grep -qE '^channel = "[0-9]+\.[0-9]+\.[0-9]+"' rust-toolchain.toml; then
  err "rust-toolchain.toml does not pin an exact stable release (ADR-0015 clause 4)"
fi
if grep -rnE '#!\[feature\(' src/ ; then
  err "a nightly feature is enabled in ref/src (ADR-0015 clause 4)"
fi

# clause 1 + 5 — the clippy clauses (clippy.toml disallowed-types / disallowed-methods)
if ! cargo clippy --quiet -- -D warnings 2>clippy.err; then
  cat clippy.err; rm -f clippy.err
  err "cargo clippy -D warnings failed — a forbidden host type/method or a lint (ADR-0015 clauses 1/5)"
else
  rm -f clippy.err
fi
if ! grep -q 'disallowed-types' clippy.toml || ! grep -q 'disallowed-methods' clippy.toml; then
  err "ref/clippy.toml lost its disallowed-types / disallowed-methods clauses"
fi

# the float newtype must not implement Display (structural clause 5)
if grep -nE 'impl[^{]*Display[^{]*for F64' src/*.rs; then
  err "F64 implements Display — a host float formatting could leak (ADR-0015 clause 5)"
fi

# one shape
if ! cargo fmt --check --quiet 2>/dev/null; then
  err "cargo fmt --check: ref/ is not rustfmt-clean"
fi

if [ "$fail" -ne 0 ]; then
  echo "ref-independence FAILED"; exit 1
fi
echo "ref-independence OK: no almide-* dependency, stable channel $(grep -oE '"[0-9.]+"' rust-toolchain.toml | head -1), no nightly feature, clippy clauses hold, F64 has no Display, rustfmt clean."
