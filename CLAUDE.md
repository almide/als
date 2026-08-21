# almide/als — working rules

This repository is the **judge**: what an Almide implementation is measured
against. It holds normative text, the behavior-contract ledger, the executable
conformance corpus, the gates binding them, and the runner. It holds **no
compiler code**. Read `README.md` for the charter and `BOUNDARY.md` for what
lives here versus in an implementation, and why.

## Boundary rules

- **Nothing here depends on compiler internals.** If a change needs a compiler
  change to be true, the spec/contract/fixture lands HERE first (reviewed on its
  own), then the implementation bumps its pin in a second PR. Never the reverse
  order, never one PR spanning both.
- **Paths are verbatim** with the implementation layout (`spec/wasm_cross/…`,
  `docs/contracts/…`, `proofs/als-element-coverage.toml`). Do not rename or
  reorganise: the ledger, the ALS prose, the fixture headers, the implementation
  gates and the port gates all cite these paths, and a pinned checkout must diff
  cleanly against the copies the implementations still carry.
- Evidence that lives in an implementation (`tests/*.rs`, `crates/**`,
  `proofs/*.v|*.lean`, `spec/churn`, `spec/pass_isolated`) may be CITED in the
  ledger; the gates here report it as *deferred*, and the implementation's CI
  (running the same gates with `--impl-root`) owns the verdict. Do not add such
  a citation without the implementation-side unit existing.

## Before every commit

```bash
bash scripts/check-contracts.sh              # ledger ⇄ fixture ⇄ ALS traceability
python3 scripts/check-contract-provenance.py # requirement-before-behaviour ledger (retroactive shrink-only)
bash scripts/check-als-element-coverage.sh   # element → section ledger
bash scripts/check-als-style.sh              # requirements standard (STANDARD.md)
bash scripts/check-als-validation.sh         # per-section review records, hash-bound (unvalidated shrink-only)
bash scripts/check-links.sh                  # links and anchors resolve
bash scripts/check-gate-verification.sh      # the tools' own DO-330 ledger
bash scripts/check-ratchet-separation.sh --staged   # a ratchet loosening is its own dated commit
python3 scripts/selftest-conformance.py      # the runner can fail correctly
python3 scripts/check-runner-coverage.py     # and how much of it the self-test reaches (exact line floor)
bash docs/contracts/generate-readme.sh      > docs/contracts/README.md
bash docs/contracts/generate-conformance.sh > docs/contracts/conformance.md
bash docs/specs/als/generate-readme.sh      > docs/specs/als/README.md
```

Before an edition tag: `python3 scripts/edition-readiness.py --tag vX.Y.Z --almide <bin>` —
the baseline instrument; a tag is cut only on READY.
`lefthook install` wires these as pre-commit hooks. The `gates` workflow runs
them on every push and PR; `main` only takes PRs that pass it.

To judge a binary (needs `wasmtime` on `PATH`; the native leg uses the CLI only):
```bash
python3 scripts/conformance.py --almide /path/to/almide --report out.toml
python3 scripts/conformance.py --almide … --legs diag,fail --limit 20   # smoke
```

## Writing normative text (`docs/specs/`, `docs/specs/als/`)

- A normative statement without executable evidence does not exist. Every
  `## ALS-<id>` section must be cited by ≥1 contract; every contract must carry
  ≥1 evidence of class ≥ `fixture` (`scripts/lib/contract-classes.txt` is the
  ranked vocabulary). Name the test paths in the prose.
- Write what IS, with code that compiles. "Should", "planned", "will" belong in
  an ADR's Consequences or in the implementation's roadmap, not here.
- Delete stale spec; a spec that diverges from the corpus misleads. No
  `_deprecated/`.
- `> Last updated: YYYY-MM-DD` at the top of each chapter.
- A new or rewritten section lands with its validation row in
  `proofs/als-validation.toml` (`bash scripts/check-als-validation.sh --stamp
  ALS-<id>` prints it), or raises the unvalidated ceiling by exactly one with
  a justification. Editing a reviewed section's text makes its row STALE.
- Section ids (`ALS-M1`, `ALS-T4`, …) and contract ids (`C-NNN`) are permanent;
  never renumber. Contract ids are contiguous — append.

## Fixtures

- `spec/wasm_cross/*.almd`: `// @contract: C-NNN[, C-MMM]` header is mandatory
  and the ledger must list the fixture as that contract's evidence (symmetric
  link, gated). Observable = stdout, stderr, exit code; the legs must agree
  byte-for-byte. `// @xt-allow: <reason + tracking ref>` records a known
  divergence — it is logged, and flagged stale once the legs agree.
- `spec/wasm_fail/*.almd`: `// @expect-fail: <stderr substring>` mandatory;
  both legs must fail the same way. `// @xf-allow:` mirrors `@xt-allow`.
- `tests/diagnostics/<case>/`: `broken.almd` + `fixed.almd` + `meta.toml`
  (`expects_code`, `expects_error`, `hint_substring`). The case name is
  the diagnostic's shape, kebab-case.
- `spec/lang`, `spec/stdlib`, `spec/integration`: `test "name" { … }` blocks,
  `*_test.almd`. Stdlib API families are extended by matrix, never point-wise.

## Ledger (`docs/contracts/contracts.toml`)

Flat TOML, one scalar per line (awk-parsed). Required: `id`, `spec`, `title`,
`statement`, `since` (MAJOR.MINOR.PATCH of the behaviour, not of the entry),
`status`, `evidence` (≥1 `{ path, class[, name][, n] }`). `flagged-for-revision`
is a shrink-only ratchet (ceiling 0). The header comment of the file is the
schema's authoritative prose. Every contract also has a row in
`proofs/contract-provenance.toml` (entry instant vs `since` release instant →
requirements-first / contemporaneous / retroactive / unmeasured); after adding
a contract run `python3 scripts/check-contract-provenance.py --write` on a full
clone and commit the ledger. `retroactive` is shrink-only — a new contract over
already-shipped behaviour must name its justification and raise the ceiling by
exactly one.

## Git

- `main` only, via PR. No direct pushes.
- Commit messages: English, one line, no prefix, what changed.
- Tags are editions; a tag never moves. The first tag is the freeze.
- Never rewrite published history.
