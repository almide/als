# Boundary — what is the judge, what is the implementation

The line is not "documentation versus code". It is **what a compiler is judged
against** (here) versus **how one particular compiler earns the verdict**
(an implementation repository). A path moved here if and only if it would be
unchanged by replacing the compiler with a different correct one.

## Provenance

- Source: `almide/almide`, branch `develop`, commit `53e2a2ab7`
  ("A0-1 sequencing decided: freeze lands after ADR-0012 D2/D3"), 2026-08-20.
- Method: `git filter-repo` over the path list below, on a fresh clone —
  1,321 commits retained, 3,398 files, full history of every listed path at its
  current name. History under former names (before a rename into place) was
  not followed; it remains in the implementation repository.
- Added on top of the extracted history: `README.md`, `CLAUDE.md`, this file,
  `scripts/conformance.py`, the two-repo mode of the two gates,
  `.github/workflows/`, `lefthook.yml`, `docs/specs/CLAUDE.md`.

## Classification

| Path | Side | Why |
|---|---|---|
| `docs/specs/als/` | **judge** | The normative sections contracts cite. |
| `docs/specs/{language,type-system,effect-system,module-system,package-system,result-option-effect,effect-fn-call-semantics,edit-locality}.md` | **judge** | Language specification chapters — true of any correct implementation. |
| `docs/specs/{codegen,perceus,cli}.md` | implementation | Nanopass pipeline, Perceus belt, and this compiler's CLI surface. `cli.md` could become a tooling spec later; today it documents one binary. |
| `docs/SPEC.md`, `docs/GRAMMAR.md` | **judge** | Design thesis and the EBNF. |
| `docs/design/` | **judge** | Equivalence doctrine, hidden-operation doctrine, rejected patterns — cited by the ADRs. |
| `docs/adr/` | **judge** | Language decisions and their falsifiers. |
| `docs/contracts/` | **judge** | The ledger, its prose, its generators, the conformance report, `proven-vs-trusted.md`. |
| `spec/lang`, `spec/stdlib`, `spec/integration` | **judge** | Test-block corpora over observable behaviour. |
| `spec/wasm_cross`, `spec/wasm_cross_pkg`, `spec/wasm_fail`, `spec/programs` | **judge** | Cross-target value and failure-shape fixtures, whole programs. |
| `spec/churn`, `spec/pass_isolated` | implementation | RC-churn fixtures tied to the emitter's drop scheduling; mutation-path fixtures keyed to named codegen functions (`// @pass:`). Four contracts cite `spec/churn` as evidence — resolved with `--impl-root`. |
| `tests/diagnostics/` | **judge** | 752 reject/accept pairs with pinned codes and hints. The rejection surface is part of the language; the Rust harness that runs them stays with the implementation, `scripts/conformance.py` runs them here. |
| `tests/*.rs`, `tests/wasm_runtime_test_parts/` | implementation | Cargo gates (fuzz, Σ-probes, the 3-way oracle). Cited as evidence; deferred here. |
| `proofs/als-element-coverage.toml` | **judge** | Element → ALS-section ledger. The AST enumerator (`scripts/lib/als-element-enumerate.py`) reads `crates/almide-syntax/src/ast.rs` and stays with the implementation; the gate here runs the ledger half, and the full gate with `--impl-root`. |
| `proofs/dialect-epochs.toml` | **judge** | The normative record of dialect epochs. The implementation's `dialect.rs` cross-checks against it and must keep doing so against the pinned copy. |
| `proofs/*.v`, `proofs/checker*`, `proofs/*.sh`, `proofs/*-baseline.txt`, `proofs/releases/`, `proofs/TOR.md`, `proofs/DO330-GAP.md`, `proofs/TRUSTED_BASE.md` | implementation | The proof-carrying-code chain: `ALS.v` imports `OwnershipChecker` and names the *checker's* ownership semantics; the release seals, the tool-operational requirements and the DO-330 argument describe one tool. |
| `scripts/check-contracts.sh`, `scripts/check-als-element-coverage.sh`, `scripts/lib/contract-classes.txt` | **judge** (two-repo mode added) | The traceability gates over the ledger. Copies in the implementation should become calls into the pinned checkout. |
| `scripts/gen-claims.sh` | implementation | Derives the compiler README's claims block from the ledger; README-side artifact. |
| `docs/stdlib/` | **open** | Per-module API reference; `check-semantics-manifest.sh` and `check-interface-diff.sh` generate/gate parts of it from compiler source. Candidate for the judge once the generated and the normative halves are separated. |
| `docs/CHEATSHEET.md` | **open** | The LLM-facing reference; grounds the implementation's misspelling dictionary. Non-normative today. |
| `docs/TRUST-SPINE.md`, `docs/roadmap/`, `docs/ARCHITECTURE.md`, `research/`, `examples/`, `stdlib/`, `crates/`, `runtime/` | implementation | Trust argument, plans, and the compiler itself. |

## Stage B — replacing the copies with a pin

**Executed for the greenfield implementation on 2026-08-20** (almide/almide
branch `greenfield`, commit `b42f30b57`, ratification R6 in its
ARCHITECTURE.md): this repository is mounted as the submodule `als/`, the
copies are deleted, a single indirection crate (`almide-corpus`) resolves
corpus-relative paths across the two roots, the golden generators run the
port-SHA oracle with cwd = the fixture's root, and the contract gate runs from
the mount with `--impl-root`. Two findings from that cutover, recorded for the
next consumer:

- A fixture whose contract postdates an implementation's oracle SHA may have
  NO referee there (the oracle's legs wall or disagree on it). Greenfield
  holds such fixtures out via a shrink-only register
  (`scripts/lib/run-oracle-exclusions.txt` in its tree) rather than judging
  against a non-observation.
- A pin advance may legitimately bring new implementation-evidence forward
  references and ceiling raises; the rule adopted is: raise by exactly what
  the advance brings, each named in the port log — outside a pin advance,
  ratchets only go down.

**The incumbent (`develop`) still carries copies** — its cutover is a
separately decided step. What it must do:

1. Mount this repository at a pinned commit (a submodule, or a fetch step in
   CI — `actions/checkout` does not fetch submodules by default, the
   implementation's CI already notes this for `grammar/`).
2. Point the cited paths at the mount: either a symlink per judge root
   (`spec/wasm_cross → als/spec/wasm_cross`, …) or path constants in the Rust
   harnesses (`tests/wasm_runtime_test_parts/p4_corpus.rs`,
   `tests/diagnostic_harness_test.rs`, `tests/wasm_runtime_test_parts/p6_fail_corpus.rs`).
3. Run `scripts/check-contracts.sh --impl-root .` and
   `scripts/check-als-element-coverage.sh --impl-root .` from the mount instead
   of the local copies; `gen-claims.sh` reads the ledger from the mount.
4. `dialect.rs` / `check-dialect-epochs.sh` read `proofs/dialect-epochs.toml`
   from the mount.
5. Delete the copies. The greenfield branch's `scripts/lib/port-deviations.txt`
   (its deviation register against the pinned ledger) becomes an ordinary
   *conformance exception declaration* — both implementations declare their
   unmet evidence in the same form, shrink-only.
6. The fixture added by an in-flight implementation PR becomes a PR here first.

Stage B deletes >1,000 files from the implementation's working tree and
changes every in-flight fixture PR's destination; it is executed only on an
explicit decision, after this repository's `gates` workflow is green and the
pin mechanism has been exercised on one implementation.
