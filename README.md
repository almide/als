# ALS — the Almide Language Specification

**What an Almide implementation is judged against.** This repository holds the
language's normative semantics, its behavior-contract ledger, the executable
conformance corpus, the traceability gates that bind those three together, and
a runner that executes the corpus against *any* `almide` binary. It holds no
compiler.

Almide's mission is to be the language LLMs write most accurately, and its
declared quality bar is aviation-grade (DO-178C-class). At that bar the
requirements and the verification evidence are configuration-managed
**independently** of the implementation, with two-way traceability between
them; a requirement is reviewed before the code that satisfies it, and the
verification suite must be runnable by someone who did not write the compiler.
This repository is that independence made structural. It is to
[almide/almide](https://github.com/almide/almide) what
`ferrocene/specification` is to the Ferrocene compiler, what `seL4/l4v` is to
the seL4 kernel, and what `WebAssembly/spec` is to every wasm engine.

The Almide organisation already has one external judge —
[almide/almide-dojo](https://github.com/almide/almide-dojo) measures LLM
writability (modification survival rate) from outside the compiler. ALS is the
second judge: correctness.

## What is here

| Path | Role |
|---|---|
| `docs/specs/als/` | **Normative semantics** (`## ALS-<id>` sections). Every section is cited by ≥1 contract; every contract cites a section — both directions gated. |
| `docs/specs/*.md` | Language specification chapters: language, type system, effect system, modules, packages, Result/Option/effect, effect-fn call semantics, edit locality. |
| `docs/SPEC.md`, `docs/GRAMMAR.md`, `docs/design/` | Design thesis, EBNF grammar, design doctrine (equivalence, hidden operations, rejected patterns). |
| `docs/adr/` | Architecture Decision Records — the *why* behind the language decisions, with falsifiers. |
| `docs/contracts/contracts.toml` | **Behavior-contract ledger**: 301 named `C-NNN` promises (stdout, stderr, exit code — identical on every target). `README.md` and `conformance.md` next to it are generated from it. |
| `spec/wasm_cross/` | 591 cross-target fixtures, each declaring the contract(s) it certifies on a `// @contract:` header. The ledger ↔ fixture link is symmetric and gated. |
| `spec/lang/`, `spec/stdlib/`, `spec/integration/` | Test-block corpora (`almide test`) — language, stdlib, multi-module. |
| `spec/wasm_fail/`, `spec/programs/`, `spec/wasm_cross_pkg/` | Failure-shape fixtures (`// @expect-fail:`), whole programs, the package-form cross-target fixture. |
| `tests/diagnostics/` | 752 diagnostic cases (`broken.almd` must be rejected with the pinned code/hint; `fixed.almd` must compile). Rejection behaviour is part of the language surface. |
| `proofs/als-element-coverage.toml` | Every surface-syntax element → the ALS section that specifies it (72/72 sectioned, 0 UNWRITTEN — a freeze precondition). |
| `proofs/dialect-epochs.toml` | The dialect-epoch record: what each epoch added, deprecated, removed. |
| `scripts/check-contracts.sh` | Contract-ledger traceability gate (schema, evidence floor, symmetric links, spec keying, generated-doc freshness, retired-path citations). |
| `scripts/check-als-element-coverage.sh` | Element-coverage gate (ledger side here; AST enumeration with an implementation root). |
| `scripts/conformance.py` | **The runner.** Executes the corpus against a binary and writes a conformance statement. |
| `BOUNDARY.md` | The classification of every path that was and was not moved here, with rationale, and the provenance of the extraction. |

Paths are kept **verbatim** from the implementation's layout on purpose: the
ledger, the fixtures, the ALS prose and the implementation's own gates all
cite them, and a pinned checkout of this repository must be diffable against
the copies the implementations still carry.

## How an implementation uses this repository

1. **Pin** a commit (later: a tag) of `almide/als`.
2. **Run the gates with an implementation root** so implementation-resident
   evidence (cargo gates, Lean theorems, `proofs/*.v`) is required to exist:
   ```bash
   bash scripts/check-contracts.sh            --impl-root /path/to/almide
   bash scripts/check-als-element-coverage.sh --impl-root /path/to/almide
   ```
   Without a root, those paths are counted and reported as *deferred* — never
   silently passed, never falsely red. Judge-resident evidence is required
   unconditionally.
3. **Run the judge** against the built binary (wasmtime on `PATH`):
   ```bash
   python3 scripts/conformance.py --almide /path/to/almide --report conformance.toml
   ```
   The statement records the ALS commit, the binary's version, the platform,
   the legs and any `--limit`, and per-leg counts with every failure verbatim.
   A verdict is only as wide as what the statement says was run.

## Change discipline (requirements first)

- **A behaviour change lands here before it lands in an implementation.** New
  behaviour = a new `C-NNN` contract citing its `ALS-<id>` section + ≥1 fixture
  declaring it, in one PR to this repository. The implementation then bumps its
  pin and makes the judge pass — a second, separately reviewed PR. The git
  history of the two repositories is the evidence that requirements preceded
  code.
- Fixture headers: `// @contract: C-NNN[, C-MMM]` (mandatory in
  `spec/wasm_cross`), `// @xt-allow: <reason + ref>` (a known, tracked
  divergence — logged, and flagged *stale* the moment it heals),
  `// @expect-fail: <stderr substring>` and `// @xf-allow:` in `spec/wasm_fail`.
- Contract ids are contiguous; `since` is the release the *behaviour* became
  normative; the `flagged-for-revision` count is a ratchet that may only go
  down (current ceiling: 0).
- Derived documents are regenerated, never edited:
  `bash docs/contracts/generate-readme.sh > docs/contracts/README.md` and
  `bash docs/contracts/generate-conformance.sh > docs/contracts/conformance.md`.
- Decisions with alternatives get an ADR (`docs/adr/README.md` has the form).

## Versioning

`main` is the only long-lived branch; it accepts pull requests that pass the
`gates` workflow. A **tag** is an edition of the specification and is what
implementations pin. The first tag is the semantics freeze (almide/almide
roadmap item A0-1, sequenced after ADR-0012 D2/D3) — until then,
implementations pin a commit SHA. A tag never moves.

## Provenance

Extracted from `almide/almide` at commit `53e2a2ab7` (branch `develop`,
2026-08-20) with `git filter-repo`, keeping the full history of every path
listed in [BOUNDARY.md](./BOUNDARY.md) — 1,321 commits. Earlier history of
paths that were renamed into place lives in the implementation repository.

## Honest status

- The ALS normative text covers stdlib and observable semantics section by
  section; there is **no mechanized evaluation relation for Almide source here
  yet**. `proofs/ALS.v` in almide/almide is the *implementation's* ownership
  checker model (it imports `OwnershipChecker`) and stays there; a
  language-level mechanized semantics, when written, belongs here.
- The implementations (`almide/almide` `develop` and the `greenfield` rebuild)
  still carry copies of everything in this repository. Replacing those copies
  with a pin is the next, separately decided step (BOUNDARY.md, "Stage B").
- `docs/stdlib/` (the per-module API reference) and `docs/CHEATSHEET.md` remain
  in the implementation pending classification; both are partly generated from
  compiler source today.

## License

Dual-licensed under MIT or Apache-2.0, the same terms as almide/almide
(`LICENSE`, `LICENSE-MIT`, `LICENSE-APACHE`).
