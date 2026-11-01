//! A fused walk of `list.range(a, b) |> list.map(f) |> list.filter(p) |> …
//! |> list.fold(init, g)` when every callback is pure Int arithmetic over its
//! parameters and Int values it captures — the pipeline the implementations
//! fuse into one counting loop (C-238's head fold). The fused walk calls the
//! same arithmetic in the same order the stage-by-stage evaluation would, so
//! it observes the same results: `+ - *` wrap two's-complement, `/` and `%`
//! abort on a zero divisor or `Int.MIN / -1` (ALS-T6), and the materialization
//! bound of `list.range` (a span over 2^31 is `out of memory`) is kept. It
//! only skips building the intermediate lists, and it charges fuel at 1/64 per
//! element, as the evaluator's compiled counting loop does.
//!
//! Anything else — a callback that is not plain Int arithmetic, an init that
//! is not an Int literal, a metered (fan) region — is left to the ordinary
//! stage-by-stage evaluation.
use std::rc::Rc;

use crate::ast::{Arg, BinOp, Expr, LParam, UnOp};
use crate::eval::{Env, Flow};

#[derive(Clone, Copy, Debug)]
pub enum KOp {
    Const(i64),
    Param(usize),
    Bin(BinOp),
    Neg,
    Not,
}

pub enum Stage {
    Map(Vec<KOp>),
    Filter(Vec<KOp>),
}

/// The recognised pipeline: the range bounds' expressions, the stages, and
/// the fold (its Int-literal init and body).
pub struct Plan<'a> {
    pub lo: &'a Expr,
    pub hi: &'a Expr,
    pub stages: Vec<Stage>,
    pub init: i64,
    pub fold: Vec<KOp>,
}

fn std_call<'a>(e: &'a Expr, module: &str, name: &str) -> Option<&'a [Arg]> {
    match e {
        Expr::Call { callee, args, .. } => match &**callee {
            Expr::Member { obj, name: n } if n == name => match &**obj {
                Expr::Ident(m) if m == module => Some(args),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

fn pos_args(args: &[Arg]) -> Option<Vec<&Expr>> {
    args.iter()
        .map(|a| match a {
            Arg::Pos(e) => Some(e),
            _ => None,
        })
        .collect()
}

fn int_literal(e: &Expr) -> Option<i64> {
    match e {
        Expr::Int(k) => Some(*k),
        Expr::Paren(x) => int_literal(x),
        Expr::Unary {
            op: UnOp::Neg,
            expr,
        } => int_literal(expr).and_then(i64::checked_neg),
        _ => None,
    }
}

/// a lambda of `arity` plain parameters whose body compiles
fn lambda(e: &Expr, arity: usize, env: &Rc<Env>) -> Option<(Vec<KOp>, Kind)> {
    let Expr::Lambda { params, body } = e else {
        return None;
    };
    if params.len() != arity {
        return None;
    }
    let names: Vec<&str> = params
        .iter()
        .map(|p: &LParam| {
            if p.names.len() == 1 {
                Some(p.names[0].as_str())
            } else {
                None
            }
        })
        .collect::<Option<Vec<_>>>()?;
    let mut code = Vec::new();
    let kind = compile(body, &names, env, &mut code)?;
    Some((code, kind))
}

/// what a compiled expression yields; the parameters are all Int
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Int,
    Bool,
}

/// whether `e` holds a `/` or `%` — eager evaluation of a short-circuit
/// operand that divides could abort where the stage-by-stage walk does not
fn divides(e: &Expr) -> bool {
    match e {
        Expr::Binary { op, lhs, rhs } => {
            matches!(op, BinOp::Div | BinOp::Rem) || divides(lhs) || divides(rhs)
        }
        Expr::Unary { expr, .. } | Expr::Paren(expr) => divides(expr),
        _ => false,
    }
}

fn compile(e: &Expr, params: &[&str], env: &Rc<Env>, code: &mut Vec<KOp>) -> Option<Kind> {
    match e {
        Expr::Int(k) => {
            code.push(KOp::Const(*k));
            Some(Kind::Int)
        }
        Expr::Bool(b) => {
            code.push(KOp::Const(i64::from(*b)));
            Some(Kind::Bool)
        }
        Expr::Ident(n) => {
            if let Some(i) = params.iter().rposition(|p| *p == n.as_str() && *p != "_") {
                code.push(KOp::Param(i));
                return Some(Kind::Int);
            }
            // a captured Int: pure callbacks cannot change it mid-walk
            match env.lookup(n) {
                Some(crate::value::Value::Int(k)) => {
                    code.push(KOp::Const(k));
                    Some(Kind::Int)
                }
                _ => None,
            }
        }
        Expr::Paren(x) => compile(x, params, env, code),
        Expr::Unary { op, expr } => {
            let k = compile(expr, params, env, code)?;
            match (op, k) {
                (UnOp::Neg, Kind::Int) => code.push(KOp::Neg),
                (UnOp::Not, Kind::Bool) => code.push(KOp::Not),
                _ => return None,
            }
            Some(k)
        }
        Expr::Binary { op, lhs, rhs } => {
            if matches!(op, BinOp::And | BinOp::Or) && divides(rhs) {
                return None;
            }
            let a = compile(lhs, params, env, code)?;
            let b = compile(rhs, params, env, code)?;
            let out = match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem
                    if a == Kind::Int && b == Kind::Int =>
                {
                    Kind::Int
                }
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
                    if a == Kind::Int && b == Kind::Int =>
                {
                    Kind::Bool
                }
                BinOp::Eq | BinOp::Ne if a == b => Kind::Bool,
                BinOp::And | BinOp::Or if a == Kind::Bool && b == Kind::Bool => Kind::Bool,
                _ => return None,
            };
            code.push(KOp::Bin(*op));
            Some(out)
        }
        _ => None,
    }
}

/// recognise `rhs` = `list.fold(<int literal>, (acc, x) => …)` piped from a
/// `list.range(a, b)` through `list.map` / `list.filter` stages
pub fn plan<'a>(lhs: &'a Expr, rhs: &'a Expr, env: &Rc<Env>) -> Option<Plan<'a>> {
    let fold_args = pos_args(std_call(rhs, "list", "fold")?)?;
    if fold_args.len() != 2 {
        return None;
    }
    let init = int_literal(fold_args[0])?;
    let (fold, fk) = lambda(fold_args[1], 2, env)?;
    if fk != Kind::Int {
        return None;
    }
    let mut stages = Vec::new();
    let mut cur = lhs;
    loop {
        if let Some(args) = std_call(cur, "list", "range") {
            let a = pos_args(args)?;
            if a.len() != 2 {
                return None;
            }
            stages.reverse();
            return Some(Plan {
                lo: a[0],
                hi: a[1],
                stages,
                init,
                fold,
            });
        }
        let Expr::Pipe { lhs: l, rhs: r } = cur else {
            return None;
        };
        let (f, stage_args) = if let Some(args) = std_call(r, "list", "map") {
            (true, args)
        } else if let Some(args) = std_call(r, "list", "filter") {
            (false, args)
        } else {
            return None;
        };
        let sa = pos_args(stage_args)?;
        if sa.len() != 1 {
            return None;
        }
        let (code, k) = lambda(sa[0], 1, env)?;
        stages.push(match (f, k) {
            (true, Kind::Int) => Stage::Map(code),
            (false, Kind::Bool) => Stage::Filter(code),
            _ => return None,
        });
        cur = l;
    }
}

/// run one compiled callback body over its arguments
pub fn exec(code: &[KOp], args: &[i64], stack: &mut Vec<i64>) -> Result<i64, Flow> {
    stack.clear();
    for op in code {
        match op {
            KOp::Const(k) => stack.push(*k),
            KOp::Param(i) => stack.push(args[*i]),
            KOp::Neg => {
                let a = stack.pop().expect("rpn");
                stack.push(a.wrapping_neg());
            }
            KOp::Not => {
                let a = stack.pop().expect("rpn");
                stack.push(i64::from(a == 0));
            }
            KOp::Bin(op) => {
                let b = stack.pop().expect("rpn");
                let a = stack.pop().expect("rpn");
                stack.push(match op {
                    BinOp::Add => a.wrapping_add(b),
                    BinOp::Sub => a.wrapping_sub(b),
                    BinOp::Mul => a.wrapping_mul(b),
                    BinOp::Div | BinOp::Rem => {
                        if b == 0 {
                            return Err(Flow::Abort("division by zero".into()));
                        }
                        let r = if matches!(op, BinOp::Div) {
                            a.checked_div(b)
                        } else {
                            a.checked_rem(b)
                        };
                        match r {
                            Some(v) => v,
                            None => return Err(Flow::Abort("integer overflow".into())),
                        }
                    }
                    BinOp::Eq => i64::from(a == b),
                    BinOp::Ne => i64::from(a != b),
                    BinOp::Lt => i64::from(a < b),
                    BinOp::Le => i64::from(a <= b),
                    BinOp::Gt => i64::from(a > b),
                    BinOp::Ge => i64::from(a >= b),
                    BinOp::And => i64::from(a != 0 && b != 0),
                    BinOp::Or => i64::from(a != 0 || b != 0),
                    BinOp::Pow => unreachable!("not compiled"),
                });
            }
        }
    }
    Ok(stack.pop().expect("rpn value"))
}
