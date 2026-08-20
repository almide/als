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
   instrument after the freeze.
3. **Discovery is directory-based, expectations are mandatory.** Every
   discovered fixture must declare its judgement (`@contract`,
   `@expect-fail`, the diag triple) — a case with no expectation is RED,
   never skipped (`selftest-conformance.py` pins this for the fail leg;
   `check-contracts.sh` pins it for `spec/wasm_cross`).
4. **No baseline tag exists yet.** The first tag is the semantics freeze
   (sequenced behind ADR-0012 D2/D3 on the implementation side). Until then
   consumers pin commit SHAs; the audit trail starts at extraction
   (`BOUNDARY.md`, Provenance).
5. **The runner self-test verifies judgment, not the world.** The 21
   scenarios drive the real runner over stub processes; they prove the
   verdict logic, not wasmtime's or the OS's behaviour. Those are exercised
   by the real conformance runs.
