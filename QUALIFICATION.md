# Qualification — what this repository claims, and how each claim is held

The qualification argument for `almide/als` as the requirements-and-
verification artifact set of an aviation-grade (DO-178C-class) language
project. Companions on the implementation side: `proofs/TOR.md`,
`proofs/DO330-GAP.md`, `docs/TRUST-SPINE.md` in
[almide/almide](https://github.com/almide/almide). This page states what is
enforced, by which instrument, and — just as deliberately — what is NOT
claimed.

## Objective mapping

| Objective (DO-178C shape) | Artifact here | Instrument |
|---|---|---|
| Requirements standard (A-1) | [docs/specs/als/STANDARD.md](docs/specs/als/STANDARD.md) — id grammar, prefix→chapter injection, writing rules, ratchet law, forbidden vocabulary | `scripts/check-als-style.sh` |
| Requirements identification + two-way traceability (A-3/A-6/A-7) | `ALS-<id>` sections ⇄ `C-NNN` contracts ⇄ fixtures (`// @contract:` headers) | `scripts/check-contracts.sh` — bidirectional, mutation-tested; every section cited, every contract evidenced at class ≥ fixture |
| Surface coverage of requirements | `proofs/als-element-coverage.toml` — every surface-syntax element → its normative section; 72/72, UNWRITTEN = 0 (a freeze precondition) | `scripts/check-als-element-coverage.sh` |
| Requirements precede code (the two-PR order, [CONTRIBUTING.md](CONTRIBUTING.md)) | `proofs/contract-provenance.toml` — per contract, the instant its id entered the ledger against the instant its `since` release was tagged: `requirements-first` (two-repo regime) / `contemporaneous` / `retroactive` / `unmeasured`; classes are derived from the recorded instants, the retroactive count is a shrink-only ceiling | `scripts/check-contract-provenance.py` (`--write` regenerates from full history; the gate itself needs none) |
| Configuration management (§7) | Separate repository; protected `main` (PR-only, required gate, admins included, linear history); baselines = tags; consumers pin commits | GitHub branch protection + the two-PR requirements-first order ([CONTRIBUTING.md](CONTRIBUTING.md)) |
| Verification results records | Conformance statements (TOML): corpus commit, binary, platform, legs, limit, per-leg counts, every failure verbatim | `scripts/conformance.py --report`; the `conformance` workflow uploads them as artifacts |
| Tool qualification (DO-330) | [proofs/gate-verification.toml](proofs/gate-verification.toml) — every verdict-bearing tool carries evidence it can FAIL correctly; UNVERIFIED ceiling **0** | `scripts/check-gate-verification.sh`; the runner's evidence is `scripts/selftest-conformance.py` (21 verdict-class scenarios against a scripted stub implementation, run in CI) |
| Problem reporting | [docs/ISSUE-TAXONOMY.md](docs/ISSUE-TAXONOMY.md) — closed severity set; `S-unsound` / `S-ambiguous` block an edition tag | Issue templates + labels; the edition-readiness instrument (planned with the first tag) re-checks |
| Documentation integrity | Generated documents regenerated-never-edited; links and anchors resolve | freshness diffs in `gates`; `scripts/check-links.sh` |

## Declared limitations

Stated so they cannot be mistaken for claims:

1. **No independent human second verifier.** The project is one person plus
   agents. Independence here is STRUCTURAL: two repositories, two gate sets,
   requirements reviewed and merged before implementation, and a runner any
   third party can point at any binary. It is not organizational
   independence, and this repository does not claim otherwise.
2. **The cross legs judge agreement, not truth.** A divergence between
   targets is caught; both targets being wrong IDENTICALLY is invisible to
   this runner. The implementations' third leg (the reference interpreter in
   their 3-way oracle) carries that burden today; a reference evaluator
   owned by this repository would close the gap and is the natural next
   instrument after the freeze ([#10](https://github.com/almide/als/issues/10)).
   **That third leg shares the front half.** `almide-interp` is an IR
   interpreter built on the compiler's own parser, checker and lowering; the
   three-way vote is N-version from IR downward only. A defect in the shared
   front half moves all three legs identically and agreement cannot see it —
   which is why the evaluator this repository will own must read source, not
   IR, and depend on no compiler crate.
3. **Discovery is directory-based, expectations are mandatory.** Every
   discovered fixture must declare its judgement (`@contract`,
   `@expect-fail`, the diag triple) — a case with no expectation is RED,
   never skipped (`selftest-conformance.py` pins this for the fail leg;
   `check-contracts.sh` pins it for `spec/wasm_cross`).
4. **No baseline tag exists yet.** The first tag is the semantics freeze
   (sequenced behind ADR-0012 D2/D3 on the implementation side). Until then
   consumers pin commit SHAs; the audit trail starts at extraction
   (`BOUNDARY.md`, Provenance). The chapter examples, by contrast, are all
   judged: `scripts/doctest.py` compiles AND runs every `almide` fence (167
   at the burn-down, [#13](https://github.com/almide/als/issues/13)), with
   the fragment and untagged ceilings ratcheted to zero — but the doctest
   judges the examples the spec chose to write, not the spec's coverage of
   the language; coverage is limitation 1's question, not this one's.
5. **The runner self-test verifies judgment, not the world.** The 21
   scenarios drive the real runner over stub processes; they prove the
   verdict logic, not wasmtime's or the OS's behaviour. Those are exercised
   by the real conformance runs.
6. **The proof-qualified checker certifies safety properties, not values.**
   What the implementation's kernel-proven checker establishes on every
   build is RC balance, name totality, the capability bound and type
   concretization — a certified-sound function can still print the wrong
   string ([proven-vs-trusted.md](docs/contracts/proven-vs-trusted.md)).
   Value correctness rests on the corpus and on agreement (limitation 2).
   In DO-330 terms the compiler's demotion to an output-verified tool
   therefore holds for exactly those properties; for functional correctness
   it stands where any unqualified compiler stands — the applicant verifies
   the object code — and this repository claims nothing more.
7. **Most requirements were written after their behaviour shipped.**
   `proofs/contract-provenance.toml` classifies every contract by the
   instant its id entered the ledger against the instant its `since` release
   was tagged. At the time of writing (2026-08-20): 301 contracts —
   2 requirements-first, 156 contemporaneous, 126 retroactive, 17
   unmeasured; the live figure is the first block of
   [docs/contracts/README.md](docs/contracts/README.md). The two-PR order
   is enforced from the extraction onward; the retroactive count is a
   shrink-only ceiling (`scripts/check-contract-provenance.py`).
8. **Traceability is one layer deep.** `ALS-<id>` ⇄ `C-NNN` ⇄ fixture is
   bidirectional and gated, but it is requirement ⇄ test; there is no
   HLR → LLR → design → code hierarchy, and no validation record that a
   section is accurate, complete and consistent beyond the style gate and
   the fixtures citing it. A per-section review record is the next
   instrument ([#12](https://github.com/almide/als/issues/12)); filling it
   with a second signer is limitation 1.
