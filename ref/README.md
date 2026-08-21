# als-ref — the ALS reference evaluator

The judge's own evaluator of Almide programs ([ADR-0015](../docs/adr/0015-reference-evaluator-is-fresh-source-level-python.md)):
a fresh, source-level reading of the ALS chapters, written in stable Rust with
no dependency on any `almide-*` crate, behind a black-box protocol. It exists
to close QUALIFICATION.md limitation 2 — the cross legs judge agreement, a
reference judges truth — and it is seeded by the one place truth is already
available: the λ_almd kernel corpus (`proofs/kernel-conformance/`, C-280).

## Protocol

```
als-ref run <file.almd> --json
    {"exit": n, "stdout": "…", "stderr": "…"}        the program ran
    {"abstain": {"class": "…", "reason": "…"}}        not judged — a ledgered class
    {"error": "…"}                                    evaluator fault — red, never a verdict
als-ref run <file.almd>          plain: replays stdout/stderr, exits n; abstain → stderr + exit 3; fault → exit 4
als-ref parse <file.almd>        exit 0 iff the file parses
als-ref stdlib-index             implemented stdlib names, one per line (totality gate input)
als-ref --version                crate version and the pinned rustc channel
```

Abstain classes (the vocabulary `proofs/ref-abstain.toml` is keyed by):
`parse` (the parser does not accept the file yet), `stdlib:<module.fn>`,
`syntax:<form>`, `semantics:<rule>` (a rule the evaluator has not read into
itself yet — e.g. `semantics:int-overflow`, `semantics:div-by-zero`),
`render:<Type>` (a display form not implemented), `runtime:<capability>`
(todo/hole/no-main/extern), `resource:<what>` (fuel, materialize-huge),
`semantics:type-mismatch` (the implementation accepted a program the
ALS-reading evaluator cannot type — an implicit-conversion site: a FINDING,
see PARSER-NOTES).

## Build, gates, clauses

```
cd ref && cargo build --release          # rust-toolchain.toml pins 1.95.0 (Ferrocene 26.05.0's upstream)
python3 scripts/check-ref-kernel.py      # λ_almd agreement 1.0, twice (clauses 1 & 4 of the seed)
bash    scripts/check-ref-independence.sh # no almide-* dep, no nightly feature, clippy clauses, fmt
python3 scripts/check-ref-totality.py    # every corpus call implemented or in proofs/ref-abstain.toml (shrink-only)
```

The aviation-quality clauses of ADR-0015 are structural here: `clippy.toml`
forbids `HashMap`/`HashSet` (clause 1) and the std string/number/sort methods
whose behaviour the ALS specifies (clause 5); `F64` implements no `Display`
so a float can only be rendered by the evaluator's own formatter; every
`match` over the AST is exhaustive (clause 2) and an unimplemented form is an
explicit abstain; `rust-toolchain.toml` pins the channel (clause 4); the
independence gate reads `cargo tree` (clause 6).

## Layout

`src/lexer.rs` tokens · `src/parser.rs` recursive descent from the EBNF ·
`src/ast.rs` one variant per form · `src/value.rs` abstract values, ALS-R2
rendering · `src/eval.rs` the evaluator (effect-fn lift, explicit `!`
propagation, value semantics, lazy ranges, tail-call trampoline) ·
`src/stdlib.rs` the stdlib as the judge reads it · `src/main.rs` the protocol.
Parser/semantics decisions and the spec divergences they surfaced:
[`docs/ref/PARSER-NOTES.md`](../docs/ref/PARSER-NOTES.md).
