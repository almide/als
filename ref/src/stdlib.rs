//! The stdlib as the judge reads it — every function written from the
//! normative text: ALS-S1..S6 (strings, code-point units), ALS-T1..T24
//! (whitespace set, parse/format, termination convention, numeric family),
//! ALS-C1..C10 (collection order, clamping, value semantics), ADR-0005
//! (operator desugars), ADR-0006 (fallibility-polymorphic callbacks:
//! first-err form). Never delegated to the host for ALS-specified semantics
//! (ADR-0015 clause 5; `clippy.toml` forbids the tempting std methods).
//! Anything not here is an ABSTAIN with class `stdlib:<module.fn>`.

use std::cmp::Ordering;
use std::rc::Rc;

use crate::eval::{Flow, Interp};
use crate::fmtfloat;
use crate::value::{fmt_int, render, value_cmp, values_eq, Callable, Value, F64};

/// Names the evaluator implements, for the totality gate (`als-ref stdlib-index`).
pub fn implemented() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    v.extend(crate::stdlib_ext::EXT_FNS);
    v.extend(crate::stdlib_ext2::EXT2_FNS);
    v.extend(crate::stdlib_matrix::MATRIX_FNS);
    v.extend(crate::stdlib_sized::SIZED_FNS);
    v.extend(PRELUDE_FNS);
    v.extend(LIST_FNS);
    v.extend(STRING_FNS);
    v.extend(INT_FNS);
    v.extend(FLOAT_FNS);
    v.extend(MATH_FNS);
    v.extend(OPTION_FNS);
    v.extend(RESULT_FNS);
    v.extend(MAP_FNS);
    v.extend(SET_FNS);
    v
}

const PRELUDE_FNS: &[&str] = &["println", "eprintln", "assert", "assert_eq", "assert_ne"];
const LIST_FNS: &[&str] = &[
    "list.len",
    "list.get",
    "list.get_or",
    "list.set",
    "list.swap",
    "list.sort",
    "list.reverse",
    "list.contains",
    "list.enumerate",
    "list.zip",
    "list.flatten",
    "list.take",
    "list.drop",
    "list.unique",
    "list.index_of",
    "list.last",
    "list.chunk",
    "list.windows",
    "list.sum",
    "list.product",
    "list.first",
    "list.is_empty",
    "list.min",
    "list.max",
    "list.join",
    "list.map",
    "list.filter",
    "list.find",
    "list.any",
    "list.all",
    "list.sort_by",
    "list.flat_map",
    "list.filter_map",
    "list.take_while",
    "list.drop_while",
    "list.count",
    "list.partition",
    "list.reduce",
    "list.range",
    "list.slice",
    "list.insert",
    "list.remove_at",
    "list.find_index",
    "list.update",
    "list.repeat",
    "list.scan",
    "list.intersperse",
    "list.dedup",
    "list.zip_with",
    "list.fold",
    "list.take_end",
    "list.drop_end",
    "list.unique_by",
    "list.push",
    "list.pop",
    "list.clear",
];
const STRING_FNS: &[&str] = &[
    "string.trim",
    "string.trim_start",
    "string.trim_end",
    "string.split",
    "string.join",
    "string.len",
    "string.contains",
    "string.starts_with",
    "string.ends_with",
    "string.slice",
    "string.pad_start",
    "string.pad_end",
    "string.to_bytes",
    "string.from_bytes",
    "string.capitalize",
    "string.to_upper",
    "string.to_lower",
    "string.replace",
    "string.replace_first",
    "string.get",
    "string.lines",
    "string.chars",
    "string.index_of",
    "string.last_index_of",
    "string.repeat",
    "string.count",
    "string.is_empty",
    "string.reverse",
    "string.strip_prefix",
    "string.strip_suffix",
    "string.first",
    "string.last",
    "string.take",
    "string.take_end",
    "string.drop",
    "string.drop_end",
    "string.is_digit",
    "string.is_alpha",
    "string.is_alphanumeric",
    "string.is_whitespace",
    "string.is_upper",
    "string.is_lower",
    "string.codepoint",
    "string.from_codepoint",
    "string.concat",
];
const INT_FNS: &[&str] = &[
    "int.to_string",
    "int.parse",
    "int.from_hex",
    "int.abs",
    "int.min",
    "int.max",
    "int.band",
    "int.bor",
    "int.bxor",
    "int.bnot",
    "int.bshl",
    "int.bshr",
    "int.wrap_add",
    "int.wrap_mul",
    "int.rotate_left",
    "int.rotate_right",
    "int.to_u32",
    "int.to_u8",
    "int.clamp",
    "int.to_float",
];
const FLOAT_FNS: &[&str] = &[
    "float.to_string",
    "float.to_int",
    "float.to_int64_checked",
    "float.round",
    "float.floor",
    "float.ceil",
    "float.abs",
    "float.sqrt",
    "float.from_int",
    "float.min",
    "float.max",
    "float.to_fixed",
    "float.clamp",
    "float.sign",
    "float.is_nan",
    "float.is_infinite",
    "float.to_bits",
    "float.bits_to_float",
    "float.parse",
];
const MATH_FNS: &[&str] = &[
    "math.min",
    "math.max",
    "math.abs",
    "math.pow",
    "math.fmin",
    "math.fmax",
    "math.sqrt",
];
const OPTION_FNS: &[&str] = &[
    "option.map",
    "option.flat_map",
    "option.flatten",
    "option.unwrap_or",
    "option.unwrap_or_else",
    "option.is_some",
    "option.is_none",
    "option.to_result",
    "option.filter",
    "option.zip",
    "option.or_else",
    "option.to_list",
];
const RESULT_FNS: &[&str] = &[
    "result.map",
    "result.map_err",
    "result.flat_map",
    "result.unwrap_or",
    "result.unwrap_or_else",
    "result.is_ok",
    "result.is_err",
    "result.to_option",
    "result.to_err_option",
    "result.partition",
];
const MAP_FNS: &[&str] = &[
    "map.new",
    "map.get",
    "map.get_or",
    "map.set",
    "map.contains",
    "map.remove",
    "map.keys",
    "map.values",
    "map.len",
    "map.entries",
    "map.merge",
    "map.is_empty",
    "map.from_list",
    "map.map",
    "map.filter",
    "map.fold",
    "map.any",
    "map.all",
    "map.count",
    "map.find",
    "map.update",
    "map.insert",
    "map.delete",
    "map.clear",
];
const SET_FNS: &[&str] = &[
    "set.new",
    "set.from_list",
    "set.insert",
    "set.remove",
    "set.contains",
    "set.len",
    "set.is_empty",
    "set.to_list",
    "set.union",
    "set.intersection",
    "set.difference",
    "set.symmetric_difference",
    "set.is_subset",
    "set.is_disjoint",
    "set.filter",
    "set.map",
    "set.fold",
    "set.any",
    "set.all",
];

// ── argument helpers ─────────────────────────────────────────────────────

fn arity(name: &str, args: &[Value], n: usize) -> Result<(), Flow> {
    if args.len() != n {
        return Err(Flow::Fatal(format!(
            "{name}: expected {n} argument(s), got {}",
            args.len()
        )));
    }
    Ok(())
}

fn mismatch<T>(name: &str, want: &str, got: &Value) -> Result<T, Flow> {
    Err(Flow::Abstain {
        class: "semantics:type-mismatch".into(),
        reason: format!("{name}: expected {want}, got {} — the implementation accepted a program the ALS-reading evaluator cannot type (an implicit conversion site?)", got.type_name()),
    })
}

fn want_str<'a>(name: &str, v: &'a Value) -> Result<&'a Rc<str>, Flow> {
    match v {
        Value::Str(s) => Ok(s),
        other => mismatch(name, "String", other),
    }
}

fn want_int(name: &str, v: &Value) -> Result<i64, Flow> {
    match v {
        Value::Int(n) => Ok(*n),
        other => mismatch(name, "Int", other),
    }
}

fn want_float(name: &str, v: &Value) -> Result<f64, Flow> {
    match v {
        Value::Float(f) => Ok(f.0),
        other => mismatch(name, "Float", other),
    }
}

fn want_bool(name: &str, v: &Value) -> Result<bool, Flow> {
    match v {
        Value::Bool(b) => Ok(*b),
        other => mismatch(name, "Bool", other),
    }
}

fn want_list<'a>(name: &str, v: &'a Value) -> Result<&'a Rc<Vec<Value>>, Flow> {
    match v {
        Value::List(xs) => Ok(xs),
        other => mismatch(name, "List", other),
    }
}

fn want_map<'a>(name: &str, v: &'a Value) -> Result<&'a Rc<Vec<(Value, Value)>>, Flow> {
    match v {
        Value::Map(m) => Ok(m),
        other => mismatch(name, "Map", other),
    }
}

fn want_set<'a>(name: &str, v: &'a Value) -> Result<&'a Rc<Vec<Value>>, Flow> {
    match v {
        Value::Set(xs) => Ok(xs),
        other => mismatch(name, "Set", other),
    }
}

fn want_fn<'a>(name: &str, v: &'a Value) -> Result<&'a Rc<Callable>, Flow> {
    match v {
        Value::Fn(c) => Ok(c),
        other => mismatch(name, "a function", other),
    }
}

fn eq_strict(name: &str, a: &Value, b: &Value) -> Result<bool, Flow> {
    values_eq(a, b).ok_or_else(|| Flow::Fatal(format!("{name}: incomparable values")))
}

fn cmp_strict(name: &str, a: &Value, b: &Value) -> Result<Ordering, Flow> {
    value_cmp(a, b).ok_or_else(|| Flow::Abstain {
        class: "semantics:compare".into(),
        reason: format!(
            "{name}: ordering {} and {} is not specified in a chapter this evaluator has read",
            a.type_name(),
            b.type_name()
        ),
    })
}

/// The count/index doctrine the C-054/C-056 fixtures pin: the i64 is
/// reinterpreted UNSIGNED (v0's `as usize`), then clamped to len — so a
/// negative count means "whole", not "none". (ALS-T16's prose says 負→0;
/// the fixtures say otherwise — recorded as a finding in PARSER-NOTES.)
fn clamp_idx(i: i64, len: usize) -> usize {
    let u = i as u64;
    if u > len as u64 {
        len
    } else {
        u as usize
    }
}

/// ADR-0006: a fallible callback puts the whole call in first-err form.
enum Cb {
    Val(Value),
    Bail(Value),
}

fn call_cb(
    it: &mut Interp,
    c: &Rc<Callable>,
    fallible: bool,
    args: Vec<Value>,
) -> Result<Cb, Flow> {
    let v = it.call_value(c, args)?;
    if fallible {
        match v {
            Value::Ok(x) => Ok(Cb::Val((*x).clone())),
            Value::Err(e) => Ok(Cb::Bail((*e).clone())),
            other => Ok(Cb::Val(other)),
        }
    } else {
        Ok(Cb::Val(v))
    }
}

/// wrap a HOF result per the callback's fallibility (first-err form)
fn hof_out(fallible: bool, bail: Option<Value>, v: Value) -> Value {
    match (fallible, bail) {
        (true, Some(e)) => Value::Err(Rc::new(e)),
        (true, None) => Value::Ok(Rc::new(v)),
        (false, _) => v,
    }
}

fn render_arg(v: &Value) -> Result<String, Flow> {
    render(v).ok_or_else(|| Flow::Abstain {
        class: format!("render:{}", v.type_name()),
        reason: format!(
            "rendering a {} is not implemented by the reference evaluator yet",
            v.type_name()
        ),
    })
}

/// stable merge sort with a fallible comparator (slice::sort is forbidden —
/// the ordering is the ALS's, and the comparator may abstain)
fn stable_sort_by<F>(items: &mut Vec<Value>, mut lt: F) -> Result<(), Flow>
where
    F: FnMut(&Value, &Value) -> Result<Ordering, Flow>,
{
    let n = items.len();
    if n < 2 {
        return Ok(());
    }
    let mut buf = items.clone();
    let mut width = 1;
    while width < n {
        let mut i = 0;
        while i < n {
            let mid = (i + width).min(n);
            let hi = (i + 2 * width).min(n);
            let (mut a, mut b, mut k) = (i, mid, i);
            while a < mid && b < hi {
                if lt(&items[b], &items[a])? == Ordering::Less {
                    buf[k] = items[b].clone();
                    b += 1;
                } else {
                    buf[k] = items[a].clone();
                    a += 1;
                }
                k += 1;
            }
            while a < mid {
                buf[k] = items[a].clone();
                a += 1;
                k += 1;
            }
            while b < hi {
                buf[k] = items[b].clone();
                b += 1;
                k += 1;
            }
            i = hi;
        }
        std::mem::swap(items, &mut buf);
        width *= 2;
    }
    Ok(())
}

// ── string primitives (code-point units, ALS-S1) ────────────────────────

/// ALS-T1: the 25 Unicode White_Space code points, spelled out.
fn is_als_whitespace(c: char) -> bool {
    matches!(c as u32,
        0x0009..=0x000D | 0x0020 | 0x0085 | 0x00A0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000)
}

/// Case-ignorable / cased approximations for the Final_Sigma rule (ALS-T5,
/// Unicode 3.13): sufficient for the corpus; a full property table is the
/// upgrade path.
fn is_case_ignorable(c: char) -> bool {
    matches!(c, '\'' | '\u{2019}' | '.' | ':' | '^' | '`' | '\u{00B4}' | '\u{02B0}'..='\u{02FF}' | '\u{0300}'..='\u{036F}')
}

fn is_cased(c: char) -> bool {
    c.is_lowercase() || c.is_uppercase()
}

/// substring occurrences over code points; empty needle matches every
/// boundary including both ends (ALS-S2, Rust `str::matches` semantics)
fn find_from(hay: &[char], needle: &[char], start: usize) -> Option<usize> {
    if needle.is_empty() {
        return if start <= hay.len() {
            Some(start)
        } else {
            None
        };
    }
    if needle.len() > hay.len() {
        return None;
    }
    (start..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

fn chars_of(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn s_of(cs: &[char]) -> String {
    cs.iter().collect()
}

// ── the dispatcher ───────────────────────────────────────────────────────

pub fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> Result<Value, Flow> {
    match name {
        // ═══ prelude (language.md §11, ALS-T18) ═══════════════════════
        "println" | "eprintln" => {
            arity(name, &args, 1)?;
            let s = render_arg(&args[0])?;
            let sink = if name == "println" {
                &mut it.stdout
            } else {
                &mut it.stderr
            };
            sink.push_str(&s);
            sink.push('\n');
            Ok(Value::Unit)
        }
        "assert" => {
            if args.is_empty() || args.len() > 2 {
                return Err(Flow::Fatal("assert: expected 1 or 2 arguments".into()));
            }
            match &args[0] {
                Value::Bool(true) => Ok(Value::Unit),
                Value::Bool(false) => {
                    let head = if args.len() == 2 {
                        format!("assertion failed: {}", render_arg(&args[1])?)
                    } else {
                        "assertion failed".into()
                    };
                    Err(Flow::Abort(format!("{head}\n  at: line {}", it.cur_line)))
                }
                other => mismatch(name, "Bool", other),
            }
        }
        "assert_eq" | "assert_ne" => {
            arity(name, &args, 2)?;
            let eq = eq_strict(name, &args[0], &args[1])?;
            let want = name == "assert_eq";
            if eq == want {
                Ok(Value::Unit)
            } else {
                let l = render_arg(&args[0])?;
                let expected = if want {
                    render_arg(&args[1])?
                } else {
                    format!("!= {l}")
                };
                Err(Flow::Abort(format!(
                    "assertion failed\n  at: line {}\n  expected: {expected}\n  found: {l}",
                    it.cur_line
                )))
            }
        }

        // ═══ list ══════════════════════════════════════════════════════
        "list.len" => {
            arity(name, &args, 1)?;
            Ok(Value::Int(want_list(name, &args[0])?.len() as i64))
        }
        "list.is_empty" => {
            arity(name, &args, 1)?;
            Ok(Value::Bool(want_list(name, &args[0])?.is_empty()))
        }
        "list.get" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?;
            let i = want_int(name, &args[1])?;
            Ok(if i < 0 || i as usize >= xs.len() {
                Value::None
            } else {
                Value::Some(Rc::new(xs[i as usize].clone()))
            })
        }
        "list.get_or" => {
            arity(name, &args, 3)?;
            let xs = want_list(name, &args[0])?;
            let i = want_int(name, &args[1])?;
            Ok(if i < 0 || i as usize >= xs.len() {
                args[2].clone()
            } else {
                xs[i as usize].clone()
            })
        }
        "list.first" | "list.last" => {
            arity(name, &args, 1)?;
            let xs = want_list(name, &args[0])?;
            let v = if name == "list.first" {
                xs.first()
            } else {
                xs.last()
            };
            Ok(v.map(|x| Value::Some(Rc::new(x.clone())))
                .unwrap_or(Value::None))
        }
        "list.set" | "list.update" => {
            arity(name, &args, 3)?;
            let xs = want_list(name, &args[0])?;
            let i = want_int(name, &args[1])?;
            if i < 0 || i as usize >= xs.len() {
                // OOB functional writes are NO-OPS (list_heapelem_rc update_oob,
                // list_remove_at_oob)
                return Ok(Value::List(xs.clone()));
            }
            let mut v = (**xs).clone();
            if name == "list.set" {
                v[i as usize] = args[2].clone();
            } else {
                let c = want_fn(name, &args[2])?.clone();
                let fal = it.cb_fallible(&c);
                match call_cb(it, &c, fal, vec![v[i as usize].clone()])? {
                    Cb::Val(nv) => v[i as usize] = nv,
                    Cb::Bail(e) => return Ok(Value::Err(Rc::new(e))),
                }
            }
            Ok(Value::List(Rc::new(v)))
        }
        "list.swap" => {
            arity(name, &args, 3)?;
            let xs = want_list(name, &args[0])?;
            let (i, j) = (want_int(name, &args[1])?, want_int(name, &args[2])?);
            if i < 0 || j < 0 || i as usize >= xs.len() || j as usize >= xs.len() {
                return Ok(Value::List(xs.clone()));
            }
            let mut v = (**xs).clone();
            v.swap(i as usize, j as usize);
            Ok(Value::List(Rc::new(v)))
        }
        "list.reverse" => {
            arity(name, &args, 1)?;
            let mut v = (**want_list(name, &args[0])?).clone();
            v.reverse();
            Ok(Value::List(Rc::new(v)))
        }
        "list.contains" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?.clone();
            for x in xs.iter() {
                if eq_strict(name, x, &args[1])? {
                    return Ok(Value::Bool(true));
                }
            }
            Ok(Value::Bool(false))
        }
        "list.index_of" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?.clone();
            for (i, x) in xs.iter().enumerate() {
                if eq_strict(name, x, &args[1])? {
                    return Ok(Value::Some(Rc::new(Value::Int(i as i64))));
                }
            }
            Ok(Value::None)
        }
        "list.enumerate" => {
            arity(name, &args, 1)?;
            let xs = want_list(name, &args[0])?;
            Ok(Value::List(Rc::new(
                xs.iter()
                    .enumerate()
                    .map(|(i, x)| Value::Tuple(Rc::new(vec![Value::Int(i as i64), x.clone()])))
                    .collect(),
            )))
        }
        "list.zip" => {
            arity(name, &args, 2)?;
            let (a, b) = (want_list(name, &args[0])?, want_list(name, &args[1])?);
            Ok(Value::List(Rc::new(
                a.iter()
                    .zip(b.iter())
                    .map(|(x, y)| Value::Tuple(Rc::new(vec![x.clone(), y.clone()])))
                    .collect(),
            )))
        }
        "list.flatten" => {
            arity(name, &args, 1)?;
            let xs = want_list(name, &args[0])?.clone();
            let mut out = Vec::new();
            for x in xs.iter() {
                out.extend(want_list(name, x)?.iter().cloned());
            }
            Ok(Value::List(Rc::new(out)))
        }
        "list.take" | "list.drop" | "list.take_end" | "list.drop_end" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?;
            let n = clamp_idx(want_int(name, &args[1])?, xs.len());
            let v: Vec<Value> = match name {
                "list.take" => xs[..n].to_vec(),
                "list.drop" => xs[n..].to_vec(),
                "list.take_end" => xs[xs.len() - n..].to_vec(),
                _ => xs[..xs.len() - n].to_vec(),
            };
            Ok(Value::List(Rc::new(v)))
        }
        "list.slice" => {
            arity(name, &args, 3)?;
            let xs = want_list(name, &args[0])?;
            let a = clamp_idx(want_int(name, &args[1])?, xs.len());
            let b = clamp_idx(want_int(name, &args[2])?, xs.len());
            Ok(Value::List(Rc::new(if a >= b {
                Vec::new()
            } else {
                xs[a..b].to_vec()
            })))
        }
        "list.insert" => {
            arity(name, &args, 3)?;
            let xs = want_list(name, &args[0])?;
            let i = want_int(name, &args[1])?;
            // insert index is as-usize then clamped to len: -1 APPENDS
            // (list_count_index_truncation), 10 on a 3-list appends too
            let i = (i as u64).min(xs.len() as u64) as usize;
            let mut v = (**xs).clone();
            v.insert(i, args[2].clone());
            Ok(Value::List(Rc::new(v)))
        }
        "list.remove_at" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?;
            let i = want_int(name, &args[1])?;
            if i < 0 || i as usize >= xs.len() {
                return Ok(Value::List(xs.clone()));
            }
            let mut v = (**xs).clone();
            v.remove(i as usize);
            Ok(Value::List(Rc::new(v)))
        }
        "list.unique" | "list.dedup" => {
            arity(name, &args, 1)?;
            let xs = want_list(name, &args[0])?.clone();
            let mut out: Vec<Value> = Vec::new();
            for x in xs.iter() {
                let dup = if name == "list.unique" {
                    let mut found = false;
                    for y in &out {
                        if eq_strict(name, x, y)? {
                            found = true;
                            break;
                        }
                    }
                    found
                } else {
                    match out.last() {
                        Some(y) => eq_strict(name, x, y)?,
                        None => false,
                    }
                };
                if !dup {
                    out.push(x.clone());
                }
            }
            Ok(Value::List(Rc::new(out)))
        }
        "list.chunk" | "list.windows" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?;
            let n = want_int(name, &args[1])?;
            // ALS-T4: n == 0 aborts (T6 form); n < 0: chunk = whole-as-one, windows = []
            if n == 0 {
                return Err(Flow::Abort(if name == "list.chunk" {
                    "chunk size must be positive".into()
                } else {
                    "window size must be positive".into()
                }));
            }
            if n < 0 {
                return Ok(if name == "list.chunk" {
                    if xs.is_empty() {
                        Value::List(Rc::new(vec![]))
                    } else {
                        Value::List(Rc::new(vec![Value::List(xs.clone())]))
                    }
                } else {
                    Value::List(Rc::new(vec![]))
                });
            }
            let n = n as usize;
            let mut out = Vec::new();
            if name == "list.chunk" {
                let mut i = 0;
                while i < xs.len() {
                    out.push(Value::List(Rc::new(xs[i..(i + n).min(xs.len())].to_vec())));
                    i += n;
                }
            } else if n <= xs.len() {
                for i in 0..=xs.len() - n {
                    out.push(Value::List(Rc::new(xs[i..i + n].to_vec())));
                }
            }
            Ok(Value::List(Rc::new(out)))
        }
        "list.sum" | "list.product" => {
            arity(name, &args, 1)?;
            let xs = want_list(name, &args[0])?.clone();
            // ALS-T16: sum/product wrap in i64
            let mut acc: i64 = if name == "list.sum" { 0 } else { 1 };
            for x in xs.iter() {
                let n = want_int(name, x)?;
                acc = if name == "list.sum" {
                    acc.wrapping_add(n)
                } else {
                    acc.wrapping_mul(n)
                };
            }
            Ok(Value::Int(acc))
        }
        "list.min" | "list.max" => {
            arity(name, &args, 1)?;
            let xs = want_list(name, &args[0])?.clone();
            let mut best: Option<Value> = None;
            for x in xs.iter() {
                best = Some(match best {
                    None => x.clone(),
                    Some(b) => {
                        let ord = cmp_strict(name, x, &b)?;
                        let take = if name == "list.min" {
                            ord == Ordering::Less
                        } else {
                            ord == Ordering::Greater
                        };
                        if take {
                            x.clone()
                        } else {
                            b
                        }
                    }
                });
            }
            Ok(best.map(|v| Value::Some(Rc::new(v))).unwrap_or(Value::None))
        }
        "list.join" | "string.join" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?.clone();
            let sep = want_str(name, &args[1])?.to_string();
            let mut out = String::new();
            for (i, x) in xs.iter().enumerate() {
                if i > 0 {
                    out.push_str(&sep);
                }
                out.push_str(want_str(name, x)?);
            }
            Ok(Value::str(&out))
        }
        "list.range" => {
            arity(name, &args, 2)?;
            let (a, b) = (want_int(name, &args[0])?, want_int(name, &args[1])?);
            if (b as i128) - (a as i128) > (1i128 << 31) {
                return Err(Flow::Abort("out of memory".into()));
            }
            let mut out = Vec::new();
            let mut i = a;
            while i < b {
                out.push(Value::Int(i));
                i += 1;
            }
            Ok(Value::List(Rc::new(out)))
        }
        "list.repeat" => {
            arity(name, &args, 2)?;
            let n = want_int(name, &args[1])?;
            // C-034 family: the shared 2^31 ceiling guards the allocation
            if n > (1i64 << 31) {
                return Err(Flow::Abort("repeat result too large".into()));
            }
            let n = if n < 0 { 0 } else { n as usize };
            Ok(Value::List(Rc::new(vec![args[0].clone(); n])))
        }
        "list.intersperse" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?;
            let mut out = Vec::new();
            for (i, x) in xs.iter().enumerate() {
                if i > 0 {
                    out.push(args[1].clone());
                }
                out.push(x.clone());
            }
            Ok(Value::List(Rc::new(out)))
        }
        "list.sort" => {
            arity(name, &args, 1)?;
            let mut v = (**want_list(name, &args[0])?).clone();
            stable_sort_by(&mut v, |a, b| cmp_strict(name, a, b))?;
            Ok(Value::List(Rc::new(v)))
        }
        // ── list HOFs (first-err form when the callback is fallible) ──
        "list.map" | "list.filter" | "list.find" | "list.any" | "list.all" | "list.flat_map"
        | "list.filter_map" | "list.take_while" | "list.drop_while" | "list.count"
        | "list.partition" | "list.find_index" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?.clone();
            let c = want_fn(name, &args[1])?.clone();
            let fal = it.cb_fallible(&c);
            let mut bail = None;
            let mut out: Vec<Value> = Vec::new();
            let mut out2: Vec<Value> = Vec::new();
            let mut acc_bool = name == "list.all";
            let mut found: Option<Value> = None;
            let mut count = 0i64;
            let mut dropping = name == "list.drop_while";
            for (idx, x) in xs.iter().enumerate() {
                let r = match call_cb(it, &c, fal, vec![x.clone()])? {
                    Cb::Val(v) => v,
                    Cb::Bail(e) => {
                        bail = Some(e);
                        break;
                    }
                };
                match name {
                    "list.map" => out.push(r),
                    "list.flat_map" => out.extend(want_list(name, &r)?.iter().cloned()),
                    "list.filter_map" => match r {
                        Value::Some(v) => out.push((*v).clone()),
                        Value::None => {}
                        other => return mismatch(name, "Option", &other),
                    },
                    "list.filter" | "list.partition" => {
                        if want_bool(name, &r)? {
                            out.push(x.clone());
                        } else if name == "list.partition" {
                            out2.push(x.clone());
                        }
                    }
                    "list.find" => {
                        if want_bool(name, &r)? {
                            found = Some(x.clone());
                            break;
                        }
                    }
                    "list.find_index" => {
                        if want_bool(name, &r)? {
                            found = Some(Value::Int(idx as i64));
                            break;
                        }
                    }
                    "list.any" => {
                        if want_bool(name, &r)? {
                            acc_bool = true;
                            break;
                        }
                    }
                    "list.all" => {
                        if !want_bool(name, &r)? {
                            acc_bool = false;
                            break;
                        }
                    }
                    "list.count" => {
                        if want_bool(name, &r)? {
                            count += 1;
                        }
                    }
                    "list.take_while" => {
                        if want_bool(name, &r)? {
                            out.push(x.clone());
                        } else {
                            break;
                        }
                    }
                    "list.drop_while" => {
                        if dropping && want_bool(name, &r)? {
                            continue;
                        }
                        dropping = false;
                        out.push(x.clone());
                    }
                    _ => unreachable!(),
                }
            }
            let v = match name {
                "list.map" | "list.flat_map" | "list.filter_map" | "list.filter"
                | "list.take_while" | "list.drop_while" => Value::List(Rc::new(out)),
                "list.partition" => Value::Tuple(Rc::new(vec![
                    Value::List(Rc::new(out)),
                    Value::List(Rc::new(out2)),
                ])),
                "list.find" | "list.find_index" => found
                    .map(|v| Value::Some(Rc::new(v)))
                    .unwrap_or(Value::None),
                "list.any" | "list.all" => Value::Bool(acc_bool),
                "list.count" => Value::Int(count),
                _ => unreachable!(),
            };
            Ok(hof_out(fal, bail, v))
        }
        "list.fold" | "list.scan" => {
            arity(name, &args, 3)?;
            let xs = want_list(name, &args[0])?.clone();
            let c = want_fn(name, &args[2])?.clone();
            let fal = it.cb_fallible(&c);
            let mut acc = args[1].clone();
            let mut trail: Vec<Value> = Vec::new();
            let mut bail = None;
            for x in xs.iter() {
                match call_cb(it, &c, fal, vec![acc.clone(), x.clone()])? {
                    Cb::Val(v) => {
                        acc = v;
                        trail.push(acc.clone());
                    }
                    Cb::Bail(e) => {
                        bail = Some(e);
                        break;
                    }
                }
            }
            let v = if name == "list.fold" {
                acc
            } else {
                Value::List(Rc::new(trail))
            };
            Ok(hof_out(fal, bail, v))
        }
        "list.reduce" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?.clone();
            let c = want_fn(name, &args[1])?.clone();
            let fal = it.cb_fallible(&c);
            if xs.is_empty() {
                return Ok(hof_out(fal, None, Value::None));
            }
            let mut acc = xs[0].clone();
            let mut bail = None;
            for x in xs.iter().skip(1) {
                match call_cb(it, &c, fal, vec![acc.clone(), x.clone()])? {
                    Cb::Val(v) => acc = v,
                    Cb::Bail(e) => {
                        bail = Some(e);
                        break;
                    }
                }
            }
            Ok(hof_out(fal, bail, Value::Some(Rc::new(acc))))
        }
        "list.sort_by" | "list.unique_by" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?.clone();
            let c = want_fn(name, &args[1])?.clone();
            let fal = it.cb_fallible(&c);
            let mut keyed: Vec<(Value, Value)> = Vec::with_capacity(xs.len());
            for x in xs.iter() {
                match call_cb(it, &c, fal, vec![x.clone()])? {
                    Cb::Val(k) => keyed.push((k, x.clone())),
                    Cb::Bail(e) => return Ok(Value::Err(Rc::new(e))),
                }
            }
            let v = if name == "list.sort_by" {
                let mut items: Vec<Value> = keyed
                    .iter()
                    .map(|(k, x)| Value::Tuple(Rc::new(vec![k.clone(), x.clone()])))
                    .collect();
                stable_sort_by(&mut items, |a, b| match (a, b) {
                    (Value::Tuple(p), Value::Tuple(q)) => cmp_strict(name, &p[0], &q[0]),
                    _ => Err(Flow::Fatal("sort_by internal shape".into())),
                })?;
                Value::List(Rc::new(
                    items
                        .into_iter()
                        .map(|t| match t {
                            Value::Tuple(p) => p[1].clone(),
                            _ => unreachable!(),
                        })
                        .collect(),
                ))
            } else {
                let mut seen: Vec<Value> = Vec::new();
                let mut out = Vec::new();
                for (k, x) in keyed {
                    let mut dup = false;
                    for s in &seen {
                        if eq_strict(name, &k, s)? {
                            dup = true;
                            break;
                        }
                    }
                    if !dup {
                        seen.push(k);
                        out.push(x);
                    }
                }
                Value::List(Rc::new(out))
            };
            Ok(hof_out(fal, None, v))
        }
        "list.zip_with" => {
            arity(name, &args, 3)?;
            let (a, b) = (
                want_list(name, &args[0])?.clone(),
                want_list(name, &args[1])?.clone(),
            );
            let c = want_fn(name, &args[2])?.clone();
            let fal = it.cb_fallible(&c);
            let mut out = Vec::new();
            let mut bail = None;
            for (x, y) in a.iter().zip(b.iter()) {
                match call_cb(it, &c, fal, vec![x.clone(), y.clone()])? {
                    Cb::Val(v) => out.push(v),
                    Cb::Bail(e) => {
                        bail = Some(e);
                        break;
                    }
                }
            }
            Ok(hof_out(fal, bail, Value::List(Rc::new(out))))
        }
        // in-place mutators run through Interp::call_in_place, which passes a
        // unique collection here (list.md: "in place. Requires var binding.")
        "list.push" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?;
            let mut v = (**xs).clone();
            v.push(args[1].clone());
            Ok(Value::List(Rc::new(v)))
        }
        "list.pop" => {
            arity(name, &args, 1)?;
            let _ = want_list(name, &args[0])?;
            it.abstain_pub(
                "stdlib:list.pop",
                "list.pop outside a var-binding receiver — the write-back place is not modeled",
            )
        }
        "list.clear" => {
            arity(name, &args, 1)?;
            let _ = want_list(name, &args[0])?;
            Ok(Value::List(Rc::new(Vec::new())))
        }

        // ═══ string (ALS-S1..S6, T1, T5) ══════════════════════════════
        "string.len" => {
            arity(name, &args, 1)?;
            Ok(Value::Int(want_str(name, &args[0])?.chars().count() as i64))
        }
        "string.is_empty" => {
            arity(name, &args, 1)?;
            Ok(Value::Bool(want_str(name, &args[0])?.is_empty()))
        }
        "string.concat" => {
            arity(name, &args, 2)?;
            let mut s = want_str(name, &args[0])?.to_string();
            s.push_str(want_str(name, &args[1])?);
            Ok(Value::str(&s))
        }
        "string.trim" | "string.trim_start" | "string.trim_end" => {
            arity(name, &args, 1)?;
            let cs = chars_of(want_str(name, &args[0])?);
            let mut a = 0;
            let mut b = cs.len();
            if name != "string.trim_end" {
                while a < b && is_als_whitespace(cs[a]) {
                    a += 1;
                }
            }
            if name != "string.trim_start" {
                while b > a && is_als_whitespace(cs[b - 1]) {
                    b -= 1;
                }
            }
            Ok(Value::str(&s_of(&cs[a..b])))
        }
        "string.split" => {
            arity(name, &args, 2)?;
            let cs = chars_of(want_str(name, &args[0])?);
            let sep = chars_of(want_str(name, &args[1])?);
            let mut out = Vec::new();
            if sep.is_empty() {
                out.push(Value::str(""));
                for c in &cs {
                    out.push(Value::str(&c.to_string()));
                }
                out.push(Value::str(""));
            } else {
                let mut start = 0;
                loop {
                    match find_from(&cs, &sep, start) {
                        Some(i) => {
                            out.push(Value::str(&s_of(&cs[start..i])));
                            start = i + sep.len();
                        }
                        None => {
                            out.push(Value::str(&s_of(&cs[start..])));
                            break;
                        }
                    }
                }
            }
            Ok(Value::List(Rc::new(out)))
        }
        "string.contains" | "string.starts_with" | "string.ends_with" => {
            arity(name, &args, 2)?;
            let cs = chars_of(want_str(name, &args[0])?);
            let pat = chars_of(want_str(name, &args[1])?);
            let b = match name {
                "string.contains" => find_from(&cs, &pat, 0).is_some(),
                "string.starts_with" => cs.len() >= pat.len() && cs[..pat.len()] == pat[..],
                _ => cs.len() >= pat.len() && cs[cs.len() - pat.len()..] == pat[..],
            };
            Ok(Value::Bool(b))
        }
        "string.index_of" | "string.last_index_of" => {
            arity(name, &args, 2)?;
            let cs = chars_of(want_str(name, &args[0])?);
            let pat = chars_of(want_str(name, &args[1])?);
            let hit = if name == "string.index_of" {
                find_from(&cs, &pat, 0)
            } else if pat.is_empty() {
                Some(cs.len()) // ALS-S2: empty pattern → len
            } else {
                let mut last = None;
                let mut start = 0;
                while let Some(i) = find_from(&cs, &pat, start) {
                    last = Some(i);
                    start = i + 1;
                }
                last
            };
            Ok(hit
                .map(|i| Value::Some(Rc::new(Value::Int(i as i64))))
                .unwrap_or(Value::None))
        }
        "string.count" => {
            arity(name, &args, 2)?;
            let cs = chars_of(want_str(name, &args[0])?);
            let pat = chars_of(want_str(name, &args[1])?);
            if pat.is_empty() {
                return Ok(Value::Int(cs.len() as i64 + 1)); // ALS-S2
            }
            let mut n = 0i64;
            let mut start = 0;
            while let Some(i) = find_from(&cs, &pat, start) {
                n += 1;
                start = i + pat.len();
            }
            Ok(Value::Int(n))
        }
        "string.slice" => {
            if args.len() != 2 && args.len() != 3 {
                return Err(Flow::Fatal(
                    "string.slice: expected 2 or 3 arguments".into(),
                ));
            }
            let cs = chars_of(want_str(name, &args[0])?);
            let sa = want_int(name, &args[1])?;
            // negative start clamps to 0 (string_count_truncation pins
            // `slice -1..2: [ab]`), unlike the unsigned take/drop doctrine
            let a = if sa < 0 { 0 } else { clamp_idx(sa, cs.len()) };
            let b = if args.len() == 3 {
                let sb = want_int(name, &args[2])?;
                if sb < 0 {
                    0
                } else {
                    clamp_idx(sb, cs.len())
                }
            } else {
                cs.len()
            };
            Ok(Value::str(&if a >= b {
                String::new()
            } else {
                s_of(&cs[a..b])
            }))
        }
        "string.get" | "string.first" | "string.last" => {
            arity(name, &args, if name == "string.get" { 2 } else { 1 })?;
            let cs = chars_of(want_str(name, &args[0])?);
            let i: i64 = match name {
                "string.get" => want_int(name, &args[1])?,
                "string.first" => 0,
                _ => cs.len() as i64 - 1,
            };
            Ok(if i < 0 || i as usize >= cs.len() {
                Value::None
            } else {
                Value::Some(Rc::new(Value::str(&cs[i as usize].to_string())))
            })
        }
        "string.take" | "string.drop" | "string.take_end" | "string.drop_end" => {
            arity(name, &args, 2)?;
            let cs = chars_of(want_str(name, &args[0])?);
            let n = clamp_idx(want_int(name, &args[1])?, cs.len());
            let out = match name {
                "string.take" => &cs[..n],
                "string.drop" => &cs[n..],
                "string.take_end" => &cs[cs.len() - n..],
                _ => &cs[..cs.len() - n],
            };
            Ok(Value::str(&s_of(out)))
        }
        "string.repeat" => {
            arity(name, &args, 2)?;
            let s = want_str(name, &args[0])?;
            let n = want_int(name, &args[1])?;
            // C-034: results past the shared 2^31-byte ceiling take the T6 abort
            if n > 0 && (s.len() as i128) * (n as i128) > (1i128 << 31) {
                return Err(Flow::Abort("repeat result too large".into()));
            }
            let mut out = String::new();
            for _ in 0..n.max(0) {
                out.push_str(s);
            }
            Ok(Value::str(&out))
        }
        "string.reverse" => {
            arity(name, &args, 1)?;
            let mut cs = chars_of(want_str(name, &args[0])?);
            cs.reverse();
            Ok(Value::str(&s_of(&cs)))
        }
        "string.replace" | "string.replace_first" => {
            arity(name, &args, 3)?;
            let cs = chars_of(want_str(name, &args[0])?);
            let from = chars_of(want_str(name, &args[1])?);
            let to = want_str(name, &args[2])?.to_string();
            let first_only = name == "string.replace_first";
            let mut out = String::new();
            let mut start = 0usize;
            let mut replaced = false;
            loop {
                let hit = if replaced && first_only {
                    None
                } else {
                    find_from(&cs, &from, start)
                };
                match hit {
                    Some(i) => {
                        out.push_str(&s_of(&cs[start..i]));
                        out.push_str(&to);
                        replaced = true;
                        if from.is_empty() {
                            // empty pattern: a match at every boundary — emit the
                            // char that follows and step past it
                            if i < cs.len() {
                                out.push(cs[i]);
                            }
                            start = i + 1;
                            if start > cs.len() {
                                break;
                            }
                        } else {
                            start = i + from.len();
                        }
                    }
                    None => {
                        out.push_str(&s_of(&cs[start.min(cs.len())..]));
                        break;
                    }
                }
            }
            Ok(Value::str(&out))
        }
        "string.strip_prefix" | "string.strip_suffix" => {
            arity(name, &args, 2)?;
            let cs = chars_of(want_str(name, &args[0])?);
            let pat = chars_of(want_str(name, &args[1])?);
            let hit = if name == "string.strip_prefix" {
                (cs.len() >= pat.len() && cs[..pat.len()] == pat[..])
                    .then(|| s_of(&cs[pat.len()..]))
            } else {
                (cs.len() >= pat.len() && cs[cs.len() - pat.len()..] == pat[..])
                    .then(|| s_of(&cs[..cs.len() - pat.len()]))
            };
            Ok(hit
                .map(|s| Value::Some(Rc::new(Value::str(&s))))
                .unwrap_or(Value::None))
        }
        "string.pad_start" | "string.pad_end" => {
            arity(name, &args, 3)?;
            let cs = chars_of(want_str(name, &args[0])?);
            let n = want_int(name, &args[1])?;
            let ch = chars_of(want_str(name, &args[2])?);
            // the fill is the FIRST char of the pad string, or a space when
            // empty (string_codepoint: pad.chars().next().unwrap_or(' '))
            let fill = ch.first().copied().unwrap_or(' ');
            let need = (n.max(0) as usize).saturating_sub(cs.len());
            if need > (1usize << 31) {
                return Err(Flow::Abort("out of memory".into()));
            }
            let pad: String = std::iter::repeat_n(fill, need).collect();
            Ok(Value::str(&if name == "string.pad_start" {
                format!("{}{}", pad, s_of(&cs))
            } else {
                format!("{}{}", s_of(&cs), pad)
            }))
        }
        "string.chars" => {
            arity(name, &args, 1)?;
            Ok(Value::List(Rc::new(
                want_str(name, &args[0])?
                    .chars()
                    .map(|c| Value::str(&c.to_string()))
                    .collect(),
            )))
        }
        "string.lines" => {
            arity(name, &args, 1)?;
            let s = want_str(name, &args[0])?;
            let mut out = Vec::new();
            let mut cur = String::new();
            for c in s.chars() {
                if c == '\n' {
                    if cur.ends_with('\r') {
                        cur.pop();
                    }
                    out.push(Value::str(&cur));
                    cur.clear();
                } else {
                    cur.push(c);
                }
            }
            if !cur.is_empty() {
                if cur.ends_with('\r') {
                    cur.pop();
                }
                out.push(Value::str(&cur));
            }
            Ok(Value::List(Rc::new(out)))
        }
        "string.to_bytes" => {
            arity(name, &args, 1)?;
            Ok(Value::List(Rc::new(
                want_str(name, &args[0])?
                    .bytes()
                    .map(|b| Value::Int(b as i64))
                    .collect(),
            )))
        }
        "string.from_bytes" => {
            arity(name, &args, 1)?;
            let xs = want_list(name, &args[0])?.clone();
            let mut bytes = Vec::with_capacity(xs.len());
            for x in xs.iter() {
                bytes.push(want_int(name, x)? as u8);
            }
            Ok(Value::str(&String::from_utf8_lossy(&bytes)))
        }
        "string.capitalize" => {
            arity(name, &args, 1)?;
            let mut cs = want_str(name, &args[0])?.chars();
            Ok(Value::str(&match cs.next() {
                Some(f) => f.to_uppercase().collect::<String>() + cs.as_str(),
                None => String::new(),
            }))
        }
        "string.to_upper" => {
            arity(name, &args, 1)?;
            Ok(Value::str(
                &want_str(name, &args[0])?
                    .chars()
                    .flat_map(|c| c.to_uppercase())
                    .collect::<String>(),
            ))
        }
        "string.to_lower" => {
            arity(name, &args, 1)?;
            // ALS-T5: full lowercase + the Final_Sigma context rule
            let cs = chars_of(want_str(name, &args[0])?);
            let mut out = String::new();
            for (i, &c) in cs.iter().enumerate() {
                if c == 'Σ' {
                    let before_cased = cs[..i]
                        .iter()
                        .rev()
                        .find(|ch| !is_case_ignorable(**ch))
                        .map(|ch| is_cased(*ch))
                        .unwrap_or(false);
                    let after_cased = cs[i + 1..]
                        .iter()
                        .find(|ch| !is_case_ignorable(**ch))
                        .map(|ch| is_cased(*ch))
                        .unwrap_or(false);
                    if before_cased && !after_cased {
                        out.push('ς');
                        continue;
                    }
                }
                out.extend(c.to_lowercase());
            }
            Ok(Value::str(&out))
        }
        "string.is_digit"
        | "string.is_alpha"
        | "string.is_alphanumeric"
        | "string.is_whitespace" => {
            arity(name, &args, 1)?;
            // string_ops_drain pins the lifts: is_digit is ASCII-only and
            // empty-false; is_alpha/is_alnum are Unicode and empty-false;
            // is_whitespace is Unicode and empty-TRUE (vacuous)
            let s = want_str(name, &args[0])?;
            let ok = if name == "string.is_whitespace" {
                s.chars().all(|c| c.is_whitespace())
            } else {
                !s.is_empty()
                    && s.chars().all(|c| match name {
                        "string.is_digit" => c.is_ascii_digit(),
                        "string.is_alpha" => c.is_alphabetic(),
                        _ => c.is_alphanumeric(),
                    })
            };
            Ok(Value::Bool(ok))
        }
        "string.is_upper" | "string.is_lower" => {
            arity(name, &args, 1)?;
            // string_predicates pins the string lift: uncased chars are
            // ignored ("A.B" is upper) — the string equals its own mapping
            // string_predicates pins the Python-style lift: at least one
            // cased char, and every cased char has the property (uncased
            // chars are ignored — "A.B" is upper, "123" is neither)
            let s = want_str(name, &args[0])?;
            let mut any_cased = false;
            let mut all_hold = true;
            for c in s.chars() {
                let cased = c.is_lowercase() || c.is_uppercase();
                if cased {
                    any_cased = true;
                    let hold = if name == "string.is_upper" {
                        c.is_uppercase()
                    } else {
                        c.is_lowercase()
                    };
                    if !hold {
                        all_hold = false;
                        break;
                    }
                }
            }
            Ok(Value::Bool(any_cased && all_hold))
        }
        "string.codepoint" => {
            arity(name, &args, 1)?;
            Ok(want_str(name, &args[0])?
                .chars()
                .next()
                .map(|c| Value::Some(Rc::new(Value::Int(c as i64))))
                .unwrap_or(Value::None))
        }
        "string.from_codepoint" => {
            arity(name, &args, 1)?;
            let n = want_int(name, &args[0])?;
            // surrogates, past-max, and out-of-range all yield "" (value_domain_arith)
            match u32::try_from(n).ok().and_then(char::from_u32) {
                Some(c) => Ok(Value::str(&c.to_string())),
                None => Ok(Value::str("")),
            }
        }

        // ═══ int (ALS-T6, T8, T14, T16) ═══════════════════════════════
        "int.to_string" => {
            arity(name, &args, 1)?;
            Ok(Value::str(&fmt_int(want_int(name, &args[0])?)))
        }
        "int.parse" => {
            arity(name, &args, 1)?;
            Ok(parse_i64(want_str(name, &args[0])?))
        }

        "int.from_hex" => {
            arity(name, &args, 1)?;
            // int_from_hex pins the real grammar (T8's from_str_radix claim
            // is inexact — a finding): whitespace is trimmed, LOWERCASE "0x"
            // prefixes strip repeatedly ("0x0x0x10" parses), "0X" does not,
            // and the sign may follow the prefix ("0x-ff" = -255)
            let s0 = want_str(name, &args[0])?;
            let all: Vec<char> = s0.chars().collect();
            let mut lo = 0;
            let mut hi = all.len();
            while lo < hi && is_als_whitespace(all[lo]) {
                lo += 1;
            }
            while hi > lo && is_als_whitespace(all[hi - 1]) {
                hi -= 1;
            }
            let mut cs: &[char] = &all[lo..hi];
            while cs.len() >= 2 && cs[0] == '0' && cs[1] == 'x' {
                cs = &cs[2..];
            }
            if cs.is_empty() {
                return Ok(Value::Err(Rc::new(Value::str(
                    "cannot parse integer from empty string",
                ))));
            }
            let (neg, digits) = match cs[0] {
                '+' => (false, &cs[1..]),
                '-' => (true, &cs[1..]),
                _ => (false, cs),
            };
            if digits.is_empty() {
                return Ok(Value::Err(Rc::new(Value::str(
                    "invalid digit found in string",
                ))));
            }
            let mut acc: i64 = 0;
            for &c in digits {
                let d = match c.to_digit(16) {
                    Some(d) => d as i64,
                    None => {
                        return Ok(Value::Err(Rc::new(Value::str(
                            "invalid digit found in string",
                        ))))
                    }
                };
                acc = match acc.checked_mul(16).and_then(|a| {
                    if neg {
                        a.checked_sub(d)
                    } else {
                        a.checked_add(d)
                    }
                }) {
                    Some(a) => a,
                    None => {
                        return Ok(Value::Err(Rc::new(Value::str(if neg {
                            "number too small to fit in target type"
                        } else {
                            "number too large to fit in target type"
                        }))))
                    }
                };
            }
            Ok(Value::Ok(Rc::new(Value::Int(acc))))
        }
        "int.abs" | "math.abs" => {
            arity(name, &args, 1)?;
            match &args[0] {
                Value::Int(n) => match n.checked_abs() {
                    Some(v) => Ok(Value::Int(v)),
                    None => Err(Flow::Abort("integer overflow".into())),
                },
                Value::Float(f) if name == "math.abs" => Ok(Value::Float(F64(f.0.abs()))),
                other => mismatch(name, "Int", other),
            }
        }
        "int.min" | "int.max" | "math.min" | "math.max" => {
            arity(name, &args, 2)?;
            match (&args[0], &args[1]) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(if name.ends_with("min") {
                    *a.min(b)
                } else {
                    *a.max(b)
                })),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(F64(ieee_min_max(
                    a.0,
                    b.0,
                    name.ends_with("max"),
                )))),
                (a, _) => mismatch(name, "two Ints or two Floats", a),
            }
        }
        "int.band" | "int.bor" | "int.bxor" => {
            arity(name, &args, 2)?;
            let (a, b) = (want_int(name, &args[0])?, want_int(name, &args[1])?);
            Ok(Value::Int(match name {
                "int.band" => a & b,
                "int.bor" => a | b,
                _ => a ^ b,
            }))
        }
        "int.bnot" => {
            arity(name, &args, 1)?;
            Ok(Value::Int(!want_int(name, &args[0])?))
        }
        "int.bshl" | "int.bshr" => {
            arity(name, &args, 2)?;
            let (a, n) = (want_int(name, &args[0])?, want_int(name, &args[1])?);
            if !(0..64).contains(&n) {
                return it.abstain_pub("semantics:int-shift-range", "shift count outside 0..64 — the rule is not in a chapter this evaluator has read");
            }
            Ok(Value::Int(if name == "int.bshl" {
                ((a as u64) << n) as i64
            } else {
                a >> n
            }))
        }
        "int.wrap_add" | "int.wrap_mul" => {
            arity(name, &args, 3)?;
            let (a, b, bits) = (
                want_int(name, &args[0])?,
                want_int(name, &args[1])?,
                want_int(name, &args[2])?,
            );
            if bits <= 0 {
                return it.abstain_pub("semantics:wrap-nonpositive-bits", "wrap_* with bits <= 0 — T14 points at the T6 abort but the message is not named");
            }
            let mask: u64 = if bits >= 64 {
                u64::MAX
            } else {
                (1u64 << bits) - 1
            }; // ALS-T14 saturation
            let r = if name == "int.wrap_add" {
                (a as u64).wrapping_add(b as u64)
            } else {
                (a as u64).wrapping_mul(b as u64)
            };
            Ok(Value::Int((r & mask) as i64))
        }
        "int.rotate_left" | "int.rotate_right" => {
            arity(name, &args, 3)?;
            let (a, n, bits) = (
                want_int(name, &args[0])?,
                want_int(name, &args[1])?,
                want_int(name, &args[2])?,
            );
            if bits <= 0 {
                return Err(Flow::Abort("rotate width must be positive".into()));
                // ALS-T6
            }
            // T14: only the MASK saturates; the shift distances use the width
            // as written, with hardware shift semantics (amount mod 64) —
            // int_wrap_rotate_width pins rotate_left(1, 1, 65) = 3
            let mask: u64 = if bits >= 64 {
                u64::MAX
            } else {
                (1u64 << bits) - 1
            };
            let x = (a as u64) & mask;
            let s = n.rem_euclid(bits) as u32;
            let r = if s == 0 {
                x
            } else if name == "int.rotate_left" {
                (x.wrapping_shl(s) | x.wrapping_shr((bits as u32).wrapping_sub(s) % 64)) & mask
            } else {
                (x.wrapping_shr(s) | x.wrapping_shl((bits as u32).wrapping_sub(s) % 64)) & mask
            };
            Ok(Value::Int(r as i64))
        }
        "int.to_u32" => {
            arity(name, &args, 1)?;
            Ok(Value::Int((want_int(name, &args[0])? as u32) as i64))
        }
        "int.to_u8" => {
            arity(name, &args, 1)?;
            Ok(Value::Int((want_int(name, &args[0])? as u8) as i64))
        }
        "int.clamp" => {
            arity(name, &args, 3)?;
            let (n, lo, hi) = (
                want_int(name, &args[0])?,
                want_int(name, &args[1])?,
                want_int(name, &args[2])?,
            );
            if lo > hi {
                return Err(Flow::Abort("clamp requires min <= max".into())); // ALS-T6
            }
            Ok(Value::Int(n.clamp(lo, hi)))
        }
        "int.to_float" => {
            arity(name, &args, 1)?;
            Ok(Value::Float(F64(want_int(name, &args[0])? as f64)))
        }
        "math.pow" => {
            arity(name, &args, 2)?;
            let (b, e) = (want_int(name, &args[0])?, want_int(name, &args[1])?);
            if e < 0 {
                return Err(Flow::Abort("negative exponent".into())); // ALS-T6
            }
            // wraps two's-complement on overflow (int_pow_overflow_wraps)
            let mut acc: i64 = 1;
            let mut base = b;
            let mut exp = e as u64;
            while exp > 0 {
                if exp & 1 == 1 {
                    acc = acc.wrapping_mul(base);
                }
                exp >>= 1;
                if exp > 0 {
                    base = base.wrapping_mul(base);
                }
            }
            Ok(Value::Int(acc))
        }
        "math.sqrt" | "float.sqrt" => {
            arity(name, &args, 1)?;
            // ALS-T22: sqrt is correctly rounded (0 ulp)
            Ok(Value::Float(F64(want_float(name, &args[0])?.sqrt())))
        }
        "math.fmin" | "math.fmax" => {
            arity(name, &args, 2)?;
            let (a, b) = (want_float(name, &args[0])?, want_float(name, &args[1])?);
            Ok(Value::Float(F64(ieee_min_max(a, b, name == "math.fmax"))))
        }

        // ═══ float (ALS-T2, T9, T13, T15, T23, T24) ═══════════════════
        "float.to_string" => {
            arity(name, &args, 1)?;
            Ok(Value::str(&fmtfloat::to_string_form(F64(want_float(
                name, &args[0],
            )?))))
        }
        "float.to_int" => {
            arity(name, &args, 1)?;
            // ALS-T24: truncate toward zero, saturate, NaN → 0 (Rust `as` semantics)
            let x = want_float(name, &args[0])?;
            Ok(Value::Int(x as i64))
        }
        "float.to_int64_checked" => {
            arity(name, &args, 1)?;
            let x = want_float(name, &args[0])?;
            Ok(
                if x.is_finite() && x.trunc() == x && x >= -(2f64.powi(63)) && x < 2f64.powi(63) {
                    Value::Some(Rc::new(Value::Int(x as i64)))
                } else {
                    Value::None
                },
            )
        }
        "float.round" => {
            arity(name, &args, 1)?;
            Ok(Value::Float(F64(want_float(name, &args[0])?.round()))) // T15: half away, sign kept
        }
        "float.floor" => {
            arity(name, &args, 1)?;
            Ok(Value::Float(F64(want_float(name, &args[0])?.floor())))
        }
        "float.ceil" => {
            arity(name, &args, 1)?;
            Ok(Value::Float(F64(want_float(name, &args[0])?.ceil())))
        }
        "float.abs" => {
            arity(name, &args, 1)?;
            Ok(Value::Float(F64(want_float(name, &args[0])?.abs())))
        }
        "float.from_int" => {
            arity(name, &args, 1)?;
            Ok(Value::Float(F64(want_int(name, &args[0])? as f64)))
        }
        "float.min" | "float.max" => {
            arity(name, &args, 2)?;
            let (a, b) = (want_float(name, &args[0])?, want_float(name, &args[1])?);
            Ok(Value::Float(F64(ieee_min_max(a, b, name == "float.max"))))
        }
        "float.to_fixed" => {
            arity(name, &args, 2)?;
            let x = F64(want_float(name, &args[0])?);
            let n = want_int(name, &args[1])?;
            // ALS-T9: a non-finite value renders as its display form
            if !x.0.is_finite() {
                return Ok(Value::str(&fmtfloat::display_form(x)));
            }
            // ALS-T9: the digit-count domain is 0..=4096, T6 form outside it
            if !(0..=4096).contains(&n) {
                return Err(Flow::Abort("to_fixed requires decimals in 0..=4096".into()));
            }
            match fmtfloat::to_fixed(x, n) {
                Some(s) => Ok(Value::str(&s)),
                None => it.abstain_pub("semantics:to-fixed-domain", "to_fixed answered nothing for a finite in-domain input — unreachable by ALS-T9"),
            }
        }
        "float.clamp" => {
            arity(name, &args, 3)?;
            let (x, lo, hi) = (
                want_float(name, &args[0])?,
                want_float(name, &args[1])?,
                want_float(name, &args[2])?,
            );
            // ALS-T6: lo > hi aborts, and NaN bounds are an invalid range too
            if lo.partial_cmp(&hi) != Some(Ordering::Less)
                && lo.partial_cmp(&hi) != Some(Ordering::Equal)
            {
                return Err(Flow::Abort("clamp requires min <= max".into()));
            }
            Ok(Value::Float(F64(if x.is_nan() {
                x
            } else if x < lo {
                lo
            } else if x > hi {
                hi
            } else {
                x
            })))
        }
        "float.sign" => {
            arity(name, &args, 1)?;
            Ok(Value::Float(F64(want_float(name, &args[0])?.signum()))) // ALS-T15
        }
        "float.is_nan" => {
            arity(name, &args, 1)?;
            Ok(Value::Bool(want_float(name, &args[0])?.is_nan()))
        }
        "float.is_infinite" => {
            arity(name, &args, 1)?;
            Ok(Value::Bool(want_float(name, &args[0])?.is_infinite()))
        }
        "float.to_bits" => {
            arity(name, &args, 1)?;
            let x = want_float(name, &args[0])?;
            let bits = if x.is_nan() {
                0x7FF8000000000000u64
            } else {
                x.to_bits()
            }; // C-210 canonical NaN
            Ok(Value::Int(bits as i64))
        }
        "float.bits_to_float" => {
            arity(name, &args, 1)?;
            Ok(Value::Float(F64(f64::from_bits(
                want_int(name, &args[0])? as u64
            ))))
        }
        "float.parse" => {
            arity(name, &args, 1)?;
            parse_f64(it, want_str(name, &args[0])?)
        }

        // ═══ option / result ══════════════════════════════════════════
        "option.is_some" | "option.is_none" => {
            arity(name, &args, 1)?;
            match &args[0] {
                Value::Some(_) => Ok(Value::Bool(name == "option.is_some")),
                Value::None => Ok(Value::Bool(name == "option.is_none")),
                other => mismatch(name, "Option", other),
            }
        }
        "option.unwrap_or" => {
            arity(name, &args, 2)?;
            match &args[0] {
                Value::Some(v) => Ok((**v).clone()),
                Value::None => Ok(args[1].clone()),
                other => mismatch(name, "Option", other),
            }
        }
        "option.unwrap_or_else" => {
            arity(name, &args, 2)?;
            match &args[0] {
                Value::Some(v) => Ok((**v).clone()),
                Value::None => {
                    let c = want_fn(name, &args[1])?.clone();
                    it.call_value(&c, vec![])
                }
                other => mismatch(name, "Option", other),
            }
        }
        "option.map" | "option.flat_map" | "option.filter" => {
            arity(name, &args, 2)?;
            let c = want_fn(name, &args[1])?.clone();
            let fal = it.cb_fallible(&c);
            match &args[0] {
                Value::None => Ok(hof_out(fal, None, Value::None)),
                Value::Some(v) => {
                    let inner = (**v).clone();
                    match call_cb(it, &c, fal, vec![inner.clone()])? {
                        Cb::Bail(e) => Ok(Value::Err(Rc::new(e))),
                        Cb::Val(r) => {
                            let out = match name {
                                "option.map" => Value::Some(Rc::new(r)),
                                "option.flat_map" => r,
                                _ => {
                                    if want_bool(name, &r)? {
                                        Value::Some(Rc::new(inner))
                                    } else {
                                        Value::None
                                    }
                                }
                            };
                            Ok(hof_out(fal, None, out))
                        }
                    }
                }
                other => mismatch(name, "Option", other),
            }
        }
        "option.flatten" => {
            arity(name, &args, 1)?;
            match &args[0] {
                Value::Some(v) => match &**v {
                    Value::Some(_) | Value::None => Ok((**v).clone()),
                    other => mismatch(name, "Option[Option]", other),
                },
                Value::None => Ok(Value::None),
                other => mismatch(name, "Option", other),
            }
        }
        "option.to_result" => {
            arity(name, &args, 2)?;
            match &args[0] {
                Value::Some(v) => Ok(Value::Ok(v.clone())),
                Value::None => Ok(Value::Err(Rc::new(args[1].clone()))),
                other => mismatch(name, "Option", other),
            }
        }
        "option.zip" => {
            arity(name, &args, 2)?;
            match (&args[0], &args[1]) {
                (Value::Some(a), Value::Some(b)) => {
                    Ok(Value::Some(Rc::new(Value::Tuple(Rc::new(vec![
                        (**a).clone(),
                        (**b).clone(),
                    ])))))
                }
                (Value::Some(_) | Value::None, Value::Some(_) | Value::None) => Ok(Value::None),
                (other, _) => mismatch(name, "Option", other),
            }
        }
        "option.or_else" => {
            arity(name, &args, 2)?;
            match &args[0] {
                Value::Some(_) => Ok(args[0].clone()),
                Value::None => {
                    let c = want_fn(name, &args[1])?.clone();
                    it.call_value(&c, vec![])
                }
                other => mismatch(name, "Option", other),
            }
        }
        "option.to_list" => {
            arity(name, &args, 1)?;
            match &args[0] {
                Value::Some(v) => Ok(Value::List(Rc::new(vec![(**v).clone()]))),
                Value::None => Ok(Value::List(Rc::new(vec![]))),
                other => mismatch(name, "Option", other),
            }
        }
        "result.is_ok" | "result.is_err" => {
            arity(name, &args, 1)?;
            match &args[0] {
                Value::Ok(_) => Ok(Value::Bool(name == "result.is_ok")),
                Value::Err(_) => Ok(Value::Bool(name == "result.is_err")),
                other => mismatch(name, "Result", other),
            }
        }
        "result.unwrap_or" => {
            arity(name, &args, 2)?;
            match &args[0] {
                Value::Ok(v) => Ok((**v).clone()),
                Value::Err(_) => Ok(args[1].clone()),
                other => mismatch(name, "Result", other),
            }
        }
        "result.unwrap_or_else" => {
            arity(name, &args, 2)?;
            match &args[0] {
                Value::Ok(v) => Ok((**v).clone()),
                Value::Err(e) => {
                    let c = want_fn(name, &args[1])?.clone();
                    it.call_value(&c, vec![(**e).clone()])
                }
                other => mismatch(name, "Result", other),
            }
        }
        "result.map" | "result.map_err" | "result.flat_map" => {
            arity(name, &args, 2)?;
            let c = want_fn(name, &args[1])?.clone();
            let fal = it.cb_fallible(&c);
            let (is_ok, inner) = match &args[0] {
                Value::Ok(v) => (true, (**v).clone()),
                Value::Err(e) => (false, (**e).clone()),
                other => return mismatch(name, "Result", other),
            };
            let active = match name {
                "result.map" | "result.flat_map" => is_ok,
                _ => !is_ok,
            };
            if !active {
                return Ok(args[0].clone());
            }
            match call_cb(it, &c, fal, vec![inner])? {
                Cb::Bail(e) => Ok(Value::Err(Rc::new(e))),
                Cb::Val(r) => Ok(match name {
                    "result.map" => Value::Ok(Rc::new(r)),
                    "result.map_err" => Value::Err(Rc::new(r)),
                    _ => r,
                }),
            }
        }
        "result.to_option" => {
            arity(name, &args, 1)?;
            match &args[0] {
                Value::Ok(v) => Ok(Value::Some(v.clone())),
                Value::Err(_) => Ok(Value::None),
                other => mismatch(name, "Result", other),
            }
        }
        "result.to_err_option" => {
            arity(name, &args, 1)?;
            match &args[0] {
                Value::Ok(_) => Ok(Value::None),
                Value::Err(e) => Ok(Value::Some(e.clone())),
                other => mismatch(name, "Result", other),
            }
        }
        "result.partition" => {
            arity(name, &args, 1)?;
            let xs = want_list(name, &args[0])?;
            let mut oks = Vec::new();
            let mut errs = Vec::new();
            for x in xs.iter() {
                match x {
                    Value::Ok(v) => oks.push((**v).clone()),
                    Value::Err(e) => errs.push((**e).clone()),
                    other => return mismatch(name, "List[Result]", other),
                }
            }
            Ok(Value::Tuple(Rc::new(vec![
                Value::List(Rc::new(oks)),
                Value::List(Rc::new(errs)),
            ])))
        }

        // ═══ map (ALS-C1: insertion order; upsert keeps position) ═════
        "map.new" => {
            arity(name, &args, 0)?;
            Ok(Value::Map(Rc::new(Vec::new())))
        }
        "map.len" => {
            arity(name, &args, 1)?;
            Ok(Value::Int(want_map(name, &args[0])?.len() as i64))
        }
        "map.is_empty" => {
            arity(name, &args, 1)?;
            Ok(Value::Bool(want_map(name, &args[0])?.is_empty()))
        }
        "map.get" | "map.get_or" | "map.contains" => {
            arity(name, &args, if name == "map.get_or" { 3 } else { 2 })?;
            let m = want_map(name, &args[0])?.clone();
            for (k, v) in m.iter() {
                if eq_strict(name, k, &args[1])? {
                    return Ok(match name {
                        "map.get" => Value::Some(Rc::new(v.clone())),
                        "map.get_or" => v.clone(),
                        _ => Value::Bool(true),
                    });
                }
            }
            Ok(match name {
                "map.get" => Value::None,
                "map.get_or" => args[2].clone(),
                _ => Value::Bool(false),
            })
        }
        "map.set" | "map.insert" => {
            arity(name, &args, 3)?;
            let m = want_map(name, &args[0])?;
            let mut v = (**m).clone();
            let mut hit = false;
            for slot in v.iter_mut() {
                if eq_strict(name, &slot.0, &args[1])? {
                    slot.1 = args[2].clone();
                    hit = true;
                    break;
                }
            }
            if !hit {
                v.push((args[1].clone(), args[2].clone()));
            }
            Ok(Value::Map(Rc::new(v)))
        }
        "map.remove" | "map.delete" => {
            arity(name, &args, 2)?;
            let m = want_map(name, &args[0])?;
            let mut v = Vec::with_capacity(m.len());
            for (k, val) in m.iter() {
                if !eq_strict(name, k, &args[1])? {
                    v.push((k.clone(), val.clone()));
                }
            }
            Ok(Value::Map(Rc::new(v)))
        }
        "map.clear" => {
            arity(name, &args, 1)?;
            let _ = want_map(name, &args[0])?;
            Ok(Value::Map(Rc::new(Vec::new())))
        }
        "map.keys" | "map.values" => {
            arity(name, &args, 1)?;
            let m = want_map(name, &args[0])?;
            Ok(Value::List(Rc::new(
                m.iter()
                    .map(|(k, v)| {
                        if name == "map.keys" {
                            k.clone()
                        } else {
                            v.clone()
                        }
                    })
                    .collect(),
            )))
        }
        "map.entries" => {
            arity(name, &args, 1)?;
            let m = want_map(name, &args[0])?;
            Ok(Value::List(Rc::new(
                m.iter()
                    .map(|(k, v)| Value::Tuple(Rc::new(vec![k.clone(), v.clone()])))
                    .collect(),
            )))
        }
        "map.merge" => {
            arity(name, &args, 2)?;
            let a = want_map(name, &args[0])?;
            let b = want_map(name, &args[1])?.clone();
            let mut v = (**a).clone();
            for (k, val) in b.iter() {
                let mut hit = false;
                for slot in v.iter_mut() {
                    if eq_strict(name, &slot.0, k)? {
                        slot.1 = val.clone();
                        hit = true;
                        break;
                    }
                }
                if !hit {
                    v.push((k.clone(), val.clone()));
                }
            }
            Ok(Value::Map(Rc::new(v)))
        }
        "map.from_list" => {
            arity(name, &args, 1)?;
            let xs = want_list(name, &args[0])?.clone();
            let mut v: Vec<(Value, Value)> = Vec::new();
            for x in xs.iter() {
                let (k, val) = match x {
                    Value::Tuple(p) if p.len() == 2 => (p[0].clone(), p[1].clone()),
                    other => return mismatch(name, "List[(K, V)]", other),
                };
                let mut hit = false;
                for slot in v.iter_mut() {
                    if eq_strict(name, &slot.0, &k)? {
                        slot.1 = val.clone();
                        hit = true;
                        break;
                    }
                }
                if !hit {
                    v.push((k, val));
                }
            }
            Ok(Value::Map(Rc::new(v)))
        }
        "map.update" => {
            arity(name, &args, 3)?;
            let m = want_map(name, &args[0])?.clone();
            let c = want_fn(name, &args[2])?.clone();
            let fal = it.cb_fallible(&c);
            let mut v = (*m).clone();
            for slot in v.iter_mut() {
                if eq_strict(name, &slot.0, &args[1])? {
                    match call_cb(it, &c, fal, vec![slot.1.clone()])? {
                        Cb::Val(nv) => slot.1 = nv,
                        Cb::Bail(e) => return Ok(Value::Err(Rc::new(e))),
                    }
                    break;
                }
            }
            Ok(hof_out(fal, None, Value::Map(Rc::new(v))))
        }
        "map.map" | "map.filter" | "map.any" | "map.all" | "map.count" | "map.find" => {
            arity(name, &args, 2)?;
            let m = want_map(name, &args[0])?.clone();
            let c = want_fn(name, &args[1])?.clone();
            let fal = it.cb_fallible(&c);
            let mut bail = None;
            let mut out: Vec<(Value, Value)> = Vec::new();
            let mut acc_bool = name == "map.all";
            let mut count = 0i64;
            let mut found: Option<Value> = None;
            for (k, v) in m.iter() {
                let cb_args = if name == "map.map" {
                    vec![v.clone()]
                } else {
                    vec![k.clone(), v.clone()]
                };
                let r = match call_cb(it, &c, fal, cb_args)? {
                    Cb::Val(r) => r,
                    Cb::Bail(e) => {
                        bail = Some(e);
                        break;
                    }
                };
                match name {
                    "map.map" => out.push((k.clone(), r)),
                    "map.filter" => {
                        if want_bool(name, &r)? {
                            out.push((k.clone(), v.clone()));
                        }
                    }
                    "map.any" => {
                        if want_bool(name, &r)? {
                            acc_bool = true;
                            break;
                        }
                    }
                    "map.all" => {
                        if !want_bool(name, &r)? {
                            acc_bool = false;
                            break;
                        }
                    }
                    "map.count" => {
                        if want_bool(name, &r)? {
                            count += 1;
                        }
                    }
                    "map.find" => {
                        if want_bool(name, &r)? {
                            found = Some(Value::Tuple(Rc::new(vec![k.clone(), v.clone()])));
                            break;
                        }
                    }
                    _ => unreachable!(),
                }
            }
            let v = match name {
                "map.map" | "map.filter" => Value::Map(Rc::new(out)),
                "map.any" | "map.all" => Value::Bool(acc_bool),
                "map.count" => Value::Int(count),
                _ => found
                    .map(|t| Value::Some(Rc::new(t)))
                    .unwrap_or(Value::None),
            };
            Ok(hof_out(fal, bail, v))
        }
        "map.fold" => {
            arity(name, &args, 3)?;
            let m = want_map(name, &args[0])?.clone();
            let c = want_fn(name, &args[2])?.clone();
            let fal = it.cb_fallible(&c);
            let mut acc = args[1].clone();
            let mut bail = None;
            for (k, v) in m.iter() {
                match call_cb(it, &c, fal, vec![acc.clone(), k.clone(), v.clone()])? {
                    Cb::Val(nv) => acc = nv,
                    Cb::Bail(e) => {
                        bail = Some(e);
                        break;
                    }
                }
            }
            Ok(hof_out(fal, bail, acc))
        }

        // ═══ set (ALS-C2: insertion order) ════════════════════════════
        "set.new" => {
            arity(name, &args, 0)?;
            Ok(Value::Set(Rc::new(Vec::new())))
        }
        "set.from_list" => {
            arity(name, &args, 1)?;
            let xs = want_list(name, &args[0])?.clone();
            let mut v: Vec<Value> = Vec::new();
            for x in xs.iter() {
                let mut dup = false;
                for y in &v {
                    if eq_strict(name, x, y)? {
                        dup = true;
                        break;
                    }
                }
                if !dup {
                    v.push(x.clone());
                }
            }
            Ok(Value::Set(Rc::new(v)))
        }
        "set.to_list" => {
            arity(name, &args, 1)?;
            Ok(Value::List(Rc::new((**want_set(name, &args[0])?).clone())))
        }
        "set.len" => {
            arity(name, &args, 1)?;
            Ok(Value::Int(want_set(name, &args[0])?.len() as i64))
        }
        "set.is_empty" => {
            arity(name, &args, 1)?;
            Ok(Value::Bool(want_set(name, &args[0])?.is_empty()))
        }
        "set.contains" => {
            arity(name, &args, 2)?;
            let s = want_set(name, &args[0])?.clone();
            for x in s.iter() {
                if eq_strict(name, x, &args[1])? {
                    return Ok(Value::Bool(true));
                }
            }
            Ok(Value::Bool(false))
        }
        "set.insert" => {
            arity(name, &args, 2)?;
            let s = want_set(name, &args[0])?.clone();
            for x in s.iter() {
                if eq_strict(name, x, &args[1])? {
                    return Ok(Value::Set(s.clone()));
                }
            }
            let mut v = (*s).clone();
            v.push(args[1].clone());
            Ok(Value::Set(Rc::new(v)))
        }
        "set.remove" => {
            arity(name, &args, 2)?;
            let s = want_set(name, &args[0])?.clone();
            let mut v = Vec::with_capacity(s.len());
            for x in s.iter() {
                if !eq_strict(name, x, &args[1])? {
                    v.push(x.clone());
                }
            }
            Ok(Value::Set(Rc::new(v)))
        }
        "set.union"
        | "set.intersection"
        | "set.difference"
        | "set.symmetric_difference"
        | "set.is_subset"
        | "set.is_disjoint" => {
            arity(name, &args, 2)?;
            let a = want_set(name, &args[0])?.clone();
            let b = want_set(name, &args[1])?.clone();
            let contains = |xs: &[Value], v: &Value| -> Result<bool, Flow> {
                for x in xs {
                    if eq_strict(name, x, v)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            };
            match name {
                "set.union" => {
                    let mut v = (*a).clone();
                    for x in b.iter() {
                        if !contains(&v, x)? {
                            v.push(x.clone());
                        }
                    }
                    Ok(Value::Set(Rc::new(v)))
                }
                "set.intersection" => {
                    let mut v = Vec::new();
                    for x in a.iter() {
                        if contains(&b, x)? {
                            v.push(x.clone());
                        }
                    }
                    Ok(Value::Set(Rc::new(v)))
                }
                "set.difference" => {
                    let mut v = Vec::new();
                    for x in a.iter() {
                        if !contains(&b, x)? {
                            v.push(x.clone());
                        }
                    }
                    Ok(Value::Set(Rc::new(v)))
                }
                "set.symmetric_difference" => {
                    let mut v = Vec::new();
                    for x in a.iter() {
                        if !contains(&b, x)? {
                            v.push(x.clone());
                        }
                    }
                    for x in b.iter() {
                        if !contains(&a, x)? {
                            v.push(x.clone());
                        }
                    }
                    Ok(Value::Set(Rc::new(v)))
                }
                "set.is_subset" => {
                    for x in a.iter() {
                        if !contains(&b, x)? {
                            return Ok(Value::Bool(false));
                        }
                    }
                    Ok(Value::Bool(true))
                }
                _ => {
                    for x in a.iter() {
                        if contains(&b, x)? {
                            return Ok(Value::Bool(false));
                        }
                    }
                    Ok(Value::Bool(true))
                }
            }
        }
        "set.filter" | "set.map" | "set.any" | "set.all" => {
            arity(name, &args, 2)?;
            let s = want_set(name, &args[0])?.clone();
            let c = want_fn(name, &args[1])?.clone();
            let fal = it.cb_fallible(&c);
            let mut bail = None;
            let mut out: Vec<Value> = Vec::new();
            let mut acc_bool = name == "set.all";
            for x in s.iter() {
                let r = match call_cb(it, &c, fal, vec![x.clone()])? {
                    Cb::Val(v) => v,
                    Cb::Bail(e) => {
                        bail = Some(e);
                        break;
                    }
                };
                match name {
                    "set.filter" => {
                        if want_bool(name, &r)? {
                            out.push(x.clone());
                        }
                    }
                    "set.map" => {
                        let mut dup = false;
                        for y in &out {
                            if eq_strict(name, &r, y)? {
                                dup = true;
                                break;
                            }
                        }
                        if !dup {
                            out.push(r);
                        }
                    }
                    "set.any" => {
                        if want_bool(name, &r)? {
                            acc_bool = true;
                            break;
                        }
                    }
                    _ => {
                        if !want_bool(name, &r)? {
                            acc_bool = false;
                            break;
                        }
                    }
                }
            }
            let v = match name {
                "set.filter" | "set.map" => Value::Set(Rc::new(out)),
                _ => Value::Bool(acc_bool),
            };
            Ok(hof_out(fal, bail, v))
        }
        "set.fold" => {
            arity(name, &args, 3)?;
            let s = want_set(name, &args[0])?.clone();
            let c = want_fn(name, &args[2])?.clone();
            let fal = it.cb_fallible(&c);
            let mut acc = args[1].clone();
            let mut bail = None;
            for x in s.iter() {
                match call_cb(it, &c, fal, vec![acc.clone(), x.clone()])? {
                    Cb::Val(nv) => acc = nv,
                    Cb::Bail(e) => {
                        bail = Some(e);
                        break;
                    }
                }
            }
            Ok(hof_out(fal, bail, acc))
        }

        "fan.race" => it.fan_race_values(args),
        // ── ALS-DT1: time constructors (compute.* deterministic clock,
        // duration.* wall clock) — negative aborts, overflow saturates ──
        "compute.ns" | "compute.us" | "compute.ms" | "compute.s" | "compute.min" | "compute.h"
        | "duration.ns" | "duration.us" | "duration.ms" | "duration.s" | "duration.min"
        | "duration.h" => {
            arity(name, &args, 1)?;
            let n = want_int(name, &args[0])?;
            if n < 0 {
                return Err(Flow::Abort(format!("negative time: {name}({n})")));
            }
            let per: i64 = if name.ends_with(".ns") {
                1
            } else if name.ends_with(".us") {
                1_000
            } else if name.ends_with(".ms") {
                1_000_000
            } else if name.ends_with(".s") {
                1_000_000_000
            } else if name.ends_with(".min") {
                60_000_000_000
            } else {
                3_600_000_000_000
            };
            Ok(Value::Time {
                wall: name.starts_with("duration."),
                ns: n.saturating_mul(per),
            })
        }
        other => match crate::stdlib_ext::call_ext(it, other, args.clone())
            .or_else(|| crate::stdlib_ext2::call_ext2(it, other, args.clone()))
            .or_else(|| crate::stdlib_sized::call_sized(it, other, args.clone()))
            .or_else(|| crate::stdlib_matrix::call_matrix(it, other, args))
        {
            Some(r) => r,
            None => Err(Flow::Abstain {
                class: format!("stdlib:{other}"),
                reason: format!(
                    "stdlib function `{other}` is not implemented by the reference evaluator yet"
                ),
            }),
        },
    }
}

/// ALS-T15 + ALS-T23: NaN ignored (one NaN → the other; both → NaN);
/// ±0 ordered as -0 < +0 (IEEE 754-2019 minimum/maximum), commutative.
fn ieee_min_max(a: f64, b: f64, want_max: bool) -> f64 {
    if a.is_nan() {
        return b;
    }
    if b.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 {
        let neg = if want_max {
            a.is_sign_negative() && b.is_sign_negative()
        } else {
            a.is_sign_negative() || b.is_sign_negative()
        };
        return if neg { -0.0 } else { 0.0 };
    }
    if want_max {
        if a > b {
            a
        } else {
            b
        }
    } else if a < b {
        a
    } else {
        b
    }
}

/// ALS-T8: Rust i64 FromStr grammar and its exact error strings.
fn parse_i64(s: &str) -> Value {
    // string_whitespace pins it: int.parse trims the T1 UNICODE whitespace
    // set before the Rust FromStr grammar (T8 error strings unchanged)
    let cs_all: Vec<char> = s.chars().collect();
    let mut lo = 0;
    let mut hi = cs_all.len();
    while lo < hi && is_als_whitespace(cs_all[lo]) {
        lo += 1;
    }
    while hi > lo && is_als_whitespace(cs_all[hi - 1]) {
        hi -= 1;
    }
    let cs: Vec<char> = cs_all[lo..hi].to_vec();
    if cs.is_empty() {
        return Value::Err(Rc::new(Value::str(
            "cannot parse integer from empty string",
        )));
    }
    let (neg, digits) = match cs[0] {
        '+' => (false, &cs[1..]),
        '-' => (true, &cs[1..]),
        _ => (false, &cs[..]),
    };
    if digits.is_empty() {
        return Value::Err(Rc::new(Value::str("invalid digit found in string")));
    }
    let mut acc: i64 = 0;
    for &c in digits {
        let d = match c.to_digit(10) {
            Some(d) => d as i64,
            None => return Value::Err(Rc::new(Value::str("invalid digit found in string"))),
        };
        acc = match acc.checked_mul(10).and_then(|a| {
            if neg {
                a.checked_sub(d)
            } else {
                a.checked_add(d)
            }
        }) {
            Some(a) => a,
            None => {
                return Value::Err(Rc::new(Value::str(if neg {
                    "number too small to fit in target type"
                } else {
                    "number too large to fit in target type"
                })))
            }
        };
    }
    Value::Ok(Rc::new(Value::Int(acc)))
}

/// ALS-T2 grammar + error strings; exact rounding on the fast path, abstain
/// beyond it (the full big-rational correctly rounded path is future work).
fn parse_f64(_it: &mut Interp, s: &str) -> Result<Value, Flow> {
    Ok(parse_f64_pure(s))
}

fn parse_f64_pure(s: &str) -> Value {
    // ws* sign? (number | inf | infinity | nan) ws* — ws per ALS-T1's set
    let cs_all: Vec<char> = s.chars().collect();
    let mut lo = 0;
    let mut hi = cs_all.len();
    while lo < hi && is_als_whitespace(cs_all[lo]) {
        lo += 1;
    }
    while hi > lo && is_als_whitespace(cs_all[hi - 1]) {
        hi -= 1;
    }
    if lo >= hi {
        return Value::Err(Rc::new(Value::str("cannot parse float from empty string")));
    }
    let body: Vec<char> = cs_all[lo..hi].to_vec();
    let (neg, body) = match body[0] {
        '-' => (true, body[1..].to_vec()),
        '+' => (false, body[1..].to_vec()),
        _ => (false, body),
    };
    let lower: String = body.iter().flat_map(|c| c.to_lowercase()).collect();
    if lower == "inf" || lower == "infinity" {
        return Value::Ok(Rc::new(Value::Float(F64(if neg {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        }))));
    }
    if lower == "nan" {
        return Value::Ok(Rc::new(Value::Float(F64(f64::NAN))));
    }
    let mut mant = String::new();
    let mut frac_len: i64 = 0;
    let mut exp: i64 = 0;
    let cs = body;
    let mut i = 0;
    let mut int_digits = 0;
    while i < cs.len() && cs[i].is_ascii_digit() {
        mant.push(cs[i]);
        int_digits += 1;
        i += 1;
    }
    if i < cs.len() && cs[i] == '.' {
        i += 1;
        while i < cs.len() && cs[i].is_ascii_digit() {
            mant.push(cs[i]);
            frac_len += 1;
            i += 1;
        }
    }
    if int_digits == 0 && frac_len == 0 {
        return Value::Err(Rc::new(Value::str("invalid float literal")));
    }
    if i < cs.len() && (cs[i] == 'e' || cs[i] == 'E') {
        i += 1;
        let mut eneg = false;
        if i < cs.len() && (cs[i] == '+' || cs[i] == '-') {
            eneg = cs[i] == '-';
            i += 1;
        }
        let mut ed = 0i64;
        let mut any = false;
        while i < cs.len() && cs[i].is_ascii_digit() {
            ed = ed
                .saturating_mul(10)
                .saturating_add((cs[i] as u8 - b'0') as i64);
            any = true;
            i += 1;
        }
        if !any {
            return Value::Err(Rc::new(Value::str("invalid float literal")));
        }
        exp = if eneg { -ed } else { ed };
    }
    if i != cs.len() {
        return Value::Err(Rc::new(Value::str("invalid float literal")));
    }
    let scale = exp - frac_len;
    let v = crate::fmtfloat::parse_decimal(&mant, scale);
    Value::Ok(Rc::new(Value::Float(F64(if neg { -v } else { v }))))
}

/// int.parse (ALS-T8) as a plain Result — for the normative json parser,
/// which passes the oracle's error text through
pub fn int_parse_t8(s: &str) -> Result<i64, String> {
    match parse_i64(s) {
        Value::Ok(v) => match &*v {
            Value::Int(n) => Ok(*n),
            _ => Err("int.parse: non-int payload".into()),
        },
        Value::Err(e) => match &*e {
            Value::Str(m) => Err(m.to_string()),
            _ => Err("int.parse failed".into()),
        },
        _ => Err("int.parse failed".into()),
    }
}

/// float.parse (ALS-T2) as a plain Result — same passthrough discipline
pub fn float_parse_t2(s: &str) -> Result<f64, String> {
    match parse_f64_pure(s) {
        Value::Ok(v) => match &*v {
            Value::Float(f) => Ok(f.0),
            _ => Err("float.parse: non-float payload".into()),
        },
        Value::Err(e) => match &*e {
            Value::Str(m) => Err(m.to_string()),
            _ => Err("float.parse failed".into()),
        },
        _ => Err("float.parse failed".into()),
    }
}
