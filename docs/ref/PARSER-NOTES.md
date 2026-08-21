# Reference evaluator — parser and semantics notes

> Last updated: 2026-08-21

What the reference evaluator (`ref/`, ADR-0015) decided where the normative
text was silent, stale, or contradicted by the accepted corpus — and what it
found. The judge's job is to disagree out loud: every entry below is either
a decision the evaluator took (and will change when the ALS speaks) or a
finding for the ALS / the implementation.

## Decisions the evaluator took (resolved in favour of the accepted corpus)

| # | Grammar / ALS text | Corpus reality | Evaluator |
|---|---|---|---|
| P1 | GRAMMAR `fieldinit = name (":" expr)?` (shorthand `{ x }`) | shorthand does not parse (doctest burn-down, language.md corrected) | not accepted |
| P2 | GRAMMAR `args … "_"` partial application | `_` in a call argument is E046 | parsed as a placeholder only to name the error; never evaluated |
| P3 | GRAMMAR braceless `fn_body` "collects statements until the next top-level declaration" | a top-level `let` after a braceless body is itself a declaration | a `let`/`var` at column 1 ends the braceless body |
| P4 | `""` strings: GRAMMAR lists escapes only | `"a<raw newline>b"` is accepted and IS `\n` (gleam_equality_matrix) | raw newlines allowed inside `"…"` |
| P5 | ALS-E1 Int literals i64 | `18446744073709551615` is a legal UInt64 literal through a fn boundary (C-179, uint64_upper_half) | lexed up to u64::MAX; above i64::MAX → `Expr::BigInt`, abstain `semantics:uint64-upper-half` (sized ints pending) |
| P6 | language.md §5.17 "Fan blocks only allow expressions" | `fan.bounded(budget) { let x = … }` bodies are blocks (fuel_block_body) | `fan.bounded` body parsed as a block; `fan.any`/`settle` as arms |
| P7 | uppercase-initial names are TypeName tokens | `let STANDARD = …`, `COUNT = COUNT + 1` — constants are uppercase binders (language.md §4.6) | a TypeName is a legal binder; in value position the environment is consulted before constructors |
| P8 | ALS-E10 "束縛は実リスト(list.range)を実体化する" | C-238 / #1400: a `let`-bound range used only as a for-in head is NOT materialized (range_bind_huge); forcing a beyond-cap span aborts `Error: out of memory` (C-197, range_materialize_oom) | ranges are first-class lazy values, materialized when forced; `isize::MAX/8` elements → C-197 abort; above 50M → abstain `resource:materialize-huge` |
| P9 | effect-system.md §3 lift: "Unwrapped T (auto-wrapped to ok(T)) / Full Result passed through" | `effect fn half(n) -> Int = if … then ok(n/2) else err("odd")` (error_operators) | an effect fn's body value is passed through when it is already Ok/Err, lifted otherwise; same for `T!` |
| P10 | ALS-M4 tail self-recursion O(1) stack; C-178 mutual tail recursion depth 10^6 | mutual_tail_recursion, effect_tco (bare and `!`-tailed self calls in effect fns) | tail calls to any top-level fn of the SAME channel class are trampolined (bare for total fns; bare or `!` for effect/fallible fns — both identities) |
| P11 | list.md: `list.push/pop/clear` "in place, requires var binding" (ALS-M9) | `var xs = []; list.push(xs, 1)` mutates `xs` | in-place mutators update the place and return Unit (O(1) amortized on a plain `var`) |
| P12 | ALS-C5 value semantics, capture_clone | a closure sees the captured `var`'s value at creation, not a later write (ref_gleam_capture_shadow) | closures capture by value (a snapshot of the visible bindings) |
| P13 | ALS-E23 "型は宣言または文脈から" | `let p: P = { x: 1 }` then `P { x, .. }` matches; defaulted fields filled (record_default_field_omitted) | anonymous record literals are re-tagged at annotated lets, params and returns (declaration order, defaults evaluated) |

## Rules the evaluator abstains on until the ALS states them

- `semantics:int-overflow` — `+ - * /` overflowing i64 (wrap? abort? The
  218-cell integer-domain sweep pinned guards; the rule is not in a chapter).
- `semantics:div-by-zero` — the exact abort message (C-053's statement).
- `semantics:fan-err-order` — the observable order when a `fan { … }` arm
  errs (C-199/C-200 pin it; not read in yet).
- `semantics:int-pow`, `semantics:float-op` (`**`, float `%`) — math.pow /
  libm transcription; ADR-0016 (numeric determinism family) decides.
- Float display for non-integral values — shortest round-trip digit generation
  is written by the evaluator itself, not yet (ALS-E3 / T2 are themselves
  partial); integral floats, inf/NaN render per ALS-R2/R4.

## Findings (the evaluator disagrees with the implementation, or the text is stale)

| # | Where | What | Status |
|---|---|---|---|
| F1 | `spec/wasm_cross/grain_functions.almd` labeled_args6 | The fixture's own comment says "argument EVALUATION ORDER is the written order, labels notwithstanding"; ALS-E26 says "引数は先頭から順に一度ずつ評価され"; the implementation evaluates `use2(b: tagged("first", 1), a: tagged("second", 2))` in PARAMETER order (`second` then `first`). The evaluator follows the written order and disagrees. | ALS-E26 must say which order; then either the fixture comment or the implementation is wrong |
| F2 | `spec/wasm_cross/effect_assign_unwrap.almd` | `xs[0] = step(10)` (index-assign of an effect call, no `!`) is accepted and the Int slot receives the unwrapped value — a residual implicit-propagation site after ADR-0008 ("伝搬は全明示"); `let`/`var`/plain assign spell `!` in the same file. The evaluator stores the Result value and then cannot type `int.to_string(xs[0])` → abstain `semantics:type-mismatch`. | implementation: residual auto-? at index-assign (contradicts ADR-0008 / ALS-M11); or ALS must exempt the position |
| F3 | ALS-M4 (semantics.md) | Still describes "effect fn 内の can-err 呼び出しへの自動 `?` 付与" — auto-? was removed by ADR-0008 (v0.55); C-064/068/069/119/135 cite the section. | ALS text stale — amend with a validation stamp |
| F4 | ALS-M14 (semantics.md) "キャリア … E024" | The UInt64 upper-half lane exists (C-179, uint64_upper_half.almd accepts `18446744073709551615`); the carrier paragraph describes the pre-lane wall. | ALS text stale |
| F5 | ALS-E10 (expressions.md) "束縛は実リスト(list.range)を実体化する" | C-238 was amended by #1400: a bound range used as a for-in head is not materialized (range_bind_huge). | ALS text stale |
| F6 | `almide run` stderr | compile warnings (E015 same-signature, unused variable, interpolation-debug-form) are printed by `almide run` before the program's own stderr; the conformance runner builds then executes so these never reach a verdict — a local `almide run` comparison must strip them. | note for tooling, not a spec finding |

F1 and F2 are the first two disagreements a reference evaluator produced in
this repository: exactly the class (both targets agree, the spec-reading
judge does not) that limitation 2 said agreement could not see.

## Measured at this commit

`scripts/check-ref-kernel.py`: 49/49 λ_almd programs byte-identical, twice.
Over `spec/wasm_cross` + `spec/programs` (602 programs): 172 evaluated, of
which 171 agree with the native target (the one disagreement is F1), 428
abstain in 151 classes (`proofs/ref-abstain.toml` — the long tail is stdlib:
`list.map` 38, `list.fold` 17, `bytes.new` 14, `int.parse` 14, …), 2
classified findings (F2). Both numbers are ratchets: abstains shrink-only,
agreement measured by the coming `--legs ref`.
