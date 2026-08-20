# Spec Rules (almide/als)

## The corpus is truth, the spec is its evidence

- A normative statement without an executable fixture does not exist. Name the
  test paths in the section ("テスト: `spec/integration/modules/diamond_test.almd`").
- Write what IS. Code examples compile as-is when pasted into an `.almd` file —
  and `scripts/doctest.py` proves it on every conformance run. Every fence
  declares what it is (the vocabulary is closed, see the script's docstring):
  ```` ```almide ```` is a complete file that `almide test` compiles AND runs
  (write `test` blocks that assert the claims of the prose),
  ```` ```almide check-fail=ENNN ```` must be rejected with that code
  (`syntax` for a code-less parser rejection),
  ```` ```almide project ```` is a multi-file example split by `// file: <path>`
  lines (module dirs, `almide.toml`; negative forms `check-fail=` /
  `build-fail=`), and anything that is not Almide carries its own language
  (`text`, `ebnf`, `toml`, `bash`, `rust`). Bare fences and `almide fragment`
  are both ratcheted to zero.
- Delete stale spec. No `_deprecated/`. No "〜のはず", no "予定".
- Unimplemented design goes to an ADR (`docs/adr/`) or the implementation's
  roadmap — never here.
- `> Last updated: YYYY-MM-DD` at the top of every chapter.

## Chapters

| File | Content |
|---|---|
| `als/*.md` | Normative `## ALS-<id>` sections, cited by `docs/contracts/contracts.toml` (`spec =` key, both directions gated) |
| `language.md` | Types, declarations, expressions, statements, patterns, operators, visibility, comments |
| `type-system.md` | Inference, generics, records, variants, protocols, unions |
| `effect-system.md` | `fn` vs `effect fn`, explicit propagation (`!` only — ADR-0008), fan, capabilities, E006/E007/E008 |
| `result-option-effect.md` | `T?` / `T!` / `T!E`, the operator desugar table (ADR-0005), branchability (ADR-0004) |
| `effect-fn-call-semantics.md` | Call-site semantics of effect functions |
| `module-system.md` | import, submodules, diamond dependencies, visibility, `@extern` |
| `package-system.md` | Dependencies, MVS, version coexistence, module boundary |
| `edit-locality.md` | L1–L3 invariants, enforcement map, side conditions |

The implementation-specific chapters (`codegen.md`, `perceus.md`, `cli.md`)
live in almide/almide.
