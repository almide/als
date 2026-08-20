# ADR-0015: The reference evaluator is a fresh, source-level, judge-owned evaluator behind a black-box protocol — seeded by λ_almd, ratcheted by an abstain ledger

- **Status**: Proposed
- **Date**: 2026-08-20
- **Context**: QUALIFICATION.md limitation 2 — the cross legs judge
  AGREEMENT, not truth; both targets being wrong identically is invisible.
  [#10](https://github.com/almide/als/issues/10) names the instrument that
  closes it: a reference evaluator owned by this repository and a `ref` leg
  in the runner whose verdict is *legs == ref*. What exists today, measured:
  - The corpus the leg must judge: `spec/wasm_cross` 592 fixtures,
    `spec/programs`, `spec/wasm_cross_pkg`; they call **478 distinct stdlib
    functions** (`module.fn`, top modules int/list/string/bytes/value/float/
    regex/map/json/fs/set/fan).
  - The only mechanized semantics is the λ_almd kernel (C-280, the
    edit-locality belt, Lean 4.29, 1,583 lines, no Mathlib): **12 expression
    forms** (int/string literals, var, let, single-arg call, ok/err, match on
    Result, `!`, `??`, print, seq), a fuel-indexed evaluator proven sound
    against the relation, and a generated corpus of **48 programs whose
    traces are evaluator-pinned** (`proofs/kernel-conformance/` in the
    implementation repository) plus the surface fixture
    `spec/wasm_cross/kernel_conformance.almd`.
  - The implementation's third oracle, `crates/almide-interp`, is an IR
    interpreter (10.8k lines) sharing parser, checker and lowering with the
    compiler — a front-half defect moves all three legs identically.
  - The judge's toolchain is Python 3 + bash; `gates` has no compiler,
    no cargo, no Lean. `tree-sitter-almide` is a hand-maintained mirror of
    the compiler's parser (1,050-line grammar.js, Rust bindings only, drift
    gated weekly on the implementation side).
  - ADR-0014 already commits the shape of the output: pinned expectations
    are derived by an instrument; the Roc-form traces for `spec/wasm_cross`
    appear only as the reference evaluator's output.
- **Decision**:
  1. **Fresh, from the normative text.** The evaluator is written from the
     ALS chapters (`docs/specs/als/`, 110 sections), the language grammar
     (the EBNF the implementation publishes as `docs/GRAMMAR.md`, carried
     here as the judge's parser specification) and the stdlib
     specifications — never by porting `almide-interp`'s evaluation rules.
     A port would copy the implementation's semantics and keep the co-drift
     it is meant to catch; a fresh reading finds the spec's gaps, exactly as
     the doctest burn-down did.
  2. **Source-level, own front end, abstract values.** It parses `.almd`
     source with its own recursive-descent parser (no compiler crate, no
     tree-sitter build: a third grammar would add its own drift and a native
     build step to a repository whose gates are Python). Values are abstract
     (Int as unbounded-then-wrapped per ALS, String, List, Map, Set, Tuple,
     Record, Variant, Option, Result, Fn) — no byte heap: the judge models
     *what* a program observes, not *how* the implementation lays it out.
  3. **Python, behind a black-box protocol.** The first evaluator is Python
     (the judge's existing toolchain; the corpus is small programs and
     wall-clock is bounded by a ratchet, see Consequences). The runner never
     imports it: it calls a REF PROTOCOL — `<ref> run <file.almd> --json`
     emits `{"exit": n, "stdout": s, "stderr": s}` or
     `{"abstain": {"class": c, "reason": r}}` — so any evaluator in any
     language (a Rust port if Python proves too slow, the compiled Lean
     evaluator on its fragment) is pluggable, and the judge's seam is the
     protocol, not the implementation.
  4. **Seeded by λ_almd, 100% from day one.** The 48 kernel programs and
     `kernel_conformance.almd` are the first oracle: the evaluator must
     reproduce their evaluator-pinned traces byte-for-byte before it judges
     anything else, and that agreement is a floor of 1.0 that CI holds
     forever (the "λ_almd-agreement fraction" of #10, with the fragment gate
     as its seed; it can only grow as the kernel grows).
  5. **Honest by abstaining.** Anything the evaluator does not implement is
     an ABSTAIN with a class (`stdlib:<module.fn>`, `syntax:<form>`,
     `runtime:<capability>`), recorded per fixture in
     `proofs/ref-abstain.toml`, shrink-only and classified — an abstain is
     never a pass and never a fail. Silent fallback is forbidden.
  6. **The leg's verdict is *legs == ref*.** For a fixture the evaluator
     evaluates, native and wasm must each equal the reference observables
     (exit, stdout, stderr, trimmed as in `cross`); agreement between the
     two targets is no longer sufficient. For an abstained fixture the
     existing agreement verdict stands and the abstain is counted. Each
     contract gains a reference-backed attribute (all its fixtures
     evaluated), printed by the README generator as a grow-only fraction.
- **Rationale**: The value of a reference is its independence, and
  independence here is achievable in exactly one dimension at a time:
  independent FRONT END (a second parser from the grammar), independent
  SEMANTICS (a second reading of the ALS text), independent STDLIB (a second
  implementation from the stdlib spec). Each of the rejected alternatives
  gives one of those up for convenience. Python is the cheapest host for a
  tree-walker over small programs and keeps the judge installable with
  `python3` alone; the protocol makes the language choice reversible at the
  cost of a port, not a redesign. Starting from the kernel corpus means the
  first 49 programs the evaluator must get right are the ones whose
  expected traces are *proven* rather than agreed — the only place in either
  repository where truth, not agreement, is available today.
- **Alternatives**:
  (a) *Extend the λ_almd belt in Lean until it covers the surface* —
  rejected as the executable leg: surface-wide mechanization is research-
  grade (#10); the belt stays the mechanized seed and the agreement ratchet
  relates the two. (b) *Port `almide-interp`'s rules behind a fresh parser* —
  rejected: copies the semantics under test. (c) *tree-sitter-almide as the
  front end* — rejected: a third hand-maintained grammar with its own drift,
  a native build in a Python-gated repository, and a parser that accepts
  what the compiler rejects. (d) *Rust from the start* — rejected for the
  first cut: a cargo toolchain in `gates` and a slower write/measure loop;
  the protocol keeps it open. (e) *Judge only a hand-picked subset* —
  rejected: the abstain ledger judges everything and says what it skipped.
- **Falsifiers**: (1) the evaluator's wall-clock over the corpus exceeds the
  cross leg's own — then the Rust port behind the same protocol; (2) the
  abstain ledger stops shrinking for two releases — then the stdlib-spec
  gap, not the evaluator, is the blocker and goes to #10 as a chapter debt;
  (3) a kernel-agreement regression — the evaluator is wrong by definition
  (the kernel is proven), never the other way.
- **Consequences**: `scripts/ref/` (parser, evaluator, stdlib) + `scripts/ref.py`
  (the protocol CLI); `scripts/conformance.py --legs ref` with a `--ref`
  command; `selftest-conformance.py` scenarios for every ref verdict class
  (ref pass, native ≠ ref, wasm ≠ ref, both ≠ ref while agreeing — the
  co-drift class — abstain counted, malformed protocol output is red); a
  row in `proofs/gate-verification.toml`; `proofs/ref-abstain.toml`
  shrink-only; the kernel corpus carried here under `proofs/kernel-
  conformance/` with the generator commit recorded (regenerated never
  edited, ADR-0014). The Roc-form `.expected` traces for `spec/wasm_cross`
  become possible only after the abstain set is empty for a fixture — ADR-
  0014's condition, now with a named instrument.
