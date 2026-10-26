//! Round-3 stdlib module: the sized-numeric conversion and formatting
//! families (C-038 literals, C-179 UInt64 upper half, C-180 width wrap,
//! C-182 Float32, C-190 captures, C-195 checked-conversion matrix, ALS-T24
//! checked exactness). Unchecked `to_*` conversions are exercised in-range
//! only by the corpus; an out-of-range unchecked conversion abstains rather
//! than guessing between wrap, saturate and abort.

use std::rc::Rc;

use crate::eval::{sized_unsigned, Flow, Interp};
use crate::fmtfloat;
use crate::value::{fmt_u64, Value, F64};

pub const SIZED_FNS: &[&str] = &[
    "int8.to_string",
    "int16.to_string",
    "int32.to_string",
    "int64.to_string",
    "uint8.to_string",
    "uint16.to_string",
    "uint32.to_string",
    "uint64.to_string",
    "float32.to_string",
    "int.max_value",
    "int.min_value",
    "int8.max_value",
    "int8.min_value",
    "int16.max_value",
    "int16.min_value",
    "int32.max_value",
    "int32.min_value",
    "int64.max_value",
    "int64.min_value",
    "uint8.max_value",
    "uint8.min_value",
    "uint16.max_value",
    "uint16.min_value",
    "uint32.max_value",
    "uint32.min_value",
    "uint64.max_value",
    "uint64.min_value",
    "int8.parse",
    "int16.parse",
    "int32.parse",
    "uint8.parse",
    "uint16.parse",
    "uint32.parse",
    "uint64.parse",
    // the `from` family (R-L0-40): a target type from any numeric source
    "int.from",
    "int.from_checked",
    "int.from_saturating",
    "int.from_wrapping",
    "int8.from",
    "int8.from_checked",
    "int8.from_saturating",
    "int8.from_wrapping",
    "int16.from",
    "int16.from_checked",
    "int16.from_saturating",
    "int16.from_wrapping",
    "int32.from",
    "int32.from_checked",
    "int32.from_saturating",
    "int32.from_wrapping",
    "uint8.from",
    "uint8.from_checked",
    "uint8.from_saturating",
    "uint8.from_wrapping",
    "uint16.from",
    "uint16.from_checked",
    "uint16.from_saturating",
    "uint16.from_wrapping",
    "uint32.from",
    "uint32.from_checked",
    "uint32.from_saturating",
    "uint32.from_wrapping",
    "uint64.from",
    "uint64.from_checked",
    "uint64.from_saturating",
    "uint64.from_wrapping",
    "int64.from",
    "int64.from_checked",
    "int64.from_saturating",
    "int64.from_wrapping",
    "float.from",
    "float64.from",
    "float32.from",
    "float32.from_checked",
    "float32.from_bits",
];

fn arity(name: &str, args: &[Value], n: usize) -> Result<(), Flow> {
    if args.len() == n {
        Ok(())
    } else {
        Err(Flow::Fatal(format!(
            "{name} takes {n} argument(s), got {}",
            args.len()
        )))
    }
}

fn want_int(name: &str, v: &Value) -> Result<i64, Flow> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(Flow::Fatal(format!(
            "{name}: expected Int, got {}",
            other.type_name()
        ))),
    }
}

fn want_sized(name: &str, v: &Value, bits: u8, signed: bool) -> Result<i64, Flow> {
    match v {
        Value::Sized {
            bits: b,
            signed: s,
            v,
        } if *b == bits && *s == signed => Ok(*v),
        other => Err(Flow::Fatal(format!(
            "{name}: expected a {}-bit {} value, got {}",
            bits,
            if signed { "signed" } else { "unsigned" },
            other.type_name()
        ))),
    }
}

fn sized(bits: u8, signed: bool, v: i64) -> Value {
    Value::Sized { bits, signed, v }
}

fn some(v: Value) -> Value {
    Value::Some(Rc::new(v))
}

pub fn call_sized(it: &mut Interp, name: &str, args: Vec<Value>) -> Option<Result<Value, Flow>> {
    if !SIZED_FNS.contains(&name) {
        return None;
    }
    Some(dispatch(it, name, args))
}

fn dispatch(it: &mut Interp, name: &str, args: Vec<Value>) -> Result<Value, Flow> {
    // the `from` family (stdlib/<T>.almd, R-L0-40)
    if let Some(r) = from_family(it, name, &args) {
        return r;
    }
    // <sized>.to_string
    for (module, bits, signed) in SIZED_MODULES {
        if name == format!("{module}.to_string") {
            arity(name, &args, 1)?;
            let v = want_sized(name, &args[0], *bits, *signed)?;
            return Ok(if *signed {
                Value::str(&crate::value::fmt_int(v))
            } else {
                Value::str(&fmt_u64(sized_unsigned(*bits, v)))
            });
        }
    }
    // <sized>.parse (C-413): int.parse's grammar and ASCII-only trim, then the
    // width's range; an unsigned width refuses a leading '-' first
    for (module, bits, signed) in SIZED_MODULES {
        if name == format!("{module}.parse") {
            arity(name, &args, 1)?;
            let s = match &args[0] {
                Value::Str(s) => s.to_string(),
                other => {
                    return Err(Flow::Fatal(format!(
                        "{name}: expected String, got {other:?}"
                    )))
                }
            };
            return Ok(match parse_sized_text(&s, *bits, *signed) {
                Ok(v) => Value::Ok(Rc::new(sized(*bits, *signed, v))),
                Err(e) => Value::Err(Rc::new(Value::str(&e))),
            });
        }
    }
    // <sized>.min_value / max_value — the carrier's own declared bounds
    for (module, bits, signed) in SIZED_MODULES {
        for (suffix, want_max) in [("min_value", false), ("max_value", true)] {
            if name == format!("{module}.{suffix}") {
                arity(name, &args, 0)?;
                let v: i64 = if *signed {
                    if want_max {
                        (1i64 << (*bits as u32 - 1)) - 1
                    } else {
                        -(1i64 << (*bits as u32 - 1))
                    }
                } else if want_max {
                    (u64::MAX >> (64 - *bits as u32)) as i64
                } else {
                    0
                };
                return Ok(sized(*bits, *signed, v));
            }
        }
    }
    match name {
        "int.max_value" | "int.min_value" | "int64.max_value" | "int64.min_value" => {
            arity(name, &args, 0)?;
            let v = if name.ends_with("max_value") {
                i64::MAX
            } else {
                i64::MIN
            };
            Ok(if name.starts_with("int.") {
                Value::Int(v)
            } else {
                sized(64, true, v)
            })
        }
        "float32.to_string" => {
            arity(name, &args, 1)?;
            match &args[0] {
                // C-372: the binary32's own shortest digits (0.1f32 -> "0.1"),
                // `.0` kept on an integral value like float.to_string
                Value::Float32(g) => Ok(Value::str(&fmtfloat::to_string_form_f32(*g))),
                other => Err(Flow::Fatal(format!(
                    "{name}: expected Float32, got {}",
                    other.type_name()
                ))),
            }
        }
        _ => it.abstain_pub(
            &format!("stdlib:{name}"),
            format!("`{name}` is registered but unrouted in the sized module"),
        ),
    }
}

const SIZED_MODULES: &[(&str, u8, bool)] = &[
    ("int8", 8, true),
    ("int16", 16, true),
    ("int32", 32, true),
    ("uint8", 8, false),
    ("uint16", 16, false),
    ("uint32", 32, false),
    ("uint64", 64, false),
];

/// `<sized>.parse`: the stdlib bodies' rule, over int.parse (ALS-T8, C-413).
/// Int8/16/32 range-check its answer; UInt8/16/32 first refuse a '-' after
/// the ALS-T1 trim, then check the top; UInt64 trims the ASCII set itself,
/// refuses '-', and reads 2^63..2^64-1 as head * 10 + last digit.
fn parse_sized_text(s: &str, bits: u8, signed: bool) -> Result<i64, String> {
    let int_parse = crate::stdlib::int_parse_t8;
    const NEG: &str = "invalid digit found in string";
    const BIG: &str = "number too large to fit in target type";
    const SMALL: &str = "number too small to fit in target type";
    if signed {
        let n = int_parse(s)?;
        let min = -(1i64 << (bits as u32 - 1));
        let max = (1i64 << (bits as u32 - 1)) - 1;
        return if n < min {
            Err(SMALL.into())
        } else if n > max {
            Err(BIG.into())
        } else {
            Ok(n)
        };
    }
    if bits < 64 {
        if trim_by(s, crate::stdlib::is_als_whitespace).starts_with('-') {
            return Err(NEG.into());
        }
        let n = int_parse(s)?;
        let max = (u64::MAX >> (64 - bits as u32)) as i64;
        return if n > max { Err(BIG.into()) } else { Ok(n) };
    }
    let t = trim_by(s, crate::stdlib::is_parse_trim_ws);
    let t = t.as_str();
    if t.starts_with('-') {
        return Err(NEG.into());
    }
    match int_parse(t) {
        Ok(n) => Ok(n),
        Err(e) => {
            let cs: Vec<char> = t.chars().collect();
            let n = cs.len();
            let (head, last) = if n == 0 {
                (String::new(), String::new())
            } else {
                (cs[..n - 1].iter().collect(), cs[n - 1..].iter().collect())
            };
            match (int_parse(&head), int_parse(&last)) {
                (Ok(h), Ok(d)) if h < 1844674407370955161 || h == 1844674407370955161 && d <= 5 => {
                    Ok((h as u64).wrapping_mul(10).wrapping_add(d as u64) as i64)
                }
                _ => Err(e),
            }
        }
    }
}

/// Both ends of `s` with every char `ws` holds dropped.
fn trim_by(s: &str, ws: fn(char) -> bool) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut lo = 0;
    let mut hi = cs.len();
    while lo < hi && ws(cs[lo]) {
        lo += 1;
    }
    while hi > lo && ws(cs[hi - 1]) {
        hi -= 1;
    }
    cs[lo..hi].iter().collect()
}

// ── the `from` family (R-L0-40, stdlib/<T>.almd) ─────────────────────────
// Every body reads its source through the Numeric pivots (`__int_exact`,
// `__uint64_exact`, `__trunc`, `__int_sat`, `__uint64_sat`, `__bits64`,
// `__to_float`, `__to_float32`, ...). For an integer target those pivots
// compose to one rule over the source's exact value: `from` truncates and
// must land in range (else a panic), `from_checked` must be exact and in
// range (else none), `from_saturating` truncates and clamps (NaN is 0), and
// `from_wrapping` keeps the low bits of an integer source.

/// A numeric source as the pivots see it.
enum Src {
    /// Int, Int64 and the narrower sized integers: the exact value
    I(i64),
    /// UInt64: the unsigned value (its upper half exceeds Int)
    U(u64),
    /// Float (and Float32 widened, which is exact)
    F(f64),
    /// Float32, kept for the targets whose pivot is the identity on it
    F32(f32),
}

fn src_of(v: &Value) -> Option<Src> {
    Some(match v {
        Value::Int(n) => Src::I(*n),
        Value::Sized {
            bits: 64,
            signed: false,
            v,
        } => Src::U(*v as u64),
        Value::Sized { bits, signed, v } => {
            if *signed {
                Src::I(*v)
            } else {
                Src::I(sized_unsigned(*bits, *v) as i64)
            }
        }
        Value::Float(F64(f)) => Src::F(*f),
        Value::Float32(g) => Src::F32(*g),
        _ => return None,
    })
}

/// The integer targets: (bits, signed); Int is the signed 64-bit register.
fn int_target(module: &str) -> Option<(u8, bool)> {
    Some(match module {
        // Int64 rides the Int carrier
        "int" | "int64" => (64, true),
        "int8" => (8, true),
        "int16" => (16, true),
        "int32" => (32, true),
        "uint8" => (8, false),
        "uint16" => (16, false),
        "uint32" => (32, false),
        "uint64" => (64, false),
        _ => return None,
    })
}

fn target_range(bits: u8, signed: bool) -> (i128, i128) {
    if signed {
        (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
    } else {
        (0, (1i128 << bits) - 1)
    }
}

/// The value a target of (bits, signed) holds for the in-range integer n.
fn int_value(bits: u8, signed: bool, n: i128) -> Value {
    if bits == 64 && signed {
        Value::Int(n as i64)
    } else {
        sized(bits, signed, n as i64)
    }
}

/// `__int_exact` / `__uint64_exact` before the range test: the source's value
/// when it is an integer, a float only when finite and integral (the float
/// pivots bound it to -2^63..2^64, which every target range lies inside).
fn exact_of(src: &Src) -> Option<i128> {
    match src {
        Src::I(n) => Some(*n as i128),
        Src::U(u) => Some(*u as i128),
        Src::F(f) => float_exact(*f),
        Src::F32(g) => float_exact(*g as f64),
    }
}

fn float_exact(f: f64) -> Option<i128> {
    if f.is_finite()
        && f.trunc() == f
        && (-9223372036854775808.0..18446744073709551616.0).contains(&f)
    {
        Some(f as i128)
    } else {
        None
    }
}

fn trunc_of(src: Src) -> Src {
    match src {
        Src::F(f) => Src::F(f.trunc()),
        Src::F32(g) => Src::F(g.trunc() as f64),
        other => other,
    }
}

/// UInt64 → Float as stdlib/uint64_convert.almd computes it: the lower half
/// converts directly; the upper half as (x/2 as Float) * 2 + x%2.
fn u64_to_f64(u: u64) -> f64 {
    if (u as i64) >= 0 {
        (u as i64) as f64
    } else {
        let hf = ((u / 2) as i64) as f64;
        let lf = ((u % 2) as i64) as f64;
        hf + hf + lf
    }
}

/// UInt64 → Float32 (uint64_to_float32): direct on the lower half, the
/// upper half's Float sum rounded once more.
fn u64_to_f32(u: u64) -> f32 {
    if (u as i64) >= 0 {
        (u as i64) as f32
    } else {
        u64_to_f64(u) as f32
    }
}

fn from_family(it: &Interp, name: &str, args: &[Value]) -> Option<Result<Value, Flow>> {
    let (module, verb) = name.split_once('.')?;
    if !matches!(
        verb,
        "from" | "from_checked" | "from_saturating" | "from_wrapping" | "from_bits"
    ) {
        return None;
    }
    Some(from_dispatch(it, name, module, verb, args))
}

fn from_dispatch(
    it: &Interp,
    name: &str,
    module: &str,
    verb: &str,
    args: &[Value],
) -> Result<Value, Flow> {
    arity(name, args, 1)?;
    if (module, verb) == ("float32", "from_bits") {
        // int.__bits_to_f32: the low 32 bits as a binary32
        let n = want_int(name, &args[0])?;
        return Ok(Value::Float32(f32::from_bits(n as u32)));
    }
    let src = match src_of(&args[0]) {
        Some(s) => s,
        None => {
            return Err(Flow::Fatal(format!(
                "{name}: expected a number, got {}",
                args[0].type_name()
            )))
        }
    };
    let panic = |what: &str| -> Result<Value, Flow> {
        it.abstain_pub(
            "stdlib:panic",
            format!("{name}: the value is not a number in the range of {what} — a panic (C-219), not rendered yet"),
        )
    };
    match (module, verb) {
        // Float64 rides the Float carrier
        ("float" | "float64", "from") => Ok(Value::Float(F64(match src {
            Src::I(n) => n as f64,
            Src::U(u) => u64_to_f64(u),
            Src::F(f) => f,
            Src::F32(g) => g as f64,
        }))),
        ("float32", "from") => Ok(Value::Float32(match src {
            Src::I(n) => n as f32,
            Src::U(u) => u64_to_f32(u),
            Src::F(f) => f as f32,
            Src::F32(g) => g,
        })),
        ("float32", "from_checked") => {
            let got = match src {
                // int.__to_float32_checked: the f32 widened back must not reach
                // 2^63 and must read back as n
                Src::I(n) => {
                    let g = n as f32;
                    let back = g as f64;
                    (back < 9223372036854775808.0 && back as i64 == n).then_some(g)
                }
                Src::U(u) => {
                    let g = u64_to_f32(u);
                    (float_exact(g as f64) == Some(u as i128)).then_some(g)
                }
                // float.__to_float32_checked: NaN and the infinities are none
                Src::F(f) => {
                    let g = f as f32;
                    (f.is_finite() && g as f64 == f).then_some(g)
                }
                Src::F32(g) => (!g.is_nan()).then_some(g),
            };
            Ok(match got {
                Some(g) => some(Value::Float32(g)),
                None => Value::None,
            })
        }
        _ => {
            let Some((bits, signed)) = int_target(module) else {
                return it.abstain_pub(
                    &format!("stdlib:{name}"),
                    format!("`{name}`: no integer target `{module}` in the sized module"),
                );
            };
            let (lo, hi) = target_range(bits, signed);
            let tname = match (bits, signed) {
                (64, true) => "Int",
                (8, true) => "Int8",
                (16, true) => "Int16",
                (32, true) => "Int32",
                (8, false) => "UInt8",
                (16, false) => "UInt16",
                (32, false) => "UInt32",
                _ => "UInt64",
            };
            match verb {
                "from" => match exact_of(&trunc_of(src)) {
                    Some(n) if (lo..=hi).contains(&n) => Ok(int_value(bits, signed, n)),
                    _ => panic(tname),
                },
                "from_checked" => Ok(match exact_of(&src) {
                    Some(n) if (lo..=hi).contains(&n) => some(int_value(bits, signed, n)),
                    _ => Value::None,
                }),
                "from_saturating" => {
                    let n: i128 = match trunc_of(src) {
                        Src::I(n) => n as i128,
                        Src::U(u) => u as i128,
                        Src::F(f) if f.is_nan() => 0,
                        Src::F(f) if f < lo as f64 => lo,
                        Src::F(f) if f > hi as f64 => hi,
                        Src::F(f) => f as i128,
                        Src::F32(g) => g as i128, // unreachable: trunc widened it
                    };
                    Ok(int_value(bits, signed, n.clamp(lo, hi)))
                }
                "from_wrapping" => {
                    // `[S: Integer]`: the low bits of the 64-bit pattern
                    let b64: u64 = match src {
                        Src::I(n) => n as u64,
                        Src::U(u) => u,
                        _ => {
                            return Err(Flow::Fatal(format!(
                                "{name}: from_wrapping takes an integer, got {}",
                                args[0].type_name()
                            )))
                        }
                    };
                    let v: i64 = match (bits, signed) {
                        (64, _) => b64 as i64,
                        (8, true) => b64 as u8 as i8 as i64,
                        (16, true) => b64 as u16 as i16 as i64,
                        (32, true) => b64 as u32 as i32 as i64,
                        (8, false) => b64 as u8 as i64,
                        (16, false) => b64 as u16 as i64,
                        _ => b64 as u32 as i64,
                    };
                    Ok(if bits == 64 && signed {
                        Value::Int(v)
                    } else {
                        sized(bits, signed, v)
                    })
                }
                _ => Err(Flow::Fatal("from family: unreachable verb".into())),
            }
        }
    }
}
