//! The `bigint` module as the judge reads it (C-419, almide/almide#4196):
//! arbitrary-precision integers, a sign and a magnitude.
//!
//! A BigInt is observed as the canonical record `BigInt { neg, limbs }` —
//! `limbs` a little-endian list of 30-bit limbs with no zero limb on top, so
//! zero is the empty list and is never negative: one value has one
//! representation, and structural `==` compares values.
//!
//! The text limits: in a radix that is not a power of two, `parse_radix`
//! answers `err("integer string conversion exceeds the limit of 4300 digits")`
//! for more than 4300 digits (leading zeros count, the sign does not), and
//! `to_string_radix` aborts with the same text when its text would pass 4300
//! digits. A power-of-two radix has no limit. `pow` aborts with
//! `pow result too large` when `(bit_length(base) - 1) * exp` reaches
//! 30 * 2^28. Those aborts take bigint's `panic` form (C-219 `PANIC: <msg>`),
//! which this evaluator does not render yet: they abstain as `stdlib:panic`.
//!
//! The arithmetic here is deliberately the plain schoolbook kind (repeated
//! multiply-add to parse, repeated division to print, in every radix) —
//! independent of the implementation's linear power-of-two paths.

use std::rc::Rc;

use crate::eval::{Flow, Interp};
use crate::stdlib::is_als_whitespace;
use crate::value::Value;

pub const BIGINT_FNS: &[&str] = &[
    "bigint.parse",
    "bigint.parse_radix",
    "bigint.to_string",
    "bigint.to_string_radix",
    "bigint.from",
    "bigint.pow",
    "bigint.abs",
    "bigint.sign",
    "bigint.compare",
    "bigint.bit_length",
    "bigint.is_even",
    "bigint.is_odd",
];

const LIMB_BITS: u32 = 30;
const LIMB_BASE: u64 = 1 << LIMB_BITS;
const TEXT_DIGIT_LIMIT: usize = 4300;
const TEXT_LIMIT_ERROR: &str = "integer string conversion exceeds the limit of 4300 digits";
/// 30 * 2^28 bits: 2^28 limbs, the list ceiling `bshl` meets.
const POW_BIT_CEILING: i64 = 30 * (1 << 28);

pub fn call_bigint(_it: &mut Interp, name: &str, args: Vec<Value>) -> Option<Result<Value, Flow>> {
    if !BIGINT_FNS.contains(&name) {
        return None;
    }
    Some(dispatch(name, args))
}

fn abstain<T>(class: &str, reason: impl Into<String>) -> Result<T, Flow> {
    Err(Flow::Abstain {
        class: class.into(),
        reason: reason.into(),
    })
}

/// bigint's own aborts are `panic` (C-219), not yet rendered here.
fn panic_abort<T>(msg: &str) -> Result<T, Flow> {
    abstain(
        "stdlib:panic",
        format!("bigint aborts with panic(\"{msg}\")"),
    )
}

fn arity(name: &str, args: &[Value], n: usize) -> Result<(), Flow> {
    if args.len() != n {
        return abstain(
            &format!("stdlib:{name}"),
            format!(
                "{name}: expected {n} argument(s), got {} — unmodelled overload",
                args.len()
            ),
        );
    }
    Ok(())
}

fn mismatch<T>(name: &str, want: &str, got: &Value) -> Result<T, Flow> {
    abstain(
        "semantics:type-mismatch",
        format!("{name}: expected {want}, got {}", got.type_name()),
    )
}

fn want_int(name: &str, v: &Value) -> Result<i64, Flow> {
    match v {
        Value::Int(n) => Ok(*n),
        other => mismatch(name, "Int", other),
    }
}

fn want_str<'a>(name: &str, v: &'a Value) -> Result<&'a Rc<str>, Flow> {
    match v {
        Value::Str(s) => Ok(s),
        other => mismatch(name, "String", other),
    }
}

/// sign + canonical magnitude (little-endian 30-bit limbs, no zero on top)
#[derive(Clone)]
struct Big {
    neg: bool,
    mag: Vec<u64>,
}

fn trimmed(mut mag: Vec<u64>) -> Vec<u64> {
    while mag.last() == Some(&0) {
        mag.pop();
    }
    mag
}

fn make(neg: bool, mag: Vec<u64>) -> Big {
    let mag = trimmed(mag);
    Big {
        neg: neg && !mag.is_empty(),
        mag,
    }
}

fn to_value(b: Big) -> Value {
    let limbs: Vec<Value> = b.mag.iter().map(|&l| Value::Int(l as i64)).collect();
    Value::Record {
        type_name: Some(Rc::from("BigInt")),
        fields: Rc::new(vec![
            (Rc::from("neg"), Value::Bool(b.neg)),
            (Rc::from("limbs"), Value::List(Rc::new(limbs))),
        ]),
    }
}

fn want_big(name: &str, v: &Value) -> Result<Big, Flow> {
    if let Value::Record { fields, .. } = v {
        let mut neg = None;
        let mut mag = None;
        for (k, fv) in fields.iter() {
            match (&**k, fv) {
                ("neg", Value::Bool(b)) => neg = Some(*b),
                ("limbs", Value::List(xs)) => {
                    let mut out = Vec::with_capacity(xs.len());
                    for x in xs.iter() {
                        match x {
                            Value::Int(n) if *n >= 0 && (*n as u64) < LIMB_BASE => {
                                out.push(*n as u64)
                            }
                            _ => return mismatch(name, "BigInt", v),
                        }
                    }
                    mag = Some(out);
                }
                _ => {}
            }
        }
        if let (Some(neg), Some(mag)) = (neg, mag) {
            return Ok(Big { neg, mag });
        }
    }
    mismatch(name, "BigInt", v)
}

fn from_i64(n: i64) -> Big {
    let mut m = n.unsigned_abs();
    let mut mag = Vec::new();
    while m > 0 {
        mag.push(m % LIMB_BASE);
        m /= LIMB_BASE;
    }
    make(n < 0, mag)
}

/// mag = mag * m + add, for m, add < 2^31
fn mul_add_small(mag: &mut Vec<u64>, m: u64, add: u64) {
    let mut carry = add;
    for l in mag.iter_mut() {
        let p = *l * m + carry;
        *l = p % LIMB_BASE;
        carry = p / LIMB_BASE;
    }
    while carry > 0 {
        mag.push(carry % LIMB_BASE);
        carry /= LIMB_BASE;
    }
}

/// mag / d in place, returning mag % d, for 1 <= d <= 2^30
fn divmod_small(mag: &mut Vec<u64>, d: u64) -> u64 {
    let mut rem = 0u64;
    for l in mag.iter_mut().rev() {
        let cur = rem * LIMB_BASE + *l;
        *l = cur / d;
        rem = cur % d;
    }
    let t = trimmed(std::mem::take(mag));
    *mag = t;
    rem
}

fn mul_mag(a: &[u64], b: &[u64]) -> Vec<u64> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![0u64; a.len() + b.len()];
    for (i, &x) in a.iter().enumerate() {
        let mut carry = 0u64;
        for (j, &y) in b.iter().enumerate() {
            let t = out[i + j] + x * y + carry;
            out[i + j] = t % LIMB_BASE;
            carry = t / LIMB_BASE;
        }
        let mut k = i + b.len();
        while carry > 0 {
            let t = out[k] + carry;
            out[k] = t % LIMB_BASE;
            carry = t / LIMB_BASE;
            k += 1;
        }
    }
    trimmed(out)
}

fn bit_length(mag: &[u64]) -> i64 {
    match mag.last() {
        None => 0,
        Some(&top) => {
            let mut bits = 0i64;
            let mut t = top;
            while t > 0 {
                bits += 1;
                t >>= 1;
            }
            LIMB_BITS as i64 * (mag.len() as i64 - 1) + bits
        }
    }
}

fn cmp_mag(a: &[u64], b: &[u64]) -> i64 {
    if a.len() != b.len() {
        return if a.len() < b.len() { -1 } else { 1 };
    }
    for i in (0..a.len()).rev() {
        if a[i] != b[i] {
            return if a[i] < b[i] { -1 } else { 1 };
        }
    }
    0
}

fn check_radix(radix: i64) -> Result<(), Flow> {
    if (2..=36).contains(&radix) {
        Ok(())
    } else {
        panic_abort(&format!("radix must be in 2..36, got {radix}"))
    }
}

fn is_pow2(radix: i64) -> bool {
    radix & (radix - 1) == 0
}

/// the value of an ASCII digit or letter, any case; 99 for anything else
fn digit_value(c: char) -> u64 {
    match c {
        '0'..='9' => c as u64 - '0' as u64,
        'a'..='z' => c as u64 - 'a' as u64 + 10,
        'A'..='Z' => c as u64 - 'A' as u64 + 10,
        _ => 99,
    }
}

fn prefix_radix(c: char) -> i64 {
    match c {
        'x' | 'X' => 16,
        'o' | 'O' => 8,
        'b' | 'B' => 2,
        _ => 0,
    }
}

fn parse_radix(s: &str, radix: i64) -> Result<Value, Flow> {
    check_radix(radix)?;
    let cs: Vec<char> = s.chars().collect();
    let mut lo = 0;
    let mut hi = cs.len();
    while lo < hi && is_als_whitespace(cs[lo]) {
        lo += 1;
    }
    while hi > lo && is_als_whitespace(cs[hi - 1]) {
        hi -= 1;
    }
    let mut body = &cs[lo..hi];
    if body.is_empty() {
        return Ok(Value::Err(Rc::new(Value::str(
            "cannot parse integer from empty string",
        ))));
    }
    let mut neg = false;
    if body[0] == '-' || body[0] == '+' {
        neg = body[0] == '-';
        body = &body[1..];
    }
    // 0x / 0o / 0b only in its own radix, and only with a digit after it
    if body.len() >= 3 && body[0] == '0' && prefix_radix(body[1]) == radix {
        body = &body[2..];
    }
    let invalid = || {
        Ok(Value::Err(Rc::new(Value::str(
            "invalid digit found in string",
        ))))
    };
    if body.is_empty() || body.iter().any(|&c| digit_value(c) >= radix as u64) {
        return invalid();
    }
    if !is_pow2(radix) && body.len() > TEXT_DIGIT_LIMIT {
        return Ok(Value::Err(Rc::new(Value::str(TEXT_LIMIT_ERROR))));
    }
    let mut mag = Vec::new();
    for &c in body {
        mul_add_small(&mut mag, radix as u64, digit_value(c));
    }
    Ok(Value::Ok(Rc::new(to_value(make(neg, mag)))))
}

fn digit_char(d: u64) -> char {
    if d < 10 {
        (b'0' + d as u8) as char
    } else {
        (b'a' + (d - 10) as u8) as char
    }
}

fn int_bit_length(n: i64) -> i64 {
    let mut bits = 0;
    let mut t = n;
    while t > 0 {
        bits += 1;
        t >>= 1;
    }
    bits
}

fn to_string_radix(b: &Big, radix: i64) -> Result<Value, Flow> {
    check_radix(radix)?;
    let limited = !is_pow2(radix);
    // a magnitude of 2^(limit * bit_length(radix)) or more has more than
    // `limit` digits: refused before converting
    if limited && bit_length(&b.mag) > TEXT_DIGIT_LIMIT as i64 * int_bit_length(radix) {
        return panic_abort(TEXT_LIMIT_ERROR);
    }
    let mut mag = b.mag.clone();
    let mut digits: Vec<char> = Vec::new();
    while !mag.is_empty() {
        let d = divmod_small(&mut mag, radix as u64);
        digits.push(digit_char(d));
    }
    if digits.is_empty() {
        digits.push('0');
    }
    if limited && digits.len() > TEXT_DIGIT_LIMIT {
        return panic_abort(TEXT_LIMIT_ERROR);
    }
    let mut out = String::new();
    if b.neg {
        out.push('-');
    }
    out.extend(digits.iter().rev());
    Ok(Value::str(&out))
}

fn pow(base: &Big, exp: i64) -> Result<Value, Flow> {
    if exp < 0 {
        return panic_abort("negative exponent");
    }
    let k = bit_length(&base.mag) - 1;
    if k > 0 && exp > (POW_BIT_CEILING - 1) / k {
        return panic_abort("pow result too large");
    }
    let mut acc: Vec<u64> = vec![1];
    let mut sq = base.mag.clone();
    let mut e = exp;
    while e > 0 {
        if e % 2 == 1 {
            acc = mul_mag(&acc, &sq);
        }
        e /= 2;
        if e > 0 {
            sq = mul_mag(&sq, &sq);
        }
    }
    Ok(to_value(make(base.neg && exp % 2 == 1, acc)))
}

fn dispatch(name: &str, args: Vec<Value>) -> Result<Value, Flow> {
    match name {
        "bigint.parse" => {
            arity(name, &args, 1)?;
            parse_radix(want_str(name, &args[0])?, 10)
        }
        "bigint.parse_radix" => {
            arity(name, &args, 2)?;
            parse_radix(want_str(name, &args[0])?, want_int(name, &args[1])?)
        }
        "bigint.to_string" => {
            arity(name, &args, 1)?;
            to_string_radix(&want_big(name, &args[0])?, 10)
        }
        "bigint.to_string_radix" => {
            arity(name, &args, 2)?;
            to_string_radix(&want_big(name, &args[0])?, want_int(name, &args[1])?)
        }
        "bigint.from" => {
            arity(name, &args, 1)?;
            match &args[0] {
                Value::Int(n) => Ok(to_value(from_i64(*n))),
                other => abstain(
                    "stdlib:bigint.from",
                    format!(
                        "bigint.from of a {} is not modelled yet (Int only)",
                        other.type_name()
                    ),
                ),
            }
        }
        "bigint.pow" => {
            arity(name, &args, 2)?;
            pow(&want_big(name, &args[0])?, want_int(name, &args[1])?)
        }
        "bigint.abs" => {
            arity(name, &args, 1)?;
            let b = want_big(name, &args[0])?;
            Ok(to_value(make(false, b.mag)))
        }
        "bigint.sign" => {
            arity(name, &args, 1)?;
            let b = want_big(name, &args[0])?;
            Ok(Value::Int(if b.neg {
                -1
            } else if b.mag.is_empty() {
                0
            } else {
                1
            }))
        }
        "bigint.compare" => {
            arity(name, &args, 2)?;
            let a = want_big(name, &args[0])?;
            let b = want_big(name, &args[1])?;
            Ok(Value::Int(match (a.neg, b.neg) {
                (false, true) => 1,
                (true, false) => -1,
                (false, false) => cmp_mag(&a.mag, &b.mag),
                (true, true) => cmp_mag(&b.mag, &a.mag),
            }))
        }
        "bigint.bit_length" => {
            arity(name, &args, 1)?;
            Ok(Value::Int(bit_length(&want_big(name, &args[0])?.mag)))
        }
        "bigint.is_even" | "bigint.is_odd" => {
            arity(name, &args, 1)?;
            let b = want_big(name, &args[0])?;
            let odd = b.mag.first().is_some_and(|l| l % 2 == 1);
            Ok(Value::Bool(odd == (name == "bigint.is_odd")))
        }
        _ => abstain(
            &format!("stdlib:{name}"),
            format!("{name} is not modelled yet"),
        ),
    }
}
