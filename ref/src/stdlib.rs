//! The stdlib as the judge reads it — every function written from the
//! specification text (ALS collections/strings/text-and-numbers chapters and
//! the stdlib documents), never delegated to the host for ALS-specified
//! semantics (ADR-0015 clause 5; `clippy.toml` forbids the tempting std
//! methods). Anything not here is an ABSTAIN with class `stdlib:<module.fn>`
//! — measured by `scripts/check-ref-totality.sh` against what the corpora
//! call, never a silent pass.

use std::rc::Rc;

use crate::eval::{Flow, Interp};
use crate::value::{fmt_int, render, Value};

/// Names the evaluator implements, for the totality gate (`als-ref stdlib-index`).
pub const IMPLEMENTED: &[&str] = &[
    "println",
    "eprintln",
    "assert",
    "assert_eq",
    "assert_ne",
    "list.pop",
    "list.clear",
    "int.to_string",
    "int.abs",
    "int.min",
    "int.max",
    "string.len",
    "string.concat",
    "list.len",
    "list.get",
    "list.push",
    "list.join",
];

fn arity(name: &str, args: &[Value], n: usize) -> Result<(), Flow> {
    if args.len() != n {
        return Err(Flow::Fatal(format!(
            "{name}: expected {n} argument(s), got {}",
            args.len()
        )));
    }
    Ok(())
}

fn want_str<'a>(name: &str, v: &'a Value) -> Result<&'a Rc<str>, Flow> {
    match v {
        Value::Str(s) => Ok(s),
        other => Err(Flow::Abstain { class: "semantics:type-mismatch".into(), reason: format!("{name}: expected String, got {} — the implementation accepted a program the ALS-reading evaluator cannot type (an implicit conversion site?)", other.type_name()) }),
    }
}

fn want_int(name: &str, v: &Value) -> Result<i64, Flow> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(Flow::Abstain { class: "semantics:type-mismatch".into(), reason: format!("{name}: expected Int, got {} — the implementation accepted a program the ALS-reading evaluator cannot type (an implicit conversion site?)", other.type_name()) }),
    }
}

fn want_list<'a>(name: &str, v: &'a Value) -> Result<&'a Rc<Vec<Value>>, Flow> {
    match v {
        Value::List(xs) => Ok(xs),
        other => Err(Flow::Abstain { class: "semantics:type-mismatch".into(), reason: format!("{name}: expected List, got {} — the implementation accepted a program the ALS-reading evaluator cannot type (an implicit conversion site?)", other.type_name()) }),
    }
}

/// Dispatch a stdlib / prelude call. `Err(Flow::Abstain)` names the class.
pub fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> Result<Value, Flow> {
    match name {
        // ── prelude (language.md §11) ──────────────────────────────────
        "println" => {
            arity(name, &args, 1)?;
            let s = render_arg(&args[0])?;
            it.stdout.push_str(&s);
            it.stdout.push('\n');
            Ok(Value::Unit)
        }
        "eprintln" => {
            arity(name, &args, 1)?;
            let s = render_arg(&args[0])?;
            it.stderr.push_str(&s);
            it.stderr.push('\n');
            Ok(Value::Unit)
        }
        // C-153 (ALS-T18): a failing assert OUTSIDE a test block aborts with a
        // structured block — `Error: assertion failed[: <msg>]` then one
        // `  key: value` line each: `at: line N`, `expected: <r>` (`!= <l>` for
        // ne), `found: <l>`; field order is part of the promise.
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
                other => Err(Flow::Fatal(format!(
                    "assert: expected Bool, got {}",
                    other.type_name()
                ))),
            }
        }
        "assert_eq" | "assert_ne" => {
            arity(name, &args, 2)?;
            let eq = crate::value::values_eq(&args[0], &args[1])
                .ok_or_else(|| Flow::Fatal(format!("{name}: incomparable values")))?;
            let want = name == "assert_eq";
            if eq == want {
                Ok(Value::Unit)
            } else {
                let l = render_arg(&args[0])?;
                let r = render_arg(&args[1])?;
                let expected = if want { r } else { format!("!= {l}") };
                Err(Flow::Abort(format!(
                    "assertion failed\n  at: line {}\n  expected: {expected}\n  found: {l}",
                    it.cur_line
                )))
            }
        }
        // ── int ────────────────────────────────────────────────────────
        "int.to_string" => {
            arity(name, &args, 1)?;
            Ok(Value::str(&fmt_int(want_int(name, &args[0])?)))
        }
        "int.abs" => {
            arity(name, &args, 1)?;
            Ok(Value::Int(want_int(name, &args[0])?.wrapping_abs()))
        }
        "int.min" => {
            arity(name, &args, 2)?;
            let (a, b) = (want_int(name, &args[0])?, want_int(name, &args[1])?);
            Ok(Value::Int(if a < b { a } else { b }))
        }
        "int.max" => {
            arity(name, &args, 2)?;
            let (a, b) = (want_int(name, &args[0])?, want_int(name, &args[1])?);
            Ok(Value::Int(if a > b { a } else { b }))
        }
        // ── string ─────────────────────────────────────────────────────
        "string.len" => {
            arity(name, &args, 1)?;
            // ALS strings: length counts Unicode scalar values (chars), not bytes
            Ok(Value::Int(want_str(name, &args[0])?.chars().count() as i64))
        }
        "string.concat" => {
            arity(name, &args, 2)?;
            let mut s = want_str(name, &args[0])?.to_string();
            s.push_str(want_str(name, &args[1])?);
            Ok(Value::str(&s))
        }
        // ── list ───────────────────────────────────────────────────────
        "list.len" => {
            arity(name, &args, 1)?;
            Ok(Value::Int(want_list(name, &args[0])?.len() as i64))
        }
        "list.get" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?;
            let i = want_int(name, &args[1])?;
            if i < 0 || i as usize >= xs.len() {
                Ok(Value::None)
            } else {
                Ok(Value::Some(Rc::new(xs[i as usize].clone())))
            }
        }
        "list.push" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?;
            let mut v = (**xs).clone();
            v.push(args[1].clone());
            Ok(Value::List(Rc::new(v)))
        }
        "list.pop" => {
            arity(name, &args, 1)?;
            let xs = want_list(name, &args[0])?;
            let mut v = (**xs).clone();
            v.pop();
            Ok(Value::List(Rc::new(v)))
        }
        "list.clear" => {
            arity(name, &args, 1)?;
            let _ = want_list(name, &args[0])?;
            Ok(Value::List(Rc::new(Vec::new())))
        }
        "list.join" => {
            arity(name, &args, 2)?;
            let xs = want_list(name, &args[0])?;
            let sep = want_str(name, &args[1])?;
            let mut out = String::new();
            for (i, x) in xs.iter().enumerate() {
                if i > 0 {
                    out.push_str(sep);
                }
                out.push_str(want_str(name, x)?);
            }
            Ok(Value::str(&out))
        }
        other => Err(Flow::Abstain {
            class: format!("stdlib:{other}"),
            reason: format!(
                "stdlib function `{other}` is not implemented by the reference evaluator yet"
            ),
        }),
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
