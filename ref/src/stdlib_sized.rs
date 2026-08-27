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
    "int.to_int8",
    "int.to_int16",
    "int.to_int32",
    "int.to_int64",
    "int.to_uint8",
    "int.to_uint16",
    "int.to_uint32",
    "int.to_uint64",
    "int.from_int8",
    "int.from_int16",
    "int.from_int32",
    "int.from_int64",
    "int.from_uint8",
    "int.from_uint16",
    "int.from_uint32",
    "int.bits_to_float",
    "int8.to_string",
    "int16.to_string",
    "int32.to_string",
    "int64.to_string",
    "uint8.to_string",
    "uint16.to_string",
    "uint32.to_string",
    "uint64.to_string",
    "int8.to_int64",
    "int16.to_int64",
    "int32.to_int64",
    "uint8.to_int64",
    "uint16.to_int64",
    "uint32.to_int64",
    "int64.to_int8_checked",
    "uint8.to_int8_checked",
    "int.from_uint64_checked",
    "int.to_uint8_checked",
    "int.from_int16",
    "uint64.to_float64",
    "float.from_float64",
    "float.to_int8_checked",
    "float.to_int16_checked",
    "float.to_int32_checked",
    "float.to_uint8_checked",
    "float.to_uint16_checked",
    "float.to_uint32_checked",
    "float.to_uint64_checked",
    "float.to_float32_checked",
    "float32.to_string",
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

fn want_float(name: &str, v: &Value) -> Result<f64, Flow> {
    match v {
        Value::Float(F64(f)) => Ok(*f),
        other => Err(Flow::Fatal(format!(
            "{name}: expected Float, got {}",
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

fn in_range(bits: u8, signed: bool, n: i64) -> bool {
    if signed {
        let min = -(1i64 << (bits as u32 - 1));
        let max = (1i64 << (bits as u32 - 1)) - 1;
        n >= min && n <= max
    } else if bits == 64 {
        n >= 0 // an Int source can only reach the lower half
    } else {
        n >= 0 && (n as u64) <= (u64::MAX >> (64 - bits as u32))
    }
}

fn checked_from_float(f: f64, bits: u8, signed: bool) -> Option<i64> {
    // ALS-T24: some() only when the value is EXACTLY representable —
    // fractional, out-of-range, NaN and ±inf are none; -0.0 is some(0)
    if !f.is_finite() || f.trunc() != f {
        return None;
    }
    if signed {
        let min = -((1i128) << (bits - 1));
        let max = ((1i128) << (bits - 1)) - 1;
        let as_i = f as i128;
        // reject values whose f64 form lies outside (2^63-1 rounds up)
        if f >= (max as f64) + 1.0 || f < min as f64 {
            return None;
        }
        if as_i < min || as_i > max {
            return None;
        }
        Some(as_i as i64)
    } else {
        let max: u128 = if bits == 64 {
            u64::MAX as u128
        } else {
            (u64::MAX >> (64 - bits as u32)) as u128
        };
        if f < 0.0 || f >= (max as f64) + 1.0 {
            return None;
        }
        let as_u = f as u128;
        if as_u > max {
            return None;
        }
        Some(as_u as u64 as i64)
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
    // int.to_<sized>: unchecked — in-range only in the corpus
    if let Some(rest) = name.strip_prefix("int.to_") {
        if let Some((bits, signed)) = parse_sized(rest) {
            arity(name, &args, 1)?;
            let n = want_int(name, &args[0])?;
            if !in_range(bits, signed, n) {
                return it.abstain_pub(
                    &format!("stdlib:{name}"),
                    format!("{name}({n}) out of range — the unchecked edge is not pinned"),
                );
            }
            return Ok(sized(bits, signed, n));
        }
    }
    // int.from_<sized>: the value back as Int
    if let Some(rest) = name.strip_prefix("int.from_") {
        if let Some((bits, signed)) = parse_sized(rest) {
            arity(name, &args, 1)?;
            let v = want_sized(name, &args[0], bits, signed)?;
            let n = if signed {
                v
            } else {
                sized_unsigned(bits, v) as i64
            };
            return Ok(Value::Int(n));
        }
    }
    // <sized>.to_string / <sized>.to_int64
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
        if name == format!("{module}.to_int64") {
            arity(name, &args, 1)?;
            let v = want_sized(name, &args[0], *bits, *signed)?;
            let n = if *signed {
                v
            } else {
                sized_unsigned(*bits, v) as i64
            };
            return Ok(Value::Int(n));
        }
    }
    match name {
        "int.to_int64" | "float.from_float64" => {
            // Int64 rides the Int carrier; Float64 rides Float — identities
            arity(name, &args, 1)?;
            Ok(args.into_iter().next().unwrap())
        }
        "int.from_int64" => {
            arity(name, &args, 1)?;
            Ok(args.into_iter().next().unwrap())
        }
        "int.bits_to_float" => {
            arity(name, &args, 1)?;
            let n = want_int(name, &args[0])?;
            // C-210: the raw pattern loads; observation boundaries canonicalize
            Ok(Value::Float(F64(f64::from_bits(n as u64))))
        }
        "int64.to_int8_checked" => {
            arity(name, &args, 1)?;
            let n = want_int(name, &args[0])?;
            Ok(if in_range(8, true, n) {
                some(sized(8, true, n))
            } else {
                Value::None
            })
        }
        "uint8.to_int8_checked" => {
            arity(name, &args, 1)?;
            let v = sized_unsigned(8, want_sized(name, &args[0], 8, false)?);
            Ok(if v <= 127 {
                some(sized(8, true, v as i64))
            } else {
                Value::None
            })
        }
        "int.from_uint64_checked" => {
            arity(name, &args, 1)?;
            let v = want_sized(name, &args[0], 64, false)?;
            Ok(if v >= 0 {
                some(Value::Int(v))
            } else {
                Value::None
            })
        }
        "int.to_uint8_checked" => {
            arity(name, &args, 1)?;
            let n = want_int(name, &args[0])?;
            Ok(if in_range(8, false, n) {
                some(sized(8, false, n))
            } else {
                Value::None
            })
        }
        "uint64.to_float64" => {
            arity(name, &args, 1)?;
            let v = want_sized(name, &args[0], 64, false)?;
            Ok(Value::Float(F64(v as u64 as f64)))
        }
        "float.to_float32_checked" => {
            arity(name, &args, 1)?;
            let f = want_float(name, &args[0])?;
            // exactly representable in binary32 (T24); NaN/±inf are none
            let g = f as f32;
            Ok(if f.is_finite() && (g as f64) == f {
                some(Value::Float32(g))
            } else {
                Value::None
            })
        }
        "float32.to_string" => {
            arity(name, &args, 1)?;
            match &args[0] {
                // measured on 0.59.1: the spelling is the f64 shortest form
                // of the WIDENED value (0.1f32 prints 0.10000000149011612)
                Value::Float32(g) => Ok(Value::str(&fmtfloat::to_string_form(F64(*g as f64)))),
                other => Err(Flow::Fatal(format!(
                    "{name}: expected Float32, got {}",
                    other.type_name()
                ))),
            }
        }
        _ => {
            // float.to_<sized>_checked
            if let Some(rest) = name.strip_prefix("float.to_") {
                if let Some(base) = rest.strip_suffix("_checked") {
                    if let Some((bits, signed)) = parse_sized(base) {
                        arity(name, &args, 1)?;
                        let f = want_float(name, &args[0])?;
                        return Ok(match checked_from_float(f, bits, signed) {
                            Some(v) => some(sized(bits, signed, v)),
                            None => Value::None,
                        });
                    }
                }
            }
            it.abstain_pub(
                &format!("stdlib:{name}"),
                format!("`{name}` is registered but unrouted in the sized module"),
            )
        }
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

fn parse_sized(s: &str) -> Option<(u8, bool)> {
    Some(match s {
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
