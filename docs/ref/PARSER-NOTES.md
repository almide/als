# Reference evaluator — parser and semantics notes

> Last updated: 2026-08-21 (second pass: the stdlib round)

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
| P14 | closure capture (ALS-C5 value semantics vs the corpus) | a closure's write to a captured `var` is seen outside (closure_captured_list_mutation, sort_by_call_count = 10 key calls), while a later shadowing `let` is not (ref_gleam_capture_shadow) | bindings are shared SLOTS: closures capture the slots visible at creation — writes flow both ways, shadowing is frozen |
| P15 | checker-inferred nominal records | `{ zeta: 1, alpha: 2, mid: 3 }` renders `Rec { zeta, … }` (r5_wasm_inferred_record_repr); nested anonymous literals inside a recursive record take its type (compound_repr_recursive_interp) | an anonymous literal whose field set matches exactly ONE declared record type is tagged with it (declaration order); ambiguous sets stay anonymous |
| P16 | count/index truncation families | `string.take(s, -1)` = whole, `list.slice` negative start = empty (unsigned/as-usize), but `string.slice(-1, 2)` = `[ab]` (signed 0-clamp) — string_count_truncation, list_slice_oob | take/drop/take_end/drop_end use the UNSIGNED doctrine; string.slice clamps a negative start to 0; string.slice also has a 2-argument to-end form (string_codepoint_index) |

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

| F7 | ALS-T16 vs C-054/C-056 fixtures | T16's prose says a negative count clamps 負→0; string_count_truncation pins `take(-1)` = WHOLE (the as-usize doctrine its own comment names). The prose and the executable evidence disagree. | amend T16 (or the fixtures) — the evaluator follows the fixtures |
| F8 | ALS-S3 "空文字列に対する全称述語は true" | is_alphanumeric("") = false (string_is_alphanumeric), is_digit("") = false and ASCII-only, is_whitespace("") = TRUE (string_ops_drain, string_whitespace); is_upper/is_lower are Python-style (≥1 cased, all cased hold). One vacuous-truth sentence covers five different lifts. | rewrite S3 per predicate |
| F9 | ALS-T8 | int.parse trims the T1 UNICODE whitespace set first (string_whitespace pins it; T8 is silent). int.from_hex is NOT from_str_radix: whitespace trims, lowercase `0x` prefixes strip REPEATEDLY (`0x0x0x10` = 16), uppercase `0X` does not, and the sign may follow the prefix (`0x-ff` = -255) — int_from_hex pins all of it. | amend T8 with the real grammars |
| F10 | ALS-T23 vs release 0.58.0 | the implementation still applies the retired first-argument ±0 rule in float.min/max (float_signed_zero_minmax, float_clamp_negative_zero's min/max line); the chapter itself declares its suite files red until the pin advances. | `@ref-allow` carries both fixtures; comes off at the catch-up |

F1 and F2 are the first two disagreements a reference evaluator produced in
this repository: exactly the class (both targets agree, the spec-reading
judge does not) that limitation 2 said agreement could not see. F7–F9 are the
second pass's: three places where normative prose and executable evidence
already disagreed BEFORE any implementation was judged.

## Measured at this commit

`scripts/check-ref-kernel.py`: 49/49 λ_almd programs byte-identical, twice.
Over `spec/wasm_cross` + `spec/programs` (610 programs): **522 evaluated, 518
agree with the native target, 0 evaluator faults** — the 4 disagreements are
all adjudicated (F1 named-arg order, F10 ±0 min/max ×3, each under
`@ref-allow`). 88 abstain in 35 classes (`proofs/ref-abstain.toml`; the head
of the tail is now Codec statics / the fan charge model / matrix / sized
ints). 451 stdlib functions implemented from the chapters — round 2 added
the dynamic `Value` + the NORMATIVE json parser (transcribed from
`stdlib/json_parse.almd`, deliberately lenient), `bytes` with reference
semantics (let/var snapshots, aliasing through parameters) and total zero reads,
fs/env/io/process effects, the vendored musl libm (`ref/src/libm.rs`, the
SAME upstream the runtime vendors — bit-agreement confirmed over
math_transcendental_bits / trig_libm / math_log_gamma), the civil calendar,
hashes, base64/hex, http builders, and the C-034/C-197 allocation aborts. The float text path is the evaluator's own
exact big-integer Dragon4 + correctly rounded decimal→binary64
(`ref/src/fmtfloat.rs`), held against the host formatter/parser by two
oracle tests over ~6,000 samples (`ref/tests/fmt_oracle.rs` — the host
appears only as a test oracle, never in the production path).
