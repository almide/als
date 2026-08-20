# Contributing to ALS

This repository is the judge: what an Almide implementation is measured
against. It accepts changes through pull requests into `main`; the `gates`
workflow is a required check for everyone, including administrators.

## The order of change (requirements first)

A behaviour change lands HERE before it lands in any implementation:

1. PR to this repository: the `ALS-<id>` section (new or amended), the
   `C-NNN` contract citing it, and ≥1 fixture declaring the contract in its
   `// @contract:` header. The gates enforce the three-way link.
2. The implementation advances its pin to the merged commit and makes the
   judge pass — a second, separately reviewed PR in its own repository.

Never the reverse order; never one PR spanning both repositories. The two
git histories are the evidence that requirements preceded code — and that
evidence is measured, not assumed: `proofs/contract-provenance.toml` records,
per contract, the instant its id entered the ledger against the instant its
`since` release was tagged (`requirements-first` / `contemporaneous` /
`retroactive` / `unmeasured`; `scripts/check-contract-provenance.py`). The
retroactive count is shrink-only. After adding a contract, regenerate the
ledger on a full clone — `python3 scripts/check-contract-provenance.py --write`
— and commit it.

## Identifiers are permanent

- Contract ids are contiguous `C-001..C-NNN`: take the next number, never
  renumber, never leave gaps. When several agents work in parallel, agree on
  disjoint id ranges FIRST — id collisions have happened twice.
- `ALS-<letter><number>[letter]` section ids and ADR numbers never change and
  are never reused, even when superseded.

## Before every commit

```bash
bash scripts/check-contracts.sh              # ledger ⇄ fixture ⇄ ALS traceability
python3 scripts/check-contract-provenance.py # requirement-before-behaviour ledger
bash scripts/check-als-element-coverage.sh   # element → section ledger
bash scripts/check-links.sh                  # every relative link and anchor resolves
bash scripts/check-als-style.sh              # requirements standard
bash scripts/check-gate-verification.sh      # the tools' own verification ledger
bash docs/contracts/generate-readme.sh      > docs/contracts/README.md
bash docs/contracts/generate-conformance.sh > docs/contracts/conformance.md
bash docs/specs/als/generate-readme.sh      > docs/specs/als/README.md
```

Spec code examples are judged: a ```almide fence must pass `almide check`
standalone, ```almide check-fail=ENNN must be rejected with that code, and
```almide fragment is counted against a shrink-only ceiling
(`scripts/doctest.py`). Prefer completing an example over tagging it
fragment.

`lefthook install` wires these as pre-commit hooks. Derived documents are
regenerated, never edited.

## Writing rules

- A normative statement without executable evidence does not exist.
- Observable means (stdout, stderr, exit code) — write nothing an
  implementation could not be tested against.
- Write what IS. Intentions and plans belong in an ADR's Consequences or in
  an implementation's roadmap.
- Commit messages: English, one line, no prefix.

## Judging a binary

```bash
python3 scripts/conformance.py --almide /path/to/almide --report out.toml
```

The statement records the corpus commit, the binary, the platform, the legs
and any `--limit`; a verdict is only as wide as what the statement says was
run. Findings against a RELEASED binary are expected when the corpus has
moved past it — that is the judge describing the release, not a defect in
either.
