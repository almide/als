//! The `testing` asserts as ALS-T18 reads them (C-381): a direct call
//! outside a test block passes silently or aborts with the T18 block —
//! `assertion failed`, `  at: line <N>`, `  expected: <relation>`,
//! `  found: <operand>` — the same record C-153 gives `assert_eq`.
//! The evaluator runs programs, never test blocks, so every call it sees is
//! outside one. `testing.assert_aborts` (ALS-T30) is legal only inside a test
//! block — outside one it is a check-time error (E099) — so no program the
//! evaluator runs calls it; it is listed so a call abstains by name.

use std::rc::Rc;

use crate::eval::{Flow, Interp};
use crate::value::{render, Value};

pub const TESTING_FNS: &[&str] = &[
    "testing.assert_gt",
    "testing.assert_lt",
    "testing.assert_approx",
    "testing.assert_contains",
    "testing.assert_some",
    "testing.assert_none",
    "testing.assert_ok",
    "testing.assert_err",
    "testing.assert_aborts",
];

fn mismatch<T>(name: &str, want: &str, got: &Value) -> Result<T, Flow> {
    Err(Flow::Fatal(format!(
        "{name}: expected {want}, got {}",
        got.type_name()
    )))
}

fn shown(v: &Value) -> Result<String, Flow> {
    render(v).ok_or_else(|| Flow::Abstain {
        class: format!("render:{}", v.type_name()),
        reason: format!(
            "rendering a {} is not implemented by the reference evaluator yet",
            v.type_name()
        ),
    })
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

fn want_str<'a>(name: &str, v: &'a Value) -> Result<&'a Rc<str>, Flow> {
    match v {
        Value::Str(s) => Ok(s),
        other => mismatch(name, "String", other),
    }
}

/// substring test over code points; the empty needle is contained (ALS-S2)
fn contains(hay: &str, needle: &str) -> bool {
    let h: Vec<char> = hay.chars().collect();
    let n: Vec<char> = needle.chars().collect();
    if n.is_empty() {
        return true;
    }
    if n.len() > h.len() {
        return false;
    }
    (0..=h.len() - n.len()).any(|i| h[i..i + n.len()] == n[..])
}

/// The T18 abort record (the `Error: ` prefix is the termination
/// convention's, added by the runner).
fn fail(line: usize, expected: String, found: String) -> Result<Value, Flow> {
    Err(Flow::Abort(format!(
        "assertion failed\n  at: line {line}\n  expected: {expected}\n  found: {found}"
    )))
}

pub fn call_testing(it: &mut Interp, name: &str, args: Vec<Value>) -> Option<Result<Value, Flow>> {
    if !TESTING_FNS.contains(&name) {
        return None;
    }
    Some(dispatch(it, name, args))
}

fn arity(name: &str, args: &[Value], n: usize) -> Result<(), Flow> {
    if args.len() == n {
        Ok(())
    } else {
        Err(Flow::Fatal(format!(
            "{name}: expected {n} arguments, got {}",
            args.len()
        )))
    }
}

fn dispatch(it: &mut Interp, name: &str, args: Vec<Value>) -> Result<Value, Flow> {
    let line = it.cur_line;
    match name {
        "testing.assert_gt" | "testing.assert_lt" => {
            arity(name, &args, 2)?;
            let a = want_int(name, &args[0])?;
            let b = want_int(name, &args[1])?;
            let (pass, rel) = if name == "testing.assert_gt" {
                (a > b, ">")
            } else {
                (a < b, "<")
            };
            if pass {
                return Ok(Value::Unit);
            }
            fail(
                line,
                format!("{rel} {}", shown(&args[1])?),
                shown(&args[0])?,
            )
        }
        // |a - b| < tolerance, strict; a NaN anywhere fails
        "testing.assert_approx" => {
            arity(name, &args, 3)?;
            let a = want_float(name, &args[0])?;
            let b = want_float(name, &args[1])?;
            let t = want_float(name, &args[2])?;
            if a - b < t && b - a < t {
                return Ok(Value::Unit);
            }
            let expected = format!("within {} of {}", shown(&args[2])?, shown(&args[1])?);
            fail(line, expected, shown(&args[0])?)
        }
        "testing.assert_contains" => {
            arity(name, &args, 2)?;
            let hay = want_str(name, &args[0])?;
            let needle = want_str(name, &args[1])?;
            if contains(hay, needle) {
                return Ok(Value::Unit);
            }
            fail(line, format!("contains {needle}"), hay.to_string())
        }
        "testing.assert_some" | "testing.assert_none" => {
            arity(name, &args, 1)?;
            let is_some = match &args[0] {
                Value::Some(_) => true,
                Value::None => false,
                other => return mismatch(name, "Option", other),
            };
            match (name == "testing.assert_some", is_some) {
                (true, true) | (false, false) => Ok(Value::Unit),
                (true, false) => fail(line, "some(_)".into(), "none".into()),
                (false, true) => fail(line, "none".into(), shown(&args[0])?),
            }
        }
        "testing.assert_ok" | "testing.assert_err" => {
            arity(name, &args, 1)?;
            let is_ok = match &args[0] {
                Value::Ok(_) => true,
                Value::Err(_) => false,
                other => return mismatch(name, "Result", other),
            };
            let want_ok = name == "testing.assert_ok";
            if is_ok == want_ok {
                return Ok(Value::Unit);
            }
            let expected = if want_ok { "ok(_)" } else { "err(_)" };
            fail(line, expected.into(), shown(&args[0])?)
        }
        // ALS-T30: a child-process assertion of test blocks only (E099
        // outside one), which the evaluator never runs.
        _ => Err(Flow::Abstain {
            class: "testing:assert_aborts".into(),
            reason: "testing.assert_aborts runs only in a test block, in a child; the reference evaluator runs programs".into(),
        }),
    }
}
