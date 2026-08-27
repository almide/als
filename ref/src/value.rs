//! Abstract values (ADR-0015 decision 2): what a program OBSERVES, not how an
//! implementation lays it out. Rendering follows ALS-R2 (interpolation display
//! forms) and ALS-E1/E2 (Int decimal, Bool lowercase).

use std::rc::Rc;

use crate::ast::{Expr, LParam};

/// A binary64 value that deliberately does NOT implement `Display`: the only
/// way to render a float is `fmt_float` (ALS text-and-numbers), so a host
/// `{}` formatting can never leak in by accident (ADR-0015 clause 5, made
/// structural).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct F64(pub f64);

#[derive(Clone, Debug)]
pub struct Closure {
    pub params: Vec<LParam>,
    pub body: Expr,
    pub env: Rc<crate::eval::Env>,
    /// an effect-slot lambda (its `!` falls into its own failure channel)
    pub fallible: bool,
}

#[derive(Clone, Debug)]
pub enum Callable {
    /// a declared fn/effect fn by name (looked up in the program)
    Named(String),
    /// `Type.method` convention method
    Method(String, String),
    /// a stdlib fn `module.fn`
    Std(String),
    /// a lambda closure
    Closure(Rc<Closure>),
    /// `f >> g`
    Composed(Rc<Callable>, Rc<Callable>),
    /// a variant constructor with payload, used as a function value
    Ctor(String, String),
    /// derived Codec entry on a `: Codec` type — `T.decode` / `T.encode`
    /// (ALS-D6); the bool is true for decode, the fallible direction
    Codec(String, bool),
}

/// record / record-payload fields in display order
pub type Fields = Rc<Vec<(Rc<str>, Value)>>;

#[derive(Clone, Debug)]
pub enum Value {
    Int(i64),
    Float(F64),
    Bool(bool),
    Unit,
    Str(Rc<str>),
    List(Rc<Vec<Value>>),
    /// insertion-ordered association list (ALS-E12: iteration order is
    /// insertion order; lookup is by structural equality)
    Map(Rc<Vec<(Value, Value)>>),
    /// insertion-ordered
    Set(Rc<Vec<Value>>),
    Tuple(Rc<Vec<Value>>),
    /// `type_name` is None for anonymous records; fields keep DECLARATION
    /// order for named records and are sorted by name for anonymous ones
    /// (ALS-R2)
    Record {
        type_name: Option<Rc<str>>,
        fields: Fields,
    },
    Variant {
        type_name: Rc<str>,
        case: Rc<str>,
        payload: Payload,
    },
    Some(Rc<Value>),
    None,
    Ok(Rc<Value>),
    Err(Rc<Value>),
    Fn(Rc<Callable>),
    /// a first-class range `lo ..< hi` (half-open, Int): iterated lazily as a
    /// for-in head (C-238 / #1400), materialized only when forced
    Range(i64, i64),
    /// the dynamic `Value` type (value/json modules) — its text IS its JSON
    /// (ALS-D2, compact, insertion-ordered keys)
    Dyn(Dyn),
    /// the `Bytes` buffer — REFERENCE semantics: writes through a parameter
    /// are visible to the caller (bytes_param_writeback)
    Bytes(Rc<std::cell::RefCell<Vec<u8>>>),
    /// an opaque `JsonPath` (json.root/field/index)
    Path(Rc<Vec<PathSeg>>),
    /// the `matrix` value — row-major f64 storage (C-161 family)
    Matrix(Rc<Mat>),
}

/// the matrix payload: dimensions are the NORMALIZED ones (negatives clamped
/// to 0, the 2^28 element/row ceiling enforced at construction — C-161)
#[derive(Clone, Debug)]
pub struct Mat {
    pub rows: i64,
    pub cols: i64,
    pub data: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PathSeg {
    Field(Rc<str>),
    Index(i64),
}

#[derive(Clone, Debug)]
pub enum Dyn {
    Null,
    B(bool),
    I(i64),
    F(f64),
    S(Rc<str>),
    A(Rc<Vec<Dyn>>),
    O(Rc<Vec<(Rc<str>, Dyn)>>),
}

/// ALS-D2: a Value's string form is its JSON text — compact, keys in
/// insertion order, numbers in the canonical display form (integral floats
/// drop the fraction: value_repr pins `fltint=3`).
pub fn dyn_text(d: &Dyn) -> String {
    match d {
        Dyn::Null => "null".into(),
        Dyn::B(b) => {
            if *b {
                "true".into()
            } else {
                "false".into()
            }
        }
        Dyn::I(n) => fmt_int(*n),
        Dyn::F(f) => crate::fmtfloat::display_form(F64(*f)),
        Dyn::S(s) => json_quote(s),
        Dyn::A(items) => {
            let mut out = String::from("[");
            for (i, x) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&dyn_text(x));
            }
            out.push(']');
            out
        }
        Dyn::O(fields) => {
            let mut out = String::from("{");
            for (i, (k, v)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&json_quote(k));
                out.push(':');
                out.push_str(&dyn_text(v));
            }
            out.push('}');
            out
        }
    }
}

/// the canonical 5-escape rule (stdlib value_core __json_quote): backslash,
/// quote, \n, \r, \t — every other byte, control chars included, is RAW
pub fn json_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn dyn_eq(a: &Dyn, b: &Dyn) -> bool {
    match (a, b) {
        (Dyn::Null, Dyn::Null) => true,
        (Dyn::B(x), Dyn::B(y)) => x == y,
        (Dyn::I(x), Dyn::I(y)) => x == y,
        (Dyn::F(x), Dyn::F(y)) => x == y,
        (Dyn::I(x), Dyn::F(y)) | (Dyn::F(y), Dyn::I(x)) => *x as f64 == *y,
        (Dyn::S(x), Dyn::S(y)) => x == y,
        (Dyn::A(x), Dyn::A(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| dyn_eq(p, q))
        }
        (Dyn::O(x), Dyn::O(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.iter().any(|(k2, v2)| k == k2 && dyn_eq(v, v2)))
        }
        _ => false,
    }
}

#[derive(Clone, Debug)]
pub enum Payload {
    Unit,
    Tuple(Rc<Vec<Value>>),
    Record(Fields),
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(Rc::from(s))
    }
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "Int",
            Value::Float(_) => "Float",
            Value::Bool(_) => "Bool",
            Value::Unit => "Unit",
            Value::Str(_) => "String",
            Value::List(_) => "List",
            Value::Map(_) => "Map",
            Value::Set(_) => "Set",
            Value::Tuple(_) => "Tuple",
            Value::Record { .. } => "Record",
            Value::Variant { .. } => "Variant",
            Value::Some(_) | Value::None => "Option",
            Value::Ok(_) | Value::Err(_) => "Result",
            Value::Fn(_) => "Fn",
            Value::Range(..) => "List",
            Value::Dyn(_) => "Value",
            Value::Bytes(_) => "Bytes",
            Value::Path(_) => "JsonPath",
            Value::Matrix(_) => "Matrix",
        }
    }
    /// number of elements a range would materialize
    pub fn range_len(lo: i64, hi: i64) -> u128 {
        if hi <= lo {
            0
        } else {
            (hi as i128 - lo as i128) as u128
        }
    }
}

/// Structural equality (ALS-E29 "等値の意味論は型ごと — スカラーは prim 比較、
/// String / List / Value は深い比較"; ALS-E4 Unit reflexive; ALS-E8 tuples
/// element-wise). Fn values are not comparable — callers treat `None` as a
/// type error surfaced by the checker, never reached on accepted programs.
pub fn values_eq(a: &Value, b: &Value) -> Option<bool> {
    Some(match (a, b) {
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x.0 == y.0,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Unit, Value::Unit) => true,
        (Value::Str(x), Value::Str(y)) => x == y,
        // ALS-C3: sets compare as SETS — same members, any insertion order
        (Value::Set(x), Value::Set(y)) => {
            if x.len() != y.len() {
                return Some(false);
            }
            for v in x.iter() {
                let mut hit = false;
                for w in y.iter() {
                    if values_eq(v, w)? {
                        hit = true;
                        break;
                    }
                }
                if !hit {
                    return Some(false);
                }
            }
            true
        }
        (Value::List(x), Value::List(y)) | (Value::Tuple(x), Value::Tuple(y)) => {
            if x.len() != y.len() {
                return Some(false);
            }
            for (p, q) in x.iter().zip(y.iter()) {
                if !values_eq(p, q)? {
                    return Some(false);
                }
            }
            true
        }
        (Value::Map(x), Value::Map(y)) => {
            if x.len() != y.len() {
                return Some(false);
            }
            for (k, v) in x.iter() {
                match y.iter().find(|(k2, _)| values_eq(k, k2) == Some(true)) {
                    Some((_, v2)) => {
                        if !values_eq(v, v2)? {
                            return Some(false);
                        }
                    }
                    None => return Some(false),
                }
            }
            true
        }
        (
            Value::Record {
                type_name: tn1,
                fields: f1,
            },
            Value::Record {
                type_name: tn2,
                fields: f2,
            },
        ) => {
            if tn1 != tn2 || f1.len() != f2.len() {
                return Some(false);
            }
            for (n, v) in f1.iter() {
                match f2.iter().find(|(n2, _)| n == n2) {
                    Some((_, v2)) => {
                        if !values_eq(v, v2)? {
                            return Some(false);
                        }
                    }
                    None => return Some(false),
                }
            }
            true
        }
        (
            Value::Variant {
                type_name: t1,
                case: c1,
                payload: p1,
            },
            Value::Variant {
                type_name: t2,
                case: c2,
                payload: p2,
            },
        ) => {
            if t1 != t2 || c1 != c2 {
                return Some(false);
            }
            match (p1, p2) {
                (Payload::Unit, Payload::Unit) => true,
                (Payload::Tuple(x), Payload::Tuple(y)) => {
                    values_eq(&Value::Tuple(x.clone()), &Value::Tuple(y.clone()))?
                }
                (Payload::Record(x), Payload::Record(y)) => values_eq(
                    &Value::Record {
                        type_name: None,
                        fields: x.clone(),
                    },
                    &Value::Record {
                        type_name: None,
                        fields: y.clone(),
                    },
                )?,
                _ => false,
            }
        }
        (Value::Some(x), Value::Some(y)) => values_eq(x, y)?,
        (Value::None, Value::None) => true,
        (Value::Some(_), Value::None) | (Value::None, Value::Some(_)) => false,
        (Value::Ok(x), Value::Ok(y)) | (Value::Err(x), Value::Err(y)) => values_eq(x, y)?,
        (Value::Ok(_), Value::Err(_)) | (Value::Err(_), Value::Ok(_)) => false,
        (Value::Dyn(x), Value::Dyn(y)) => dyn_eq(x, y),
        (Value::Bytes(x), Value::Bytes(y)) => *x.borrow() == *y.borrow(),
        (Value::Path(x), Value::Path(y)) => x == y,
        (Value::Fn(_), _) | (_, Value::Fn(_)) => return None,
        (Value::Range(..), _) | (_, Value::Range(..)) => return None,
        _ => return None,
    })
}

/// ALS-C9: type-directed total order — Int and String total, Float by
/// IEEE-754 totalOrder (NaN to the max side, -0 < +0). Other types have no
/// specified order yet → None (the caller abstains).
pub fn value_cmp(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::Float(x), Value::Float(y)) => Some(x.0.total_cmp(&y.0)),
        (Value::Str(x), Value::Str(y)) => Some(crate::eval::char_cmp(x, y)),
        _ => None,
    }
}

/// ALS-E1: decimal, including i64::MIN.
pub fn fmt_int(n: i64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let neg = n < 0;
    // work on the magnitude as u64 so i64::MIN does not overflow
    let mut m: u64 = if neg {
        (n as i128).unsigned_abs() as u64
    } else {
        n as u64
    };
    let mut digits: Vec<u8> = Vec::new();
    while m > 0 {
        digits.push(b'0' + (m % 10) as u8);
        m /= 10;
    }
    if neg {
        digits.push(b'-');
    }
    digits.reverse();
    String::from_utf8(digits).expect("ascii digits")
}

/// ALS-R2 interpolation display form. `None` = a rendering this evaluator
/// does not implement yet (the caller abstains with class `render:<what>`).
pub fn render(v: &Value) -> Option<String> {
    Some(match v {
        Value::Int(n) => fmt_int(*n),
        Value::Float(f) => crate::fmtfloat::display_form(*f),
        Value::Bool(b) => {
            if *b {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        Value::Unit => "()".to_string(),
        Value::Str(s) => s.to_string(),
        Value::List(xs) => {
            let mut out = String::from("[");
            for (i, x) in xs.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&render_literal(x)?);
            }
            out.push(']');
            out
        }
        Value::Tuple(xs) => {
            let mut out = String::from("(");
            for (i, x) in xs.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&render_literal(x)?);
            }
            if xs.len() == 1 {
                out.push(',');
            }
            out.push(')');
            out
        }
        Value::Some(x) => format!("some({})", render_literal(x)?),
        Value::None => "none".to_string(),
        Value::Ok(x) => format!("ok({})", render_literal(x)?),
        Value::Err(x) => format!("err({})", render_literal(x)?),
        // the pinned repr forms (ALS-R2; compound_repr_interp / records fixtures)
        Value::Map(kvs) => {
            if kvs.is_empty() {
                "[:]".to_string()
            } else {
                let mut out = String::from("[");
                for (i, (k, v2)) in kvs.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&render_literal(k)?);
                    out.push_str(": ");
                    out.push_str(&render_literal(v2)?);
                }
                out.push(']');
                out
            }
        }
        Value::Set(xs) => {
            let mut out = String::from("set.from_list([");
            for (i, x) in xs.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&render_literal(x)?);
            }
            out.push_str("])");
            out
        }
        Value::Record { type_name, fields } => {
            let mut out = match type_name {
                Some(t) => format!("{t} {{ "),
                None => "{ ".to_string(),
            };
            for (i, (n, v2)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(n);
                out.push_str(": ");
                out.push_str(&render_literal(v2)?);
            }
            out.push_str(" }");
            out
        }
        Value::Variant { case, payload, .. } => match payload {
            Payload::Unit => case.to_string(),
            Payload::Tuple(items) => {
                let mut out = format!("{case}(");
                for (i, x) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&render_literal(x)?);
                }
                out.push(')');
                out
            }
            Payload::Record(fields) => {
                let mut out = format!("{case} {{ ");
                for (i, (n, v2)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(n);
                    out.push_str(": ");
                    out.push_str(&render_literal(v2)?);
                }
                out.push_str(" }");
                out
            }
        },
        Value::Dyn(d) => dyn_text(d),
        Value::Fn(_) | Value::Range(..) | Value::Bytes(_) | Value::Path(_) | Value::Matrix(_) => {
            return None
        }
    })
}

/// Almide-literal form used INSIDE containers (ALS-R2): strings are quoted.
pub fn render_literal(v: &Value) -> Option<String> {
    match v {
        Value::Str(s) => Some(quote_str(s)),
        other => render(other),
    }
}

fn quote_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
