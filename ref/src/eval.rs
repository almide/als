//! Evaluator — written from the ALS chapters (expressions.md ALS-E*/ST*/DL*,
//! semantics.md ALS-M*, runtime.md ALS-R*, result-option-effect.md) and the
//! λ_almd kernel (C-280). Every `match` over the AST is exhaustive; a form the
//! evaluator does not implement yet is an explicit `Flow::Abstain` with a
//! class (ADR-0015 clause 2). A state the evaluator considers impossible on a
//! type-checked program is `Flow::Fatal` — a protocol error, never a verdict.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::ast::*;
use crate::parser::parse_program;
use crate::stdlib;
use crate::value::*;

#[derive(Debug)]
pub enum Flow {
    /// ALS-R1 abort: `Error: <msg>` on stderr, exit 1
    Abort(String),
    /// `!` met an err/none: the failure travels to the enclosing fn boundary
    /// with its polarity (a Result channel reifies none as err("none"),
    /// ADR-0003 D3; an Option channel propagates none as none, C-211)
    Propagate(Prop),
    Break,
    Continue,
    Exit(i32),
    Abstain {
        class: String,
        reason: String,
    },
    Fatal(String),
}

#[derive(Debug)]
pub enum Prop {
    Err(Value),
    None,
}

impl Prop {
    fn as_err(&self) -> Value {
        match self {
            Prop::Err(e) => e.clone(),
            Prop::None => Value::str("none"),
        }
    }
}

pub type R = Result<Value, Flow>;
/// positional and named argument values
pub type Args = (Vec<Value>, Vec<(String, Value)>);

pub enum Outcome {
    Ran {
        exit: i32,
        stdout: String,
        stderr: String,
    },
    Abstain {
        class: String,
        reason: String,
    },
    Fault(String),
}

/// A binding is a shared SLOT: a closure captures the slots visible at its
/// creation (so a `var` write flows both ways — closure_captured_list_mutation,
/// sort_by_call_count), while a later shadowing `let` makes a NEW slot the
/// closure never sees (ref_gleam_capture_shadow).
type Slot = Rc<RefCell<Value>>;

#[derive(Debug)]
pub struct Env {
    vars: RefCell<Vec<(Rc<str>, Slot)>>,
    parent: Option<Rc<Env>>,
}

impl Env {
    pub fn new(parent: Option<Rc<Env>>) -> Rc<Env> {
        Rc::new(Env {
            vars: RefCell::new(Vec::new()),
            parent,
        })
    }
    fn slot(&self, name: &str) -> Option<Slot> {
        for (n, s) in self.vars.borrow().iter().rev() {
            if &**n == name {
                return Some(s.clone());
            }
        }
        self.parent.as_ref().and_then(|p| p.slot(name))
    }
    pub fn lookup(&self, name: &str) -> Option<Value> {
        self.slot(name).map(|s| s.borrow().clone())
    }
    pub fn define(&self, name: &str, v: Value) {
        self.vars
            .borrow_mut()
            .push((Rc::from(name), Rc::new(RefCell::new(v))));
    }
    /// assign through the nearest binding's slot; false if unbound
    pub fn assign(&self, name: &str, v: Value) -> bool {
        match self.slot(name) {
            Some(s) => {
                *s.borrow_mut() = v;
                true
            }
            None => false,
        }
    }
}

#[derive(Clone, Debug)]
enum CaseShape {
    Unit,
    Tuple(usize),
    Record(Vec<FieldDecl>),
}

/// Tail-position evaluation result: a self tail call is trampolined by
/// `call_fn` (ALS-M4: tail self-recursion runs in O(1) stack).
enum Tail {
    Value(Value),
    SelfCall(Vec<Value>),
    /// a tail call to ANOTHER top-level fn of the same channel class (C-178:
    /// mutual tail recursion at depth 10^6) — `unwrapped` records a `g(..)!`
    /// tail, whose unwrap-then-rewrap is the identity between same-class fns
    Call(Rc<FnDecl>, Vec<Value>),
}

pub struct Interp {
    fns: BTreeMap<String, Rc<FnDecl>>,
    methods: BTreeMap<(String, String), Rc<FnDecl>>,
    types: BTreeMap<String, Rc<TypeDecl>>,
    /// case name → (type name, shape)
    ctors: BTreeMap<String, (String, CaseShape)>,
    globals: Rc<Env>,
    pub stdout: String,
    /// env.set overlay (read before the host env; never written back to the host)
    pub env_overlay: Vec<(String, String)>,
    /// fixed-seed splitmix64 for the entropy floor (value nondeterminism is
    /// sanctioned; the ALS asserts only range properties)
    rand_state: u64,
    pub stderr: String,
    fuel: u64,
    in_test: bool,
    /// line of the innermost call being dispatched (C-153 `at: line N`)
    pub cur_line: usize,
}

const STD_MODULES: &[&str] = &[
    "string", "list", "map", "set", "int", "float", "value", "result", "option", "math",
    "datetime", "error", "bytes", "matrix", "prim", "int8", "int16", "int32", "uint8", "uint16",
    "uint32", "uint64", "float32", "json", "fs", "http", "env", "io", "random", "regex", "process",
    "testing", "path", "args", "net", "zlib", "base64", "hex", "html", "mem", "fan", "compute",
    "duration", "i8", "i16", "i32", "u8", "u16", "u32", "u64", "i64", "bool", "char", "tuple",
    "time", "url", "log", "hash", "base", "testing", "uuid", "csv", "toml", "yaml",
];

/// stdlib mutators that update their first argument IN PLACE and return Unit
/// (list.md: "Append an element in place. Requires var binding.")
/// Bytes bind by VALUE on let/var (`let snap = arena` snapshots —
/// mutable_global_repeat_writes) while call arguments alias the same buffer
/// (bytes_param_writeback)
fn snapshot_bytes(v: Value) -> Value {
    match v {
        Value::Bytes(b) => {
            let copy = b.borrow().clone();
            Value::Bytes(Rc::new(std::cell::RefCell::new(copy)))
        }
        other => other,
    }
}

/// canonicalize NaN results: every float operation that produces a NaN
/// observes as the single canonical quiet NaN (nan_canonical_observation)
pub fn fnan(x: f64) -> f64 {
    if x.is_nan() {
        f64::from_bits(0x7FF8000000000000)
    } else {
        x
    }
}

const IN_PLACE: &[&str] = &[
    "list.push",
    "list.pop",
    "list.clear",
    "map.insert",
    "map.delete",
    "map.clear",
    "string.push",
];

const PRELUDE: &[&str] = &[
    "println",
    "eprintln",
    "assert",
    "assert_eq",
    "assert_ne",
    "panic",
];

pub fn run_source(src: &str) -> Outcome {
    // Deep (non-tail) recursion in the judged program is evaluated on a thread
    // with a generous stack; the evaluator itself recurses per AST node.
    let src = src.to_string();
    let handle = std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(move || run_source_inner(&src));
    match handle {
        Ok(h) => match h.join() {
            Ok(o) => o,
            Err(_) => {
                Outcome::Fault("evaluator thread panicked (stack overflow or internal bug)".into())
            }
        },
        Err(e) => Outcome::Fault(format!("cannot spawn evaluator thread: {e}")),
    }
}

fn run_source_inner(src: &str) -> Outcome {
    let prog = match parse_program(src) {
        Ok(p) => p,
        Err(e) => {
            return Outcome::Abstain {
                class: "parse".into(),
                reason: format!("line {}: {}", e.line, e.msg),
            };
        }
    };
    let mut it = Interp::new(&prog);
    match it.init_globals(&prog) {
        Ok(()) => {}
        Err(f) => return it.finish(Err(f)),
    }
    let main = match it.fns.get("main") {
        Some(m) => m.clone(),
        None => {
            return Outcome::Abstain { class: "runtime:no-main".into(), reason: "the file declares no `main`; `als-ref run` judges programs (test files are not run yet)".into() };
        }
    };
    let r = it.call_fn(&main, Vec::new(), Vec::new());
    it.finish(r)
}

impl Interp {
    fn new(prog: &Program) -> Interp {
        let mut it = Interp {
            fns: BTreeMap::new(),
            methods: BTreeMap::new(),
            types: BTreeMap::new(),
            ctors: {
                // prelude enum Endian (bytes cursor API): bare nullary ctors
                let mut m = BTreeMap::new();
                m.insert(
                    "LittleEndian".to_string(),
                    ("Endian".to_string(), CaseShape::Unit),
                );
                m.insert(
                    "BigEndian".to_string(),
                    ("Endian".to_string(), CaseShape::Unit),
                );
                m
            },
            globals: Env::new(None),
            stdout: String::new(),
            env_overlay: Vec::new(),
            rand_state: 0x243F6A8885A308D3,
            stderr: String::new(),
            fuel: 200_000_000,
            in_test: false,
            cur_line: 0,
        };
        for d in &prog.decls {
            match d {
                Decl::Fn(f) => {
                    let f = Rc::new(f.clone());
                    match &f.sig.owner {
                        Some(t) => {
                            it.methods.insert((t.clone(), f.sig.name.clone()), f);
                        }
                        None => {
                            it.fns.insert(f.sig.name.clone(), f);
                        }
                    }
                }
                Decl::Type(t) => {
                    if let TypeBody::Variant(cases) = &t.body {
                        for c in cases {
                            let (name, shape) = match c {
                                VariantCase::Unit(n) => (n.clone(), CaseShape::Unit),
                                VariantCase::Tuple(n, tys) => {
                                    (n.clone(), CaseShape::Tuple(tys.len()))
                                }
                                VariantCase::Record(n, fs) => {
                                    (n.clone(), CaseShape::Record(fs.clone()))
                                }
                            };
                            it.ctors.insert(name, (t.name.clone(), shape));
                        }
                    }
                    it.types.insert(t.name.clone(), Rc::new(t.clone()));
                }
                Decl::TopLet { .. } | Decl::Protocol { .. } | Decl::Test { .. } => {}
            }
        }
        it
    }

    fn init_globals(&mut self, prog: &Program) -> Result<(), Flow> {
        let g = self.globals.clone();
        for d in &prog.decls {
            if let Decl::TopLet { name, expr, ty, .. } = d {
                let v = self.eval(&g, expr)?;
                let v = self.retag(v, ty.as_ref());
                g.define(name, v);
            }
        }
        Ok(())
    }

    fn finish(self, r: R) -> Outcome {
        let mut stdout = self.stdout;
        let mut stderr = self.stderr;
        match r {
            Ok(Value::Err(e)) => {
                let msg = match render(&e) {
                    Some(s) => s,
                    None => {
                        return Outcome::Abstain {
                            class: "render:error-value".into(),
                            reason: "cannot render the main error value".into(),
                        }
                    }
                };
                stderr.push_str(&format!("Error: {msg}\n"));
                Outcome::Ran {
                    exit: 1,
                    stdout,
                    stderr,
                }
            }
            Ok(_) => Outcome::Ran {
                exit: 0,
                stdout,
                stderr,
            },
            Err(Flow::Propagate(p)) => {
                let msg = render(&p.as_err()).unwrap_or_default();
                stderr.push_str(&format!("Error: {msg}\n"));
                Outcome::Ran {
                    exit: 1,
                    stdout,
                    stderr,
                }
            }
            Err(Flow::Abort(msg)) => {
                stderr.push_str(&format!("Error: {msg}\n"));
                Outcome::Ran {
                    exit: 1,
                    stdout,
                    stderr,
                }
            }
            Err(Flow::Exit(code)) => {
                stdout.truncate(stdout.len());
                Outcome::Ran {
                    exit: code,
                    stdout,
                    stderr,
                }
            }
            Err(Flow::Abstain { class, reason }) => Outcome::Abstain { class, reason },
            Err(Flow::Fatal(msg)) => Outcome::Fault(msg),
            Err(Flow::Break) | Err(Flow::Continue) => {
                Outcome::Fault("break/continue escaped to the program boundary".into())
            }
        }
    }

    fn abstain<T>(&self, class: &str, reason: impl Into<String>) -> Result<T, Flow> {
        Err(Flow::Abstain {
            class: class.to_string(),
            reason: reason.into(),
        })
    }

    /// stdlib-facing abstain
    pub fn abstain_pub<T>(&self, class: &str, reason: impl Into<String>) -> Result<T, Flow> {
        self.abstain(class, reason)
    }

    /// ADR-0006: is a value in callback position fallible? Closures carry the
    /// syntactic bit; named fns and methods read their declared channel.
    pub fn next_rand(&mut self) -> u64 {
        self.rand_state = self.rand_state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.rand_state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    pub fn cb_fallible(&self, c: &Callable) -> bool {
        match c {
            Callable::Closure(cl) => cl.fallible && !self.in_test,
            Callable::Named(n) => self
                .fns
                .get(n)
                .map(|f| {
                    f.sig.effect
                        || Self::ret_is_result(&f.sig.ret)
                        || matches!(f.sig.ret, TypeExpr::Fallible(..))
                })
                .unwrap_or(false),
            Callable::Method(t, n) => self
                .methods
                .get(&(t.clone(), n.clone()))
                .map(|f| {
                    f.sig.effect
                        || Self::ret_is_result(&f.sig.ret)
                        || matches!(f.sig.ret, TypeExpr::Fallible(..))
                })
                .unwrap_or(false),
            Callable::Std(_) | Callable::Ctor(..) => false,
            Callable::Codec(_, decode) => *decode,
            Callable::EffectWrap(_) => true,
            Callable::Composed(_, g) => self.cb_fallible(g),
        }
    }

    fn tick(&mut self) -> Result<(), Flow> {
        if self.fuel == 0 {
            return self.abstain("resource:fuel", "evaluation fuel exhausted — the program runs longer than the reference evaluator's budget");
        }
        self.fuel -= 1;
        Ok(())
    }

    // ── calls ────────────────────────────────────────────────────────────

    fn ret_is_result(ret: &TypeExpr) -> bool {
        matches!(ret, TypeExpr::Named { name, .. } if name == "Result")
    }

    fn ret_is_option(ret: &TypeExpr) -> bool {
        matches!(ret, TypeExpr::Option(_))
            || matches!(ret, TypeExpr::Named { name, .. } if name == "Option")
    }

    fn bind_params(
        &mut self,
        f: &Rc<FnDecl>,
        args: Vec<Value>,
        named: &[(String, Value)],
    ) -> Result<Rc<Env>, Flow> {
        let env = Env::new(Some(self.globals.clone()));
        let params = &f.sig.params;
        if args.len() > params.len() {
            return Err(Flow::Fatal(format!(
                "{}: too many arguments ({} > {})",
                f.sig.name,
                args.len(),
                params.len()
            )));
        }
        let mut args_iter = args.into_iter();
        for p in params {
            let v = match args_iter.next() {
                Some(v) => v,
                None => match named.iter().find(|(n, _)| *n == p.name) {
                    Some((_, v)) => v.clone(),
                    None => match &p.default {
                        Some(d) => self.eval(&env, d)?,
                        None => {
                            return Err(Flow::Fatal(format!(
                                "{}: missing argument `{}`",
                                f.sig.name, p.name
                            )))
                        }
                    },
                },
            };
            if p.mutable {
                return self.abstain(
                    "semantics:mut-param",
                    format!("`{}` takes `mut {}` — ALS-M13 in-place write-back through the caller's slot is not modeled yet", f.sig.name, p.name),
                );
            }
            let v = self.retag(v, p.ty.as_ref());
            env.define(&p.name, v);
        }
        Ok(env)
    }

    pub fn call_fn(&mut self, f: &Rc<FnDecl>, args: Vec<Value>, named: Vec<(String, Value)>) -> R {
        let mut env = self.bind_params(f, args, &named)?;
        let body = match &f.body {
            Some(b) => b,
            None => {
                return self.abstain(
                    "runtime:extern-fn",
                    format!("`{}` has no body (extern)", f.sig.name),
                )
            }
        };
        // trampoline: a tail call to this fn (or to another fn of the same
        // channel class) rebinds the parameters and loops — O(1) stack
        let mut f: Rc<FnDecl> = f.clone();
        let mut body = body;
        let r = loop {
            match self.eval_tail(&env, body, &f) {
                Ok(Tail::Value(v)) => break Ok(v),
                Ok(Tail::SelfCall(args2)) => {
                    env = self.bind_params(&f, args2, &[])?;
                }
                Ok(Tail::Call(g, args2)) => {
                    env = self.bind_params(&g, args2, &[])?;
                    f = g;
                    body = match &f.body {
                        Some(b) => b,
                        None => {
                            return self.abstain(
                                "runtime:extern-fn",
                                format!("`{}` has no body (extern)", f.sig.name),
                            )
                        }
                    };
                }
                Err(e) => break Err(e),
            }
        };
        let f = &f;
        let fallible = f.sig.effect
            || Self::ret_is_result(&f.sig.ret)
            || matches!(f.sig.ret, TypeExpr::Fallible(..));
        match r {
            Ok(v) => {
                let v = self.retag(v, Some(&f.sig.ret));
                if Self::ret_is_result(&f.sig.ret) {
                    Ok(v)
                } else if matches!(f.sig.ret, TypeExpr::Fallible(..)) {
                    Ok(match v {
                        Value::Ok(_) | Value::Err(_) => v,
                        other => Value::Ok(Rc::new(other)),
                    })
                } else if f.sig.effect {
                    // effect-system.md §3: the body may yield the unwrapped T (lifted to
                    // ok(T)) or a full Result (passed through as-is)
                    Ok(match v {
                        Value::Ok(_) | Value::Err(_) => v,
                        other => Value::Ok(Rc::new(other)),
                    })
                } else {
                    Ok(v)
                }
            }
            Err(Flow::Propagate(p)) => {
                if fallible {
                    Ok(Value::Err(Rc::new(p.as_err())))
                } else if Self::ret_is_option(&f.sig.ret) {
                    // C-211: a pure fn whose return resolves to Option — `!`
                    // on none propagates none through the Option channel
                    match p {
                        Prop::None => Ok(Value::None),
                        Prop::Err(e) => Err(Flow::Fatal(format!(
                            "an err propagated into the Option channel of `{}`: {e:?}",
                            f.sig.name
                        ))),
                    }
                } else {
                    Err(Flow::Fatal(format!(
                        "`!` propagated out of the total pure fn `{}`",
                        f.sig.name
                    )))
                }
            }
            Err(other) => Err(other),
        }
    }

    /// Evaluate `e` in tail position of fn `me`: a direct self call (or `self
    /// call !`, whose unwrap-then-rewrap is the identity for the same fn) is
    /// returned as `Tail::SelfCall` instead of recursing.
    fn channel_class(f: &FnDecl) -> (bool, bool, bool) {
        (
            f.sig.effect,
            Self::ret_is_result(&f.sig.ret),
            matches!(f.sig.ret, TypeExpr::Fallible(..)),
        )
    }

    fn eval_tail(&mut self, env: &Rc<Env>, e: &Expr, me: &Rc<FnDecl>) -> Result<Tail, Flow> {
        // `g(args)` or `g(args)!` in tail position: a self call, or a call to
        // another top-level fn of the same channel class (bare for total
        // fns, `!`-tail for effect/fallible fns), is trampolined
        let (direct, unwrapped) = match e {
            Expr::Call { callee, args, .. } => (Some((callee, args)), false),
            Expr::Unwrap(inner) => match &**inner {
                Expr::Call { callee, args, .. } => (Some((callee, args)), true),
                _ => (None, false),
            },
            _ => (None, false),
        };
        if let Some((callee, args)) = direct {
            if let Expr::Ident(n) = &**callee {
                if env.lookup(n).is_none() {
                    if let Some(g) = self.fns.get(n).cloned() {
                        let me_class = Self::channel_class(me);
                        let g_class = Self::channel_class(&g);
                        // a total fn's tail call is bare; an effect/fallible fn's tail may be
                        // bare (Result passed through) or `!`-unwrapped (then re-lifted) —
                        // both are the identity between same-class fns
                        let total = me_class == (false, false, false);
                        let ok_shape = !total || !unwrapped;
                        if me_class == g_class && ok_shape && g.sig.params.len() == args.len() {
                            let (pos, named) = self.eval_args(env, args)?;
                            if named.is_empty() {
                                return Ok(if Rc::ptr_eq(&g, me) {
                                    Tail::SelfCall(pos)
                                } else {
                                    Tail::Call(g, pos)
                                });
                            }
                        }
                    }
                }
            }
            return Ok(Tail::Value(self.eval(env, e)?));
        }
        match e {
            Expr::Paren(inner) => self.eval_tail(env, inner, me),
            Expr::If { cond, then, els } => match self.eval(env, cond)? {
                Value::Bool(true) => self.eval_tail(env, then, me),
                Value::Bool(false) => match els {
                    Some(e2) => self.eval_tail(env, e2, me),
                    None => Ok(Tail::Value(Value::Unit)),
                },
                other => Err(Flow::Fatal(format!(
                    "if condition is a {}",
                    other.type_name()
                ))),
            },
            Expr::Block(stmts) => {
                let inner = Env::new(Some(env.clone()));
                if let Some((Stmt::Expr(last, _), init)) = stmts.split_last() {
                    self.exec_block(&inner, init)?;
                    return self.eval_tail(&inner, last, me);
                }
                Ok(Tail::Value(self.exec_block(&inner, stmts)?))
            }
            Expr::Match { subject, arms } => {
                let v = self.eval(env, subject)?;
                for arm in arms {
                    let inner = Env::new(Some(env.clone()));
                    if self.matches(&arm.pat, &v, &inner)? {
                        if let Some(g) = &arm.guard {
                            match self.eval(&inner, g)? {
                                Value::Bool(true) => {}
                                Value::Bool(false) => continue,
                                other => {
                                    return Err(Flow::Fatal(format!(
                                        "match guard is a {}",
                                        other.type_name()
                                    )))
                                }
                            }
                        }
                        return self.eval_tail(&inner, &arm.body, me);
                    }
                }
                Err(Flow::Fatal(
                    "no match arm matched (non-exhaustive match reached at run time)".into(),
                ))
            }
            _ => Ok(Tail::Value(self.eval(env, e)?)),
        }
    }

    /// ALS-E23: a record's type comes from the declaration or the CONTEXT —
    /// an anonymous literal bound at a slot annotated with a named record type
    /// becomes that record (declaration order, defaults filled).
    fn retag(&mut self, v: Value, ty: Option<&TypeExpr>) -> Value {
        // ALS-M15: a callable in an `effect (A) -> B` position takes the
        // carrier shape `(A) -> Result[B, String]` — one form for pure and
        // fallible spellings alike
        if let (Value::Fn(c), Some(TypeExpr::Fn { effect: true, .. })) = (&v, ty) {
            if matches!(**c, Callable::EffectWrap(_)) {
                return v;
            }
            return Value::Fn(Rc::new(Callable::EffectWrap(c.clone())));
        }
        let name = match ty {
            Some(TypeExpr::Named { name, .. }) => name,
            _ => return v,
        };
        match &v {
            Value::Record {
                type_name: None,
                fields,
            } => {
                if let Some(decl) = self.types.get(name).cloned() {
                    if let TypeBody::Record(fdecls) = &decl.body {
                        let mut ordered: Vec<(Rc<str>, Value)> = Vec::with_capacity(fdecls.len());
                        for fd in fdecls {
                            if let Some((k, fv)) =
                                fields.iter().find(|(k, _)| &**k == fd.name.as_str())
                            {
                                ordered.push((k.clone(), fv.clone()));
                            } else if let Some(d) = &fd.default {
                                let g = self.globals.clone();
                                if let Ok(dv) = self.eval(&g, d) {
                                    ordered.push((Rc::from(fd.name.as_str()), dv));
                                } else {
                                    return v;
                                }
                            } else {
                                return v;
                            }
                        }
                        return Value::Record {
                            type_name: Some(Rc::from(name.as_str())),
                            fields: Rc::new(ordered),
                        };
                    }
                }
                v
            }
            _ => v,
        }
    }

    fn call_closure(&mut self, c: &Rc<Closure>, args: Vec<Value>) -> R {
        let env = Env::new(Some(c.env.clone()));
        if args.len() != c.params.len() {
            return Err(Flow::Fatal(format!(
                "lambda arity: expected {}, got {}",
                c.params.len(),
                args.len()
            )));
        }
        for (p, v) in c.params.iter().zip(args) {
            if p.names.len() == 1 {
                env.define(&p.names[0], v);
            } else {
                match v {
                    Value::Tuple(items) if items.len() == p.names.len() => {
                        for (n, x) in p.names.iter().zip(items.iter()) {
                            env.define(n, x.clone());
                        }
                    }
                    other => {
                        return Err(Flow::Fatal(format!(
                            "tuple-destructuring lambda parameter got {}",
                            other.type_name()
                        )))
                    }
                }
            }
        }
        let r = self.eval(&env, &c.body);
        match r {
            Ok(v) => {
                if c.fallible && !self.in_test {
                    Ok(match v {
                        Value::Ok(_) | Value::Err(_) => v,
                        other => Value::Ok(Rc::new(other)),
                    })
                } else {
                    Ok(v)
                }
            }
            Err(Flow::Propagate(p)) => {
                if c.fallible && !self.in_test {
                    // L4: an Option operand's none reifies as err("none")
                    Ok(Value::Err(Rc::new(p.as_err())))
                } else {
                    Err(Flow::Fatal(
                        "`!` propagated out of a non-fallible lambda".into(),
                    ))
                }
            }
            Err(other) => Err(other),
        }
    }

    pub fn call_value(&mut self, callee: &Callable, args: Vec<Value>) -> R {
        match callee {
            Callable::Named(n) => {
                let f = self
                    .fns
                    .get(n)
                    .cloned()
                    .ok_or_else(|| Flow::Fatal(format!("unknown fn `{n}`")))?;
                self.call_fn(&f, args, Vec::new())
            }
            Callable::Method(t, n) => {
                let f = self
                    .methods
                    .get(&(t.clone(), n.clone()))
                    .cloned()
                    .ok_or_else(|| Flow::Fatal(format!("unknown method `{t}.{n}`")))?;
                self.call_fn(&f, args, Vec::new())
            }
            Callable::Std(n) => stdlib::call(self, n, args),
            Callable::Closure(c) => self.call_closure(c, args),
            Callable::Composed(f, g) => {
                let mid = self.call_value(f, args)?;
                self.call_value(g, vec![mid])
            }
            Callable::Ctor(t, case) => Ok(Value::Variant {
                type_name: Rc::from(t.as_str()),
                case: Rc::from(case.as_str()),
                payload: Payload::Tuple(Rc::new(args)),
            }),
            Callable::EffectWrap(inner) => {
                let inner = inner.clone();
                match self.call_value(&inner, args) {
                    Ok(v @ (Value::Ok(_) | Value::Err(_))) => Ok(v),
                    Ok(other) => Ok(Value::Ok(Rc::new(other))),
                    Err(Flow::Propagate(p)) => Ok(Value::Err(Rc::new(p.as_err()))),
                    Err(e) => Err(e),
                }
            }
            Callable::Codec(t, decode) => {
                let t = t.clone();
                let decode = *decode;
                if args.len() != 1 {
                    return Err(Flow::Fatal(format!(
                        "`{t}.{}` takes 1 argument, got {}",
                        if decode { "decode" } else { "encode" },
                        args.len()
                    )));
                }
                let arg = self.force(args.into_iter().next().unwrap())?;
                if decode {
                    let Value::Dyn(d) = &arg else {
                        return self.abstain(
                            "semantics:codec",
                            format!("`{t}.decode` on a {}", arg.type_name()),
                        );
                    };
                    match self.codec_decode(&t, &d.clone())? {
                        Ok(v) => Ok(Value::Ok(Rc::new(v))),
                        Err(e) => Ok(Value::Err(Rc::new(Value::str(&e)))),
                    }
                } else {
                    Ok(Value::Dyn(self.codec_encode(&t, &arg)?))
                }
            }
        }
    }

    fn module_of(v: &Value) -> Option<&'static str> {
        Some(match v {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "string",
            Value::Bool(_) => "bool",
            Value::List(_) => "list",
            Value::Map(_) => "map",
            Value::Set(_) => "set",
            Value::Tuple(_) => "tuple",
            Value::Some(_) | Value::None => "option",
            Value::Ok(_) | Value::Err(_) => "result",
            Value::Range(..) => "list",
            Value::Dyn(_) => "value",
            Value::Bytes(_) => "bytes",
            Value::Path(_) => "json",
            Value::Matrix(_) => "matrix",
            Value::Unit | Value::Record { .. } | Value::Variant { .. } | Value::Fn(_) => {
                return None
            }
        })
    }

    fn value_type_name(v: &Value) -> Option<String> {
        match v {
            Value::Record {
                type_name: Some(t), ..
            } => Some(t.to_string()),
            Value::Variant { type_name, .. } => Some(type_name.to_string()),
            _ => None,
        }
    }

    fn eval_args(&mut self, env: &Rc<Env>, args: &[Arg]) -> Result<Args, Flow> {
        let mut pos = Vec::new();
        let mut named = Vec::new();
        for a in args {
            match a {
                Arg::Pos(e) => {
                    let v = self.eval(env, e)?;
                    pos.push(self.force(v)?)
                }
                Arg::Named(n, e) => {
                    let v = self.eval(env, e)?;
                    named.push((n.clone(), self.force(v)?))
                }
                Arg::Placeholder => {
                    return Err(Flow::Fatal(
                        "`_` placeholder argument reached evaluation (E046 is a check-time error)"
                            .into(),
                    ))
                }
            }
        }
        Ok((pos, named))
    }

    /// `list.push(xs, v)` etc.: the first argument is a PLACE (a `var`); the
    /// mutator computes the new collection and it is written back (value
    /// semantics — no sharing is observable); the call yields Unit.
    fn call_in_place(&mut self, env: &Rc<Env>, full: &str, args: &[Arg]) -> R {
        let place = match args.first() {
            Some(Arg::Pos(p)) => p.clone(),
            _ => {
                return Err(Flow::Fatal(format!(
                    "{full}: first argument must be a place"
                )))
            }
        };
        // Fast path: a plain `var` — take the collection out of its slot so the
        // Rc is unique and the mutation is O(1) amortized (the value semantics
        // are unchanged: nothing else can observe the slot meanwhile).
        if let Expr::Ident(name) = &place {
            if let Some(cur) = env.lookup(name) {
                if let Value::List(_) = cur {
                    let mut rest = Vec::new();
                    for a in &args[1..] {
                        match a {
                            Arg::Pos(e) => rest.push(self.eval(env, e)?),
                            _ => {
                                return self.abstain(
                                    "syntax:named-arg-std",
                                    "named arguments to a stdlib mutator",
                                )
                            }
                        }
                    }
                    env.assign(name, Value::Unit);
                    let mut list = match cur {
                        Value::List(rc) => rc,
                        _ => unreachable!(),
                    };
                    let mut ret = Value::Unit;
                    {
                        let v = Rc::make_mut(&mut list);
                        match full {
                            "list.push" => {
                                if rest.len() != 1 {
                                    return Err(Flow::Fatal(
                                        "list.push: expected 2 arguments".into(),
                                    ));
                                }
                                v.push(rest.pop().unwrap());
                            }
                            "list.pop" => {
                                // pop mutates AND answers the element:
                                // some(last), none on empty (list_comprehensive)
                                ret = match v.pop() {
                                    Some(x) => Value::Some(Rc::new(x)),
                                    None => Value::None,
                                };
                            }
                            "list.clear" => v.clear(),
                            _ => {
                                return Err(Flow::Fatal(format!(
                                    "{full}: not an in-place list mutator"
                                )))
                            }
                        }
                    }
                    env.assign(name, Value::List(list));
                    return Ok(ret);
                }
            }
        }
        let (pos, named) = self.eval_args(env, args)?;
        if !named.is_empty() {
            return self.abstain(
                "syntax:named-arg-std",
                "named arguments to a stdlib mutator",
            );
        }
        let updated = stdlib::call(self, full, pos)?;
        self.assign_place(env, &place, updated)?;
        Ok(Value::Unit)
    }

    /// Materialize a lazy range when a list is demanded. C-197: a span no
    /// 64-bit machine can hold aborts `Error: out of memory`; a span the judge
    /// should not attempt (but a machine could) abstains.
    pub fn force(&mut self, v: Value) -> R {
        match v {
            Value::Range(a, b) => {
                let n = Value::range_len(a, b);
                if n.saturating_mul(8) > isize::MAX as u128 {
                    return Err(Flow::Abort("out of memory".into()));
                }
                if n > 50_000_000 {
                    return self.abstain(
                        "resource:materialize-huge",
                        format!("materializing a {n}-element range"),
                    );
                }
                let mut out = Vec::with_capacity(n as usize);
                let mut i = a;
                while i < b {
                    out.push(Value::Int(i));
                    i += 1;
                }
                Ok(Value::List(Rc::new(out)))
            }
            other => Ok(other),
        }
    }

    fn is_module_ref(&self, env: &Rc<Env>, name: &str) -> bool {
        STD_MODULES.contains(&name) && env.lookup(name).is_none() && !self.fns.contains_key(name)
    }

    fn eval_call(&mut self, env: &Rc<Env>, callee: &Expr, args: &[Arg]) -> R {
        match callee {
            Expr::Ident(name) => {
                if let Some(v) = env.lookup(name) {
                    let (pos, named) = self.eval_args(env, args)?;
                    if !named.is_empty() {
                        return self.abstain(
                            "syntax:named-arg-on-value",
                            "named arguments on a function value",
                        );
                    }
                    return match v {
                        Value::Fn(c) => self.call_value(&c, pos),
                        other => Err(Flow::Fatal(format!(
                            "calling a non-function `{name}` ({})",
                            other.type_name()
                        ))),
                    };
                }
                if let Some(f) = self.fns.get(name).cloned() {
                    let (pos, named) = self.eval_args(env, args)?;
                    return self.call_fn(&f, pos, named);
                }
                if PRELUDE.contains(&name.as_str()) {
                    let (pos, named) = self.eval_args(env, args)?;
                    if !named.is_empty() {
                        return self.abstain(
                            "syntax:named-arg-std",
                            "named arguments to a prelude function",
                        );
                    }
                    return stdlib::call(self, name, pos);
                }
                Err(Flow::Fatal(format!("unbound function `{name}`")))
            }
            Expr::Member { obj, name } => {
                // module function: string.len(s)
                if let Expr::Ident(m) = &**obj {
                    if self.is_module_ref(env, m) {
                        let full = format!("{m}.{name}");
                        if IN_PLACE.contains(&full.as_str()) {
                            return self.call_in_place(env, &full, args);
                        }
                        let (pos, named) = self.eval_args(env, args)?;
                        if !named.is_empty() {
                            return self.abstain(
                                "syntax:named-arg-std",
                                "named arguments to a stdlib function",
                            );
                        }
                        return stdlib::call(self, &full, pos);
                    }
                }
                // UFCS form of an in-place mutator: xs.push(v)
                if let Some(m) = match &**obj {
                    Expr::Ident(v) => env.lookup(v).as_ref().and_then(Self::module_of),
                    _ => None,
                } {
                    let full = format!("{m}.{name}");
                    if IN_PLACE.contains(&full.as_str()) {
                        let mut full_args: Vec<Arg> = Vec::with_capacity(args.len() + 1);
                        full_args.push(Arg::Pos((**obj).clone()));
                        full_args.extend(args.iter().cloned());
                        return self.call_in_place(env, &full, &full_args);
                    }
                }
                // static method: Type.method(args)
                if let Expr::TypeName {
                    module: None,
                    name: t,
                } = &**obj
                {
                    if let Some(f) = self.methods.get(&(t.clone(), name.clone())).cloned() {
                        let (pos, named) = self.eval_args(env, args)?;
                        return self.call_fn(&f, pos, named);
                    }
                    // ALS-D6: `T.decode` / `T.encode` derived from `: Codec`
                    if (name == "decode" || name == "encode")
                        && self
                            .types
                            .get(t)
                            .is_some_and(|d| d.conventions.iter().any(|c| c == "Codec"))
                    {
                        let c = Callable::Codec(t.clone(), name == "decode");
                        let (pos, named) = self.eval_args(env, args)?;
                        if !named.is_empty() {
                            return self.abstain(
                                "syntax:named-arg-on-value",
                                "named arguments to a Codec entry",
                            );
                        }
                        return self.call_value(&c, pos);
                    }
                }
                // value receiver: field-fn, convention method, or UFCS stdlib
                let recv = self.eval(env, obj)?;
                if let Value::Record { fields, .. } = &recv {
                    if let Some((_, Value::Fn(c))) =
                        fields.iter().find(|(n, _)| &**n == name.as_str())
                    {
                        let c = c.clone();
                        let (pos, named) = self.eval_args(env, args)?;
                        if !named.is_empty() {
                            return self.abstain(
                                "syntax:named-arg-on-value",
                                "named arguments on a field function",
                            );
                        }
                        return self.call_value(&c, pos);
                    }
                }
                if let Some(t) = Self::value_type_name(&recv) {
                    if let Some(f) = self.methods.get(&(t.clone(), name.clone())).cloned() {
                        let (mut pos, named) = self.eval_args(env, args)?;
                        pos.insert(0, recv);
                        return self.call_fn(&f, pos, named);
                    }
                }
                match Self::module_of(&recv) {
                    Some(m) => {
                        let (mut pos, named) = self.eval_args(env, args)?;
                        if !named.is_empty() {
                            return self.abstain(
                                "syntax:named-arg-std",
                                "named arguments to a UFCS stdlib call",
                            );
                        }
                        pos.insert(0, recv);
                        stdlib::call(self, &format!("{m}.{name}"), pos)
                    }
                    None => self.abstain(
                        "semantics:ufcs-receiver",
                        format!("UFCS call `.{name}()` on a {}", recv.type_name()),
                    ),
                }
            }
            Expr::TypeName { module, name } => {
                let (pos, named) = self.eval_args(env, args)?;
                if !named.is_empty() {
                    return self
                        .abstain("syntax:named-arg-ctor", "named arguments to a constructor");
                }
                let _ = module;
                match self.ctors.get(name).cloned() {
                    Some((t, CaseShape::Tuple(n))) if n == pos.len() => Ok(Value::Variant {
                        type_name: Rc::from(t.as_str()),
                        case: Rc::from(name.as_str()),
                        payload: Payload::Tuple(Rc::new(pos)),
                    }),
                    Some(_) => Err(Flow::Fatal(format!(
                        "constructor `{name}` called with the wrong shape"
                    ))),
                    None => {
                        self.abstain("semantics:ctor-call", format!("call of type name `{name}`"))
                    }
                }
            }
            other => {
                let f = self.eval(env, other)?;
                let (pos, named) = self.eval_args(env, args)?;
                if !named.is_empty() {
                    return self.abstain(
                        "syntax:named-arg-on-value",
                        "named arguments on a function value",
                    );
                }
                match f {
                    Value::Fn(c) => self.call_value(&c, pos),
                    other => Err(Flow::Fatal(format!(
                        "calling a non-function ({})",
                        other.type_name()
                    ))),
                }
            }
        }
    }

    // ── expressions ──────────────────────────────────────────────────────

    pub fn eval(&mut self, env: &Rc<Env>, e: &Expr) -> R {
        self.tick()?;
        match e {
            Expr::Int(n) => Ok(Value::Int(*n)),
            Expr::BigInt(_) => self.abstain("semantics:uint64-upper-half", "a literal above i64::MAX (UInt64 upper half, C-179) — sized integers are not implemented yet"),
            Expr::Float(f) => Ok(Value::Float(*f)),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Unit => Ok(Value::Unit),
            Expr::Str(segs) => {
                let mut out = String::new();
                for s in segs {
                    match s {
                        StrSeg::Text(t) => out.push_str(t),
                        StrSeg::Expr(x) => {
                            let v = self.eval(env, x)?;
                            let v = self.force(v)?;
                            match render(&v) {
                                Some(s) => out.push_str(&s),
                                None => return self.abstain(&format!("render:{}", v.type_name()), format!("interpolating a {} is not implemented yet", v.type_name())),
                            }
                        }
                    }
                }
                Ok(Value::str(&out))
            }
            Expr::Ident(name) => {
                if let Some(v) = env.lookup(name) {
                    return Ok(v);
                }
                if self.fns.contains_key(name) {
                    return Ok(Value::Fn(Rc::new(Callable::Named(name.clone()))));
                }
                if PRELUDE.contains(&name.as_str()) {
                    return Ok(Value::Fn(Rc::new(Callable::Std(name.clone()))));
                }
                Err(Flow::Fatal(format!("unbound identifier `{name}`")))
            }
            Expr::TypeName { module: _, name } => match self.ctors.get(name).cloned() {
                _ if env.lookup(name).is_some() => Ok(env.lookup(name).unwrap()),
                Some((t, CaseShape::Unit)) => Ok(Value::Variant { type_name: Rc::from(t.as_str()), case: Rc::from(name.as_str()), payload: Payload::Unit }),
                Some((t, CaseShape::Tuple(_))) => Ok(Value::Fn(Rc::new(Callable::Ctor(t, name.clone())))),
                Some((_, CaseShape::Record(_))) => self.abstain("semantics:record-ctor-as-value", format!("record-payload constructor `{name}` used as a value")),
                None => self.abstain("semantics:type-name-value", format!("type name `{name}` in value position")),
            },
            Expr::List(items) => {
                let mut out = Vec::with_capacity(items.len());
                for x in items {
                    out.push(self.eval(env, x)?);
                }
                Ok(Value::List(Rc::new(out)))
            }
            Expr::Map(pairs) => {
                let mut out: Vec<(Value, Value)> = Vec::with_capacity(pairs.len());
                for (k, v) in pairs {
                    let kv = self.eval(env, k)?;
                    let vv = self.eval(env, v)?;
                    // ALS-ST4 upsert semantics also for duplicate literal keys
                    if let Some(slot) = out.iter_mut().find(|(k2, _)| values_eq(k2, &kv) == Some(true)) {
                        slot.1 = vv;
                    } else {
                        out.push((kv, vv));
                    }
                }
                Ok(Value::Map(Rc::new(out)))
            }
            Expr::EmptyMap => Ok(Value::Map(Rc::new(Vec::new()))),
            Expr::Tuple(items) => {
                let mut out = Vec::with_capacity(items.len());
                for x in items {
                    out.push(self.eval(env, x)?);
                }
                Ok(Value::Tuple(Rc::new(out)))
            }
            Expr::Record { module: _, type_name, spread, fields } => {
                let mut out: Vec<(Rc<str>, Value)> = Vec::new();
                if let Some(sp) = spread {
                    match self.eval(env, sp)? {
                        Value::Record { fields: base, .. } => out = (*base).clone(),
                        other => return Err(Flow::Fatal(format!("spread of a non-record ({})", other.type_name()))),
                    }
                }
                for (n, x) in fields {
                    let v = self.eval(env, x)?;
                    if let Some(slot) = out.iter_mut().find(|(k, _)| &**k == n.as_str()) {
                        slot.1 = v;
                    } else {
                        out.push((Rc::from(n.as_str()), v));
                    }
                }
                match type_name {
                    Some(t) if self.ctors.contains_key(t) => {
                        // record-payload variant constructor `Dot { x: 5 }` — payload in
                        // declaration order, omitted defaulted fields evaluated here
                        let (tn, shape) = self.ctors.get(t).cloned().unwrap();
                        let fdecls = match shape {
                            CaseShape::Record(f) => f,
                            _ => return Err(Flow::Fatal(format!("`{t}` is not a record-payload constructor"))),
                        };
                        let mut ordered: Vec<(Rc<str>, Value)> = Vec::with_capacity(fdecls.len());
                        for fd in &fdecls {
                            match out.iter().find(|(k, _)| &**k == fd.name.as_str()) {
                                Some((k, v)) => ordered.push((k.clone(), v.clone())),
                                None => match &fd.default {
                                    Some(d) => {
                                        let v = self.eval(env, d)?;
                                        ordered.push((Rc::from(fd.name.as_str()), v));
                                    }
                                    None => return Err(Flow::Fatal(format!("constructor `{t}` missing field `{}`", fd.name))),
                                },
                            }
                        }
                        Ok(Value::Variant { type_name: Rc::from(tn.as_str()), case: Rc::from(t.as_str()), payload: Payload::Record(Rc::new(ordered)) })
                    }
                    Some(t) => {
                        // named record: declaration order (ALS-R2), defaults filled
                        if let Some(decl) = self.types.get(t).cloned() {
                            if let TypeBody::Record(fdecls) = &decl.body {
                                let mut ordered: Vec<(Rc<str>, Value)> = Vec::with_capacity(fdecls.len());
                                for fd in fdecls {
                                    match out.iter().find(|(k, _)| &**k == fd.name.as_str()) {
                                        Some((k, v)) => ordered.push((k.clone(), v.clone())),
                                        None => match &fd.default {
                                            Some(d) => {
                                                let v = self.eval(env, d)?;
                                                ordered.push((Rc::from(fd.name.as_str()), v));
                                            }
                                            None => return Err(Flow::Fatal(format!("record `{t}` missing field `{}`", fd.name))),
                                        },
                                    }
                                }
                                out = ordered;
                            }
                        }
                        Ok(Value::Record { type_name: Some(Rc::from(t.as_str())), fields: Rc::new(out) })
                    }
                    None => {
                        // the checker infers a nominal type for an anonymous literal
                        // whose field set matches exactly one declared record type
                        // (r5_wasm_inferred_record_repr); mirror that when unambiguous
                        let names: Vec<&str> = out.iter().map(|(k, _)| &**k).collect();
                        let mut hits: Vec<String> = Vec::new();
                        for (tname, decl) in self.types.iter() {
                            if let TypeBody::Record(fds) = &decl.body {
                                if fds.len() == names.len() && fds.iter().all(|fd| names.contains(&fd.name.as_str())) {
                                    hits.push(tname.clone());
                                }
                            }
                        }
                        if hits.len() == 1 {
                            let t = hits.pop().unwrap();
                            let decl = self.types.get(&t).cloned().unwrap();
                            if let TypeBody::Record(fds) = &decl.body {
                                let mut ordered: Vec<(Rc<str>, Value)> = Vec::with_capacity(fds.len());
                                for fd in fds {
                                    let (k, v) = out.iter().find(|(k, _)| &**k == fd.name.as_str()).cloned().unwrap();
                                    ordered.push((k, v));
                                }
                                return Ok(Value::Record {
                                    type_name: Some(Rc::from(t.as_str())),
                                    fields: Rc::new(ordered),
                                });
                            }
                        }
                        // anonymous: field-name order (ALS-R2) — a stable insertion sort
                        let mut sorted: Vec<(Rc<str>, Value)> = Vec::with_capacity(out.len());
                        for item in out {
                            let pos = sorted.iter().position(|(k, _)| char_cmp(k, &item.0) == std::cmp::Ordering::Greater).unwrap_or(sorted.len());
                            sorted.insert(pos, item);
                        }
                        Ok(Value::Record { type_name: None, fields: Rc::new(sorted) })
                    }
                }
            }
            Expr::Block(stmts) => {
                let inner = Env::new(Some(env.clone()));
                self.exec_block(&inner, stmts)
            }
            Expr::If { cond, then, els } => match self.eval(env, cond)? {
                Value::Bool(true) => self.eval(env, then),
                Value::Bool(false) => match els {
                    Some(e2) => self.eval(env, e2),
                    None => Ok(Value::Unit),
                },
                other => Err(Flow::Fatal(format!("if condition is a {}", other.type_name()))),
            },
            Expr::IfLet { name, scrut, then, els } => match self.eval(env, scrut)? {
                Value::Some(v) => {
                    let inner = Env::new(Some(env.clone()));
                    inner.define(name, (*v).clone());
                    self.eval(&inner, then)
                }
                Value::None => self.eval(env, els),
                other => Err(Flow::Fatal(format!("if-let scrutinee is a {}", other.type_name()))),
            },
            Expr::Match { subject, arms } => {
                let v = self.eval(env, subject)?;
                self.eval_match(env, v, arms)
            }
            Expr::PipeMatch { .. } => Err(Flow::Fatal("pipe-match outside a pipe".into())),
            Expr::For { binders, iter, body } => {
                if let Expr::Range { lo, hi, inclusive } = &**iter {
                    let l = self.eval(env, lo)?;
                    let h = self.eval(env, hi)?;
                    let (a, b) = match (l, h) {
                        (Value::Int(a), Value::Int(b)) => (a, b),
                        (a, b) => return Err(Flow::Fatal(format!("range over {} and {}", a.type_name(), b.type_name()))),
                    };
                    let end = if *inclusive { b } else { b - 1 };
                    let mut i = a;
                    while i <= end {
                        let inner = Env::new(Some(env.clone()));
                        if binders.len() != 1 {
                            return Err(Flow::Fatal("tuple destructuring over a range".into()));
                        }
                        if binders[0] != "_" {
                            inner.define(&binders[0], Value::Int(i));
                        }
                        match self.eval(&inner, body) {
                            Ok(_) => {}
                            Err(Flow::Break) => break,
                            Err(Flow::Continue) => {}
                            Err(f) => return Err(f),
                        }
                        i += 1;
                    }
                    return Ok(Value::Unit);
                }
                let it = self.eval(env, iter)?;
                if let Value::Range(a, b) = it {
                    let mut i = a;
                    while i < b {
                        let inner = Env::new(Some(env.clone()));
                        if binders.len() != 1 {
                            return Err(Flow::Fatal("tuple destructuring over a range".into()));
                        }
                        if binders[0] != "_" {
                            inner.define(&binders[0], Value::Int(i));
                        }
                        match self.eval(&inner, body) {
                            Ok(_) => {}
                            Err(Flow::Break) => break,
                            Err(Flow::Continue) => {}
                            Err(f) => return Err(f),
                        }
                        i += 1;
                    }
                    return Ok(Value::Unit);
                }
                let items: Vec<Value> = match it {
                    Value::List(xs) | Value::Set(xs) => (*xs).clone(),
                    Value::Map(kvs) => kvs.iter().map(|(k, v)| Value::Tuple(Rc::new(vec![k.clone(), v.clone()]))).collect(),
                    other => return self.abstain("semantics:for-iterable", format!("for-in over a {}", other.type_name())),
                };
                for item in items {
                    let inner = Env::new(Some(env.clone()));
                    if binders.len() == 1 {
                        if binders[0] != "_" {
                            inner.define(&binders[0], item);
                        }
                    } else {
                        match item {
                            Value::Tuple(parts) if parts.len() == binders.len() => {
                                for (b, p) in binders.iter().zip(parts.iter()) {
                                    if b != "_" {
                                        inner.define(b, p.clone());
                                    }
                                }
                            }
                            other => return Err(Flow::Fatal(format!("for-in destructuring of a {}", other.type_name()))),
                        }
                    }
                    match self.eval(&inner, body) {
                        Ok(_) => {}
                        Err(Flow::Break) => break,
                        Err(Flow::Continue) => continue,
                        Err(f) => return Err(f),
                    }
                }
                Ok(Value::Unit)
            }
            Expr::While { cond, body } => {
                loop {
                    match self.eval(env, cond)? {
                        Value::Bool(true) => {}
                        Value::Bool(false) => break,
                        other => return Err(Flow::Fatal(format!("while condition is a {}", other.type_name()))),
                    }
                    match self.eval(env, body) {
                        Ok(_) => {}
                        Err(Flow::Break) => break,
                        Err(Flow::Continue) => continue,
                        Err(f) => return Err(f),
                    }
                }
                Ok(Value::Unit)
            }
            Expr::Fan { head, head_args: _, arms } => {
                match head {
                    None => {
                        // ALS-R3: deterministic, list order; the all-ok path yields the
                        // tuple of unwrapped values. The err path's observable order is
                        // pinned by C-199 and not yet read into this evaluator.
                        let mut vals = Vec::new();
                        let mut first_err: Option<Value> = None;
                        for a in arms {
                            match self.eval(env, a)? {
                                Value::Ok(v) => vals.push((*v).clone()),
                                Value::Err(e) => {
                                    if first_err.is_none() {
                                        first_err = Some((*e).clone());
                                    }
                                }
                                other => vals.push(other),
                            }
                        }
                        if let Some(e) = first_err {
                            // C-199: the first err in arm order escalates like `!`
                            return Err(Flow::Propagate(crate::eval::Prop::Err(e)));
                        }
                        if vals.len() == 1 {
                            Ok(vals.pop().unwrap())
                        } else {
                            Ok(Value::Tuple(Rc::new(vals)))
                        }
                    }
                    Some(h) if h == "any" => {
                        // ALS-R3: list order, first Ok wins; plain thunk values
                        // auto-wrap (effect-system.md §5 Thunk typing)
                        for a in arms {
                            match self.eval(env, a)? {
                                Value::Ok(v) => return Ok(Value::Ok(v)),
                                Value::Err(_) => continue,
                                other => return Ok(Value::Ok(Rc::new(other))),
                            }
                        }
                        Ok(Value::Err(Rc::new(Value::str("fan.any: all candidates failed"))))
                    }
                    Some(h) if h == "settle" => {
                        // ALS-R3: a TUPLE of per-arm Results, list order
                        let mut out = Vec::with_capacity(arms.len());
                        for a in arms {
                            let v = self.eval(env, a)?;
                            out.push(match v {
                                v @ (Value::Ok(_) | Value::Err(_)) => v,
                                other => Value::Ok(Rc::new(other)),
                            });
                        }
                        if out.len() == 1 {
                            Ok(out.pop().unwrap())
                        } else {
                            Ok(Value::Tuple(Rc::new(out)))
                        }
                    }
                    Some(h) => self.abstain(&format!("syntax:fan.{h}"), format!("`fan.{h}` block head is not implemented yet")),
                }
            }
            Expr::Lambda { params, body } => {
                // value semantics (ALS-C5, capture_clone): the closure captures
                // COPIES of the bindings visible at creation — a later `var`
                // write in the enclosing scope is not observed by the closure
                let fallible = expr_has_unwrap(body);
                let snap = Env::new(Some(self.globals.clone()));
                snapshot_into(env, &snap, &self.globals);
                Ok(Value::Fn(Rc::new(Callable::Closure(Rc::new(Closure { params: params.clone(), body: (**body).clone(), env: snap, fallible })))))
            }
            Expr::Call { callee, type_args: _, args, line } => {
                self.cur_line = *line;
                self.eval_call(env, callee, args)
            }
            Expr::Index { obj, idx } => {
                let o = self.eval(env, obj)?;
                let i = self.eval(env, idx)?;
                let o = self.force(o)?;
                match (o, i) {
                    (Value::List(xs), Value::Int(i)) => {
                        if i < 0 || i as usize >= xs.len() {
                            Err(Flow::Abort("index out of bounds".into()))
                        } else {
                            Ok(xs[i as usize].clone())
                        }
                    }
                    (Value::Map(kvs), k) => Ok(match kvs.iter().find(|(k2, _)| values_eq(k2, &k) == Some(true)) {
                        Some((_, v)) => Value::Some(Rc::new(v.clone())),
                        None => Value::None,
                    }),
                    (o, i) => self.abstain("semantics:index", format!("indexing a {} with a {}", o.type_name(), i.type_name())),
                }
            }
            Expr::Member { obj, name } => {
                if let Expr::Ident(m) = &**obj {
                    if self.is_module_ref(env, m) {
                        return Ok(Value::Fn(Rc::new(Callable::Std(format!("{m}.{name}")))));
                    }
                }
                if let Expr::TypeName { module: None, name: t } = &**obj {
                    if self.methods.contains_key(&(t.clone(), name.clone())) {
                        return Ok(Value::Fn(Rc::new(Callable::Method(t.clone(), name.clone()))));
                    }
                    // ALS-D6: `T.decode` / `T.encode` derived from `: Codec`
                    if (name == "decode" || name == "encode")
                        && self.types.get(t).is_some_and(|d| d.conventions.iter().any(|c| c == "Codec"))
                    {
                        return Ok(Value::Fn(Rc::new(Callable::Codec(t.clone(), name == "decode"))));
                    }
                }
                let v = self.eval(env, obj)?;
                match &v {
                    Value::Record { fields, .. } => match fields.iter().find(|(n, _)| &**n == name.as_str()) {
                        Some((_, fv)) => Ok(fv.clone()),
                        None => Err(Flow::Fatal(format!("no field `{name}` on record"))),
                    },
                    Value::Variant { payload: Payload::Record(fields), .. } => match fields.iter().find(|(n, _)| &**n == name.as_str()) {
                        Some((_, fv)) => Ok(fv.clone()),
                        None => Err(Flow::Fatal(format!("no field `{name}` on variant payload"))),
                    },
                    other => self.abstain("semantics:member", format!("member `.{name}` on a {}", other.type_name())),
                }
            }
            Expr::TupleIndex { obj, k } => match self.eval(env, obj)? {
                Value::Tuple(items) => items.get(*k).cloned().ok_or_else(|| Flow::Fatal(format!("tuple index .{k} out of range"))),
                other => Err(Flow::Fatal(format!("tuple index on a {}", other.type_name()))),
            },
            Expr::Unwrap(inner) => match self.eval(env, inner)? {
                Value::Ok(v) | Value::Some(v) => Ok((*v).clone()),
                Value::Err(e) => {
                    if self.in_test {
                        Err(Flow::Abort(format!("unwrap on err: {}", render(&e).unwrap_or_default())))
                    } else {
                        Err(Flow::Propagate(Prop::Err((*e).clone())))
                    }
                }
                Value::None => {
                    if self.in_test {
                        Err(Flow::Abort("unwrap on none".into()))
                    } else {
                        Err(Flow::Propagate(Prop::None))
                    }
                }
                other => self.abstain(
                    "semantics:carrier-shape",
                    format!("`!` on a {} — the effect-slot carrier shape (ALS-M15) is not modeled yet", other.type_name()),
                ),
            },
            Expr::ToOption(inner) => match self.eval(env, inner)? {
                Value::Ok(v) => Ok(Value::Some(v)),
                Value::Err(_) => Ok(Value::None),
                v @ (Value::Some(_) | Value::None) => Ok(v),
                other => self.abstain(
                    "semantics:carrier-shape",
                    format!("`?` on a {} — the effect-slot carrier shape (ALS-M15) is not modeled yet", other.type_name()),
                ),
            },
            Expr::OptChain { obj, name } => match self.eval(env, obj)? {
                Value::Some(v) => match &*v {
                    Value::Record { fields, .. } => match fields.iter().find(|(n, _)| &**n == name.as_str()) {
                        Some((_, fv)) => Ok(Value::Some(Rc::new(fv.clone()))),
                        None => Err(Flow::Fatal(format!("no field `{name}` on record"))),
                    },
                    other => Err(Flow::Fatal(format!("`?.{name}` on some({})", other.type_name()))),
                },
                Value::None => Ok(Value::None),
                other => Err(Flow::Fatal(format!("`?.` on a {}", other.type_name()))),
            },
            Expr::UnwrapOr { expr, fallback } => match self.eval(env, expr)? {
                Value::Ok(v) | Value::Some(v) => Ok((*v).clone()),
                Value::Err(_) | Value::None => self.eval(env, fallback),
                other => self.abstain(
                    "semantics:carrier-shape",
                    format!("`??` on a {} — the effect-slot carrier shape (ALS-M15) is not modeled yet", other.type_name()),
                ),
            },
            Expr::Binary { op, lhs, rhs } => {
                if *op == BinOp::And {
                    return match self.eval(env, lhs)? {
                        Value::Bool(false) => Ok(Value::Bool(false)),
                        Value::Bool(true) => self.eval(env, rhs),
                        other => Err(Flow::Fatal(format!("`and` on a {}", other.type_name()))),
                    };
                }
                if *op == BinOp::Or {
                    return match self.eval(env, lhs)? {
                        Value::Bool(true) => Ok(Value::Bool(true)),
                        Value::Bool(false) => self.eval(env, rhs),
                        other => Err(Flow::Fatal(format!("`or` on a {}", other.type_name()))),
                    };
                }
                let l = self.eval(env, lhs)?;
                let l = self.force(l)?;
                let r = self.eval(env, rhs)?;
                let r = self.force(r)?;
                self.binop(*op, l, r)
            }
            Expr::Unary { op, expr } => {
                let v = self.eval(env, expr)?;
                match (op, v) {
                    (UnOp::Neg, Value::Int(n)) => Ok(Value::Int(n.wrapping_neg())),
                    (UnOp::Neg, Value::Float(f)) => Ok(Value::Float(F64(fnan(-f.0)))),
                    (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
                    (op, v) => Err(Flow::Fatal(format!("unary {op:?} on a {}", v.type_name()))),
                }
            }
            Expr::Pipe { lhs, rhs } => {
                let v = self.eval(env, lhs)?;
                match &**rhs {
                    Expr::Call { callee, type_args: _, args, line } => {
                        self.cur_line = *line;
                        let mut full: Vec<Arg> = Vec::with_capacity(args.len() + 1);
                        full.push(Arg::Pos(Expr::Ident("__pipe_value".into())));
                        full.extend(args.iter().cloned());
                        let inner = Env::new(Some(env.clone()));
                        inner.define("__pipe_value", v);
                        self.eval_call(&inner, callee, &full)
                    }
                    Expr::PipeMatch { arms } => self.eval_match(env, v, arms),
                    other => {
                        // a callable: `x |> f`, `x |> mod.f`, `x |> (f >> g)`
                        let inner = Env::new(Some(env.clone()));
                        inner.define("__pipe_value", v);
                        self.eval_call(&inner, other, &[Arg::Pos(Expr::Ident("__pipe_value".into()))])
                    }
                }
            }
            Expr::Compose { lhs, rhs } => {
                let f = self.callable_of(env, lhs)?;
                let g = self.callable_of(env, rhs)?;
                Ok(Value::Fn(Rc::new(Callable::Composed(f, g))))
            }
            Expr::Range { lo, hi, inclusive } => {
                let l = self.eval(env, lo)?;
                let h = self.eval(env, hi)?;
                match (l, h) {
                    (Value::Int(a), Value::Int(b)) => {
                        let end = if *inclusive { b.saturating_add(1) } else { b };
                        Ok(Value::Range(a, end))
                    }
                    (a, b) => Err(Flow::Fatal(format!("range over {} and {}", a.type_name(), b.type_name()))),
                }
            }
            Expr::Some(x) => Ok(Value::Some(Rc::new(self.eval(env, x)?))),
            Expr::None => Ok(Value::None),
            Expr::Ok(x) => Ok(Value::Ok(Rc::new(self.eval(env, x)?))),
            Expr::Err(x) => Ok(Value::Err(Rc::new(self.eval(env, x)?))),
            Expr::Todo(msg) => self.abstain("runtime:todo", format!("todo({msg:?}) reached — ALS-E30: no cross-target contract")),
            Expr::Hole => self.abstain("runtime:hole", "typed hole reached — ALS-E30: no cross-target contract"),
            Expr::Break => Err(Flow::Break),
            Expr::Continue => Err(Flow::Continue),
            Expr::Ascription { expr, ty: _ } => self.eval(env, expr),
            Expr::Paren(inner) => self.eval(env, inner),
        }
    }

    fn callable_of(&mut self, env: &Rc<Env>, e: &Expr) -> Result<Rc<Callable>, Flow> {
        match self.eval(env, e)? {
            Value::Fn(c) => Ok(c),
            other => Err(Flow::Fatal(format!(
                "compose of a non-function ({})",
                other.type_name()
            ))),
        }
    }

    fn eval_match(&mut self, env: &Rc<Env>, v: Value, arms: &[MatchArm]) -> R {
        for arm in arms {
            let inner = Env::new(Some(env.clone()));
            if self.matches(&arm.pat, &v, &inner)? {
                if let Some(g) = &arm.guard {
                    match self.eval(&inner, g)? {
                        Value::Bool(true) => {}
                        Value::Bool(false) => continue,
                        other => {
                            return Err(Flow::Fatal(format!(
                                "match guard is a {}",
                                other.type_name()
                            )))
                        }
                    }
                }
                return self.eval(&inner, &arm.body);
            }
        }
        Err(Flow::Fatal(
            "no match arm matched (non-exhaustive match reached at run time)".into(),
        ))
    }

    fn matches(&mut self, pat: &Pattern, v: &Value, env: &Rc<Env>) -> Result<bool, Flow> {
        Ok(match pat {
            Pattern::Wild => true,
            Pattern::Bind(n) => {
                env.define(n, v.clone());
                true
            }
            Pattern::Int(n) => matches!(v, Value::Int(m) if m == n),
            Pattern::Float(f) => matches!(v, Value::Float(g) if g.0 == f.0),
            Pattern::Str(s) => matches!(v, Value::Str(t) if &**t == s.as_str()),
            Pattern::Bool(b) => matches!(v, Value::Bool(c) if c == b),
            Pattern::None => matches!(v, Value::None),
            Pattern::Some(p) => match v {
                Value::Some(x) => self.matches(p, x, env)?,
                _ => false,
            },
            Pattern::Ok(p) => match v {
                Value::Ok(x) => self.matches(p, x, env)?,
                _ => false,
            },
            Pattern::Err(p) => match v {
                Value::Err(x) => self.matches(p, x, env)?,
                _ => false,
            },
            Pattern::Ctor {
                module: _,
                name,
                args,
            } => match v {
                Value::Variant { case, payload, .. } if &**case == name.as_str() => match payload {
                    Payload::Unit => args.is_empty(),
                    Payload::Tuple(items) => {
                        if items.len() != args.len() {
                            return Ok(false);
                        }
                        for (p, x) in args.iter().zip(items.iter()) {
                            if !self.matches(p, x, env)? {
                                return Ok(false);
                            }
                        }
                        true
                    }
                    Payload::Record(_) => false,
                },
                _ => false,
            },
            Pattern::CtorRecord {
                module: _,
                name,
                fields,
                rest,
            } => match v {
                Value::Variant {
                    case,
                    payload: Payload::Record(items),
                    ..
                } if &**case == name.as_str() => {
                    if !*rest && items.len() != fields.len() {
                        return Ok(false);
                    }
                    for (fname, sub) in fields {
                        let fv = match items.iter().find(|(n, _)| &**n == fname.as_str()) {
                            Some((_, fv)) => fv.clone(),
                            None => return Ok(false),
                        };
                        match sub {
                            Some(p) => {
                                if !self.matches(p, &fv, env)? {
                                    return Ok(false);
                                }
                            }
                            None => env.define(fname, fv),
                        }
                    }
                    true
                }
                // a plain record type used as a pattern: `P { x, y }`
                Value::Record {
                    type_name: Some(t),
                    fields: items,
                } if &**t == name.as_str() => {
                    for (fname, sub) in fields {
                        let fv = match items.iter().find(|(n, _)| &**n == fname.as_str()) {
                            Some((_, fv)) => fv.clone(),
                            None => return Ok(false),
                        };
                        match sub {
                            Some(p) => {
                                if !self.matches(p, &fv, env)? {
                                    return Ok(false);
                                }
                            }
                            None => env.define(fname, fv),
                        }
                    }
                    true
                }
                _ => false,
            },
            Pattern::Tuple(pats) => match v {
                Value::Tuple(items) if items.len() == pats.len() => {
                    for (p, x) in pats.iter().zip(items.iter()) {
                        if !self.matches(p, x, env)? {
                            return Ok(false);
                        }
                    }
                    true
                }
                _ => false,
            },
            Pattern::List(pats) => match v {
                Value::List(items) if items.len() == pats.len() => {
                    for (p, x) in pats.iter().zip(items.iter()) {
                        if !self.matches(p, x, env)? {
                            return Ok(false);
                        }
                    }
                    true
                }
                _ => false,
            },
        })
    }

    fn binop(&mut self, op: BinOp, l: Value, r: Value) -> R {
        use BinOp::*;
        match (op, &l, &r) {
            // Int + - * WRAP two's-complement (int_pow_overflow_wraps,
            // toplevel_const_wrapping, math.choose at i64::MAX); only / and %
            // abort (T6)
            (Add, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_add(*b))),
            (Sub, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_sub(*b))),
            (Mul, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_mul(*b))),
            (Div, Value::Int(a), Value::Int(b)) => {
                // ALS-T6: `/` and `%` are total — zero divisor and MIN÷-1
                // abort in the T6 form, never trap and never wrap silently
                if *b == 0 {
                    return Err(Flow::Abort("division by zero".into()));
                }
                match a.checked_div(*b) {
                    Some(v) => Ok(Value::Int(v)),
                    None => Err(Flow::Abort("integer overflow".into())),
                }
            }
            (Rem, Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    return Err(Flow::Abort("division by zero".into()));
                }
                match a.checked_rem(*b) {
                    Some(v) => Ok(Value::Int(v)),
                    None => Err(Flow::Abort("integer overflow".into())),
                }
            }
            (Pow, Value::Int(a), Value::Int(b)) => {
                // ALS-E29: `**` on Int desugars to math.pow
                stdlib::call(self, "math.pow", vec![Value::Int(*a), Value::Int(*b)])
            }
            (Add, Value::Float(a), Value::Float(b)) => Ok(Value::Float(F64(fnan(a.0 + b.0)))),
            (Sub, Value::Float(a), Value::Float(b)) => Ok(Value::Float(F64(fnan(a.0 - b.0)))),
            (Mul, Value::Float(a), Value::Float(b)) => Ok(Value::Float(F64(fnan(a.0 * b.0)))),
            (Div, Value::Float(a), Value::Float(b)) => Ok(Value::Float(F64(fnan(a.0 / b.0)))),
            (Rem, Value::Float(a), Value::Float(b)) => Ok(Value::Float(F64(fnan(a.0 % b.0)))),
            (Pow, Value::Float(a), Value::Float(b)) => {
                // `**` on Float routes through the vendored libm pow (math.fpow)
                Ok(Value::Float(F64(fnan(crate::libm::almide_rt_libm_pow(
                    a.0, b.0,
                )))))
            }
            (Add, Value::Str(a), Value::Str(b)) => {
                let mut s = a.to_string();
                s.push_str(b);
                Ok(Value::str(&s))
            }
            (Add, Value::List(a), Value::List(b)) => {
                let mut v = (**a).clone();
                v.extend(b.iter().cloned());
                Ok(Value::List(Rc::new(v)))
            }
            (Eq, _, _) => match values_eq(&l, &r) {
                Some(b) => Ok(Value::Bool(b)),
                None => Err(Flow::Fatal(format!(
                    "`==` on {} and {}",
                    l.type_name(),
                    r.type_name()
                ))),
            },
            (Ne, _, _) => match values_eq(&l, &r) {
                Some(b) => Ok(Value::Bool(!b)),
                None => Err(Flow::Fatal(format!(
                    "`!=` on {} and {}",
                    l.type_name(),
                    r.type_name()
                ))),
            },
            (Lt | Le | Gt | Ge, _, _) => {
                let ord = match (&l, &r) {
                    (Value::Int(a), Value::Int(b)) => a.cmp(b),
                    (Value::Float(a), Value::Float(b)) => match a.0.partial_cmp(&b.0) {
                        Some(o) => o,
                        None => return Ok(Value::Bool(false)),
                    },
                    (Value::Str(a), Value::Str(b)) => char_cmp(a, b),
                    _ => {
                        return self.abstain(
                            "semantics:compare",
                            format!("ordering {} and {}", l.type_name(), r.type_name()),
                        )
                    }
                };
                Ok(Value::Bool(match op {
                    Lt => ord == std::cmp::Ordering::Less,
                    Le => ord != std::cmp::Ordering::Greater,
                    Gt => ord == std::cmp::Ordering::Greater,
                    _ => ord != std::cmp::Ordering::Less,
                }))
            }
            (And | Or, _, _) => Err(Flow::Fatal("and/or are short-circuit forms".into())),
            _ => Err(Flow::Fatal(format!(
                "operator {op:?} on {} and {}",
                l.type_name(),
                r.type_name()
            ))),
        }
    }

    // ── statements ───────────────────────────────────────────────────────

    fn exec_block(&mut self, env: &Rc<Env>, stmts: &[Stmt]) -> R {
        let mut last = Value::Unit;
        let n = stmts.len();
        for (i, s) in stmts.iter().enumerate() {
            last = Value::Unit;
            match s {
                Stmt::Let { pat, ty, expr, .. } => {
                    let v = snapshot_bytes(self.eval(env, expr)?);
                    let v = self.retag(v, ty.as_ref());
                    self.bind_let(env, pat, v)?;
                }
                Stmt::Var { name, ty, expr, .. } => {
                    let v = snapshot_bytes(self.eval(env, expr)?);
                    let v = self.retag(v, ty.as_ref());
                    env.define(name, v);
                }
                Stmt::Assign { place, expr, .. } => {
                    let v = self.eval(env, expr)?;
                    self.assign_place(env, place, v)?;
                }
                Stmt::Guard { cond, els, .. } => match self.eval(env, cond)? {
                    Value::Bool(true) => {}
                    Value::Bool(false) => {
                        // ALS-ST5: the else is a raise — `err(e)` / `err(e)!` — or a Never
                        let v = self.eval(env, els)?;
                        return match v {
                            Value::Err(e) => Err(Flow::Propagate(Prop::Err((*e).clone()))),
                            other => self.abstain("semantics:guard-else-value", format!("guard else evaluated to a {} (only err(e) / err(e)! / Never are read into this evaluator)", other.type_name())),
                        };
                    }
                    other => {
                        return Err(Flow::Fatal(format!(
                            "guard condition is a {}",
                            other.type_name()
                        )))
                    }
                },
                Stmt::GuardLet {
                    name, scrut, els, ..
                } => match self.eval(env, scrut)? {
                    Value::Ok(v) | Value::Some(v) => env.define(name, (*v).clone()),
                    Value::Err(_) | Value::None => {
                        let v = self.eval(env, els)?;
                        return match v {
                            Value::Err(e) => Err(Flow::Propagate(Prop::Err((*e).clone()))),
                            other => self.abstain(
                                "semantics:guard-else-value",
                                format!("guard let else evaluated to a {}", other.type_name()),
                            ),
                        };
                    }
                    other => {
                        return Err(Flow::Fatal(format!(
                            "guard let scrutinee is a {}",
                            other.type_name()
                        )))
                    }
                },
                Stmt::Expr(e, _) => {
                    let v = self.eval(env, e)?;
                    if i + 1 == n {
                        last = v;
                    }
                }
            }
        }
        Ok(last)
    }

    fn bind_let(&mut self, env: &Rc<Env>, pat: &LetPat, v: Value) -> Result<(), Flow> {
        match pat {
            LetPat::Name(n) => {
                env.define(n, v);
                Ok(())
            }
            LetPat::Wild => Ok(()),
            LetPat::Tuple(pats) => match v {
                Value::Tuple(items) if items.len() == pats.len() => {
                    for (p, x) in pats.iter().zip(items.iter()) {
                        self.bind_let(env, p, x.clone())?;
                    }
                    Ok(())
                }
                other => Err(Flow::Fatal(format!(
                    "tuple destructuring of a {}",
                    other.type_name()
                ))),
            },
            LetPat::Record(names) => match v {
                Value::Record { fields, .. } => {
                    for n in names {
                        match fields.iter().find(|(k, _)| &**k == n.as_str()) {
                            Some((_, fv)) => env.define(n, fv.clone()),
                            None => {
                                return Err(Flow::Fatal(format!(
                                    "record destructuring: no field `{n}`"
                                )))
                            }
                        }
                    }
                    Ok(())
                }
                other => Err(Flow::Fatal(format!(
                    "record destructuring of a {}",
                    other.type_name()
                ))),
            },
        }
    }

    /// Place assignment with value semantics (ALS-ST4): the root variable is
    /// rebound to an updated copy; no sharing is observable.
    fn assign_place(&mut self, env: &Rc<Env>, place: &Expr, v: Value) -> Result<(), Flow> {
        match place {
            Expr::TypeName { module: None, name } if env.lookup(name).is_some() => {
                env.assign(name, v);
                Ok(())
            }
            Expr::Ident(name) => {
                if env.assign(name, v) {
                    Ok(())
                } else {
                    Err(Flow::Fatal(format!("assignment to unbound `{name}`")))
                }
            }
            Expr::Index { obj, idx } => {
                let cur = self.eval(env, obj)?;
                let i = self.eval(env, idx)?;
                let updated = match (cur, i) {
                    (Value::List(xs), Value::Int(i)) => {
                        if i < 0 || i as usize >= xs.len() {
                            return Err(Flow::Abort("index out of bounds".into()));
                        }
                        let mut nv = (*xs).clone();
                        nv[i as usize] = v;
                        Value::List(Rc::new(nv))
                    }
                    (Value::Map(kvs), k) => {
                        let mut nv = (*kvs).clone();
                        match nv
                            .iter_mut()
                            .find(|(k2, _)| values_eq(k2, &k) == Some(true))
                        {
                            Some(slot) => slot.1 = v,
                            None => nv.push((k, v)),
                        }
                        Value::Map(Rc::new(nv))
                    }
                    (o, i) => {
                        return self.abstain(
                            "semantics:index-assign",
                            format!(
                                "index assignment on a {} with a {}",
                                o.type_name(),
                                i.type_name()
                            ),
                        )
                    }
                };
                self.assign_place(env, obj, updated)
            }
            Expr::Member { obj, name } => {
                let cur = self.eval(env, obj)?;
                let updated = match cur {
                    Value::Record { type_name, fields } => {
                        let mut nf = (*fields).clone();
                        match nf.iter_mut().find(|(n, _)| &**n == name.as_str()) {
                            Some(slot) => slot.1 = v,
                            None => {
                                return Err(Flow::Fatal(format!("no field `{name}` to assign")))
                            }
                        }
                        Value::Record {
                            type_name,
                            fields: Rc::new(nf),
                        }
                    }
                    other => {
                        return self.abstain(
                            "semantics:field-assign",
                            format!("field assignment on a {}", other.type_name()),
                        )
                    }
                };
                self.assign_place(env, obj, updated)
            }
            Expr::TupleIndex { .. } => {
                self.abstain("semantics:tuple-assign", "tuple element assignment")
            }
            other => Err(Flow::Fatal(format!("invalid assignment target: {other:?}"))),
        }
    }
}

/// Copy every binding SLOT visible from `env` (up to, not including,
/// `globals`) into `into`, innermost shadowing outermost — the closure's
/// capture: shared slots, frozen shadowing.
fn snapshot_into(env: &Rc<Env>, into: &Rc<Env>, globals: &Rc<Env>) {
    let mut chain: Vec<Rc<Env>> = Vec::new();
    let mut cur = Some(env.clone());
    while let Some(e) = cur {
        if Rc::ptr_eq(&e, globals) {
            break;
        }
        chain.push(e.clone());
        cur = e.parent.clone();
    }
    for e in chain.iter().rev() {
        for (n, s) in e.vars.borrow().iter() {
            into.vars.borrow_mut().push((n.clone(), s.clone()));
        }
    }
}

/// ALS string ordering: code-point order == UTF-8 byte order; compared
/// char by char here (clause 5: no `str::cmp`).
pub fn char_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut ia = a.chars();
    let mut ib = b.chars();
    loop {
        match (ia.next(), ib.next()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) => {
                if x != y {
                    return (x as u32).cmp(&(y as u32));
                }
            }
        }
    }
}

/// Does this lambda body use `!` (outside nested lambdas)? — the syntactic
/// approximation of ADR-0006/0009 use-driven fallibility (L1).
fn expr_has_unwrap(e: &Expr) -> bool {
    match e {
        Expr::Unwrap(_) => true,
        Expr::Lambda { .. } => false,
        Expr::Int(_)
        | Expr::BigInt(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::Unit
        | Expr::Ident(_)
        | Expr::TypeName { .. }
        | Expr::EmptyMap
        | Expr::None
        | Expr::Todo(_)
        | Expr::Hole
        | Expr::Break
        | Expr::Continue => false,
        Expr::Str(segs) => segs
            .iter()
            .any(|s| matches!(s, StrSeg::Expr(x) if expr_has_unwrap(x))),
        Expr::List(xs) | Expr::Tuple(xs) => xs.iter().any(expr_has_unwrap),
        Expr::Map(ps) => ps
            .iter()
            .any(|(k, v)| expr_has_unwrap(k) || expr_has_unwrap(v)),
        Expr::Record { spread, fields, .. } => {
            spread.as_ref().map(|s| expr_has_unwrap(s)).unwrap_or(false)
                || fields.iter().any(|(_, x)| expr_has_unwrap(x))
        }
        Expr::Block(stmts) => stmts.iter().any(stmt_has_unwrap),
        Expr::If { cond, then, els } => {
            expr_has_unwrap(cond)
                || expr_has_unwrap(then)
                || els.as_ref().map(|x| expr_has_unwrap(x)).unwrap_or(false)
        }
        Expr::IfLet {
            scrut, then, els, ..
        } => expr_has_unwrap(scrut) || expr_has_unwrap(then) || expr_has_unwrap(els),
        Expr::Match { subject, arms } => {
            expr_has_unwrap(subject)
                || arms.iter().any(|a| {
                    expr_has_unwrap(&a.body)
                        || a.guard.as_ref().map(expr_has_unwrap).unwrap_or(false)
                })
        }
        Expr::PipeMatch { arms } => arms.iter().any(|a| expr_has_unwrap(&a.body)),
        Expr::For { iter, body, .. } => expr_has_unwrap(iter) || expr_has_unwrap(body),
        Expr::While { cond, body } => expr_has_unwrap(cond) || expr_has_unwrap(body),
        Expr::Fan {
            head_args, arms, ..
        } => head_args.iter().any(expr_has_unwrap) || arms.iter().any(expr_has_unwrap),
        Expr::Call { callee, args, .. } => {
            expr_has_unwrap(callee)
                || args.iter().any(|a| match a {
                    Arg::Pos(x) | Arg::Named(_, x) => expr_has_unwrap(x),
                    Arg::Placeholder => false,
                })
        }
        Expr::Index { obj, idx } => expr_has_unwrap(obj) || expr_has_unwrap(idx),
        Expr::Member { obj, .. } | Expr::TupleIndex { obj, .. } | Expr::OptChain { obj, .. } => {
            expr_has_unwrap(obj)
        }
        Expr::ToOption(x) | Expr::Some(x) | Expr::Ok(x) | Expr::Err(x) | Expr::Paren(x) => {
            expr_has_unwrap(x)
        }
        Expr::UnwrapOr { expr, fallback } => expr_has_unwrap(expr) || expr_has_unwrap(fallback),
        Expr::Binary { lhs, rhs, .. } | Expr::Pipe { lhs, rhs } | Expr::Compose { lhs, rhs } => {
            expr_has_unwrap(lhs) || expr_has_unwrap(rhs)
        }
        Expr::Unary { expr, .. } | Expr::Ascription { expr, .. } => expr_has_unwrap(expr),
        Expr::Range { lo, hi, .. } => expr_has_unwrap(lo) || expr_has_unwrap(hi),
    }
}

fn stmt_has_unwrap(s: &Stmt) -> bool {
    match s {
        Stmt::Let { expr, .. } | Stmt::Var { expr, .. } | Stmt::Expr(expr, _) => {
            expr_has_unwrap(expr)
        }
        Stmt::Assign { place, expr, .. } => expr_has_unwrap(place) || expr_has_unwrap(expr),
        Stmt::Guard { cond, els, .. } => expr_has_unwrap(cond) || expr_has_unwrap(els),
        Stmt::GuardLet { scrut, els, .. } => expr_has_unwrap(scrut) || expr_has_unwrap(els),
    }
}

// ── ALS-D6: the derived Codec bridge between record/variant values and the
// dynamic Value (Dyn) model. Decode walks the DECLARED fields in order and
// returns the first error; encode walks declaration order and omits `none`.
// Everything the corpus does not pin abstains instead of guessing.
impl Interp {
    fn codec_decl(&self, t: &str) -> Result<Rc<TypeDecl>, Flow> {
        match self.types.get(t) {
            Some(d) if d.conventions.iter().any(|c| c == "Codec") => Ok(d.clone()),
            _ => self.abstain(
                "semantics:codec",
                format!("`{t}` used as a Codec without a `: Codec` declaration"),
            ),
        }
    }

    /// outer Err = abstain/fatal; inner Err = the decode error string (C-084)
    fn codec_decode(&mut self, t: &str, d: &Dyn) -> Result<Result<Value, String>, Flow> {
        let decl = self.codec_decl(t)?;
        match &decl.body {
            TypeBody::Record(fds) => {
                let Dyn::O(pairs) = d else {
                    return Ok(Err("expected Object".into()));
                };
                let mut fields: Vec<(Rc<str>, Value)> = Vec::with_capacity(fds.len());
                for fd in fds {
                    let key = fd.alias.as_deref().unwrap_or(&fd.name);
                    let found = pairs
                        .iter()
                        .find(|(k, _)| &**k == key)
                        .map(|(_, v)| v.clone());
                    match self.codec_decode_field(fd, key, found.as_ref())? {
                        Ok(v) => fields.push((Rc::from(fd.name.as_str()), v)),
                        Err(e) => return Ok(Err(e)),
                    }
                }
                Ok(Ok(Value::Record {
                    type_name: Some(Rc::from(t)),
                    fields: Rc::new(fields),
                }))
            }
            TypeBody::Variant(cases) => {
                let Dyn::O(pairs) = d else {
                    return self.abstain(
                        "semantics:codec-variant-shape",
                        format!("variant `{t}` decoded from a non-object"),
                    );
                };
                if pairs.len() != 1 {
                    return self.abstain(
                        "semantics:codec-variant-shape",
                        format!("variant `{t}` document with {} keys", pairs.len()),
                    );
                }
                let (tag, payload) = (&pairs[0].0, &pairs[0].1);
                let case = cases.iter().find(|c| match c {
                    VariantCase::Unit(n) | VariantCase::Tuple(n, _) | VariantCase::Record(n, _) => {
                        **n == **tag
                    }
                });
                let Some(case) = case else {
                    return Ok(Err(format!("unknown variant for {t}")));
                };
                match case {
                    VariantCase::Unit(n) => match payload {
                        Dyn::Null => Ok(Ok(Value::Variant {
                            type_name: Rc::from(t),
                            case: Rc::from(n.as_str()),
                            payload: Payload::Unit,
                        })),
                        _ => self.abstain(
                            "semantics:codec-variant-shape",
                            format!("unit case `{n}` with a non-null payload"),
                        ),
                    },
                    VariantCase::Tuple(n, tys) => {
                        let n = n.clone();
                        let tys = tys.clone();
                        let Dyn::A(items) = payload else {
                            return self.abstain(
                                "semantics:codec-variant-shape",
                                format!("tuple case `{n}` with a non-array payload"),
                            );
                        };
                        if items.len() != tys.len() {
                            return self.abstain(
                                "semantics:codec-variant-shape",
                                format!(
                                    "tuple case `{n}` arity {} document, {} declared",
                                    items.len(),
                                    tys.len()
                                ),
                            );
                        }
                        let items = items.clone();
                        let mut out = Vec::with_capacity(items.len());
                        for (ty, it) in tys.iter().zip(items.iter()) {
                            match self.codec_decode_ty(ty, it)? {
                                Ok(v) => out.push(v),
                                Err(e) => return Ok(Err(e)),
                            }
                        }
                        Ok(Ok(Value::Variant {
                            type_name: Rc::from(t),
                            case: Rc::from(n.as_str()),
                            payload: Payload::Tuple(Rc::new(out)),
                        }))
                    }
                    VariantCase::Record(n, _) => self.abstain(
                        "semantics:codec-variant-shape",
                        format!("record-payload case `{n}`"),
                    ),
                }
            }
            TypeBody::Alias(_) => {
                self.abstain("semantics:codec", format!("`{t}.decode` on a type alias"))
            }
        }
    }

    fn codec_decode_field(
        &mut self,
        fd: &FieldDecl,
        key: &str,
        found: Option<&Dyn>,
    ) -> Result<Result<Value, String>, Flow> {
        if let TypeExpr::Option(inner) = &fd.ty {
            // C-209: missing and explicit null both fold to none — except
            // Option[Value], the 3-state cell (null → some(null))
            let dynamic =
                matches!(&**inner, TypeExpr::Named { module: None, name, .. } if name == "Value");
            return match found {
                None => {
                    if fd
                        .default
                        .as_ref()
                        .is_some_and(|d| !matches!(d, Expr::None))
                    {
                        return self.abstain(
                            "semantics:codec-field-type",
                            format!(
                                "Option field `{key}` with a non-none default and a missing key"
                            ),
                        );
                    }
                    Ok(Ok(Value::None))
                }
                Some(Dyn::Null) if !dynamic => Ok(Ok(Value::None)),
                Some(v) => Ok(match self.codec_decode_ty(inner, v)? {
                    Ok(x) => Ok(Value::Some(Rc::new(x))),
                    Err(e) => Err(e),
                }),
            };
        }
        match found {
            Some(v) => self.codec_decode_ty(&fd.ty, v),
            None => match &fd.default {
                Some(dx) => {
                    let dx = dx.clone();
                    let env = Env::new(Some(self.globals.clone()));
                    Ok(Ok(self.eval(&env, &dx)?))
                }
                None => Ok(Err(format!("missing field '{key}'"))),
            },
        }
    }

    fn codec_decode_ty(&mut self, ty: &TypeExpr, d: &Dyn) -> Result<Result<Value, String>, Flow> {
        match ty {
            TypeExpr::Named {
                module: None,
                name,
                args,
            } => match (name.as_str(), args.as_slice()) {
                ("Int", []) => Ok(match d {
                    Dyn::I(n) => Ok(Value::Int(*n)),
                    _ => Err("expected Int".into()),
                }),
                // C-085: an integer-formed JSON number widens into a Float field
                ("Float", []) => Ok(match d {
                    Dyn::F(f) => Ok(Value::Float(F64(*f))),
                    Dyn::I(n) => Ok(Value::Float(F64(*n as f64))),
                    _ => Err("expected Float".into()),
                }),
                ("String", []) => Ok(match d {
                    Dyn::S(s) => Ok(Value::Str(s.clone())),
                    _ => Err("expected Str".into()),
                }),
                ("Bool", []) => Ok(match d {
                    Dyn::B(b) => Ok(Value::Bool(*b)),
                    _ => Err("expected Bool".into()),
                }),
                ("Value", []) => Ok(Ok(Value::Dyn(d.clone()))),
                ("List", [elem]) => {
                    let Dyn::A(items) = d else {
                        return Ok(Err("expected Array".into()));
                    };
                    let elem = elem.clone();
                    let items = items.clone();
                    let mut out = Vec::with_capacity(items.len());
                    for it in items.iter() {
                        match self.codec_decode_ty(&elem, it)? {
                            Ok(v) => out.push(v),
                            Err(e) => return Ok(Err(e)),
                        }
                    }
                    Ok(Ok(Value::List(Rc::new(out))))
                }
                (other, []) if self.types.contains_key(other) => {
                    let other = other.to_string();
                    self.codec_decode(&other, d)
                }
                (other, _) => self.abstain(
                    "semantics:codec-field-type",
                    format!("decoding a `{other}` field"),
                ),
            },
            _ => self.abstain(
                "semantics:codec-field-type",
                format!("decoding a field of shape {ty:?}"),
            ),
        }
    }

    fn codec_encode(&mut self, t: &str, v: &Value) -> Result<Dyn, Flow> {
        let decl = self.codec_decl(t)?;
        match (&decl.body, v) {
            (TypeBody::Record(fds), Value::Record { fields, .. }) => {
                let fields = fields.clone();
                let mut out: Vec<(Rc<str>, Dyn)> = Vec::with_capacity(fds.len());
                for fd in fds {
                    let Some((_, fv)) = fields.iter().find(|(k, _)| &**k == fd.name.as_str())
                    else {
                        return self.abstain(
                            "semantics:codec",
                            format!("record value missing declared field `{}`", fd.name),
                        );
                    };
                    let key: Rc<str> = Rc::from(fd.alias.as_deref().unwrap_or(&fd.name));
                    match fv {
                        // C-209: none is OMITTED, never an explicit null
                        Value::None => {}
                        Value::Some(inner) => {
                            out.push((key, self.codec_encode_val(&inner.clone())?))
                        }
                        other => out.push((key, self.codec_encode_val(&other.clone())?)),
                    }
                }
                Ok(Dyn::O(Rc::new(out)))
            }
            (TypeBody::Variant(_), Value::Variant { case, payload, .. }) => {
                // externally-tagged: {"Case": [payload…]}, unit {"Case": null}
                match payload {
                    Payload::Unit => Ok(Dyn::O(Rc::new(vec![(case.clone(), Dyn::Null)]))),
                    Payload::Tuple(items) => {
                        let case = case.clone();
                        let items = items.clone();
                        let mut a = Vec::with_capacity(items.len());
                        for it in items.iter() {
                            a.push(self.codec_encode_val(it)?);
                        }
                        Ok(Dyn::O(Rc::new(vec![(case, Dyn::A(Rc::new(a)))])))
                    }
                    Payload::Record(_) => self.abstain(
                        "semantics:codec-variant-shape",
                        format!("encoding a record-payload case of `{t}`"),
                    ),
                }
            }
            (_, other) => self.abstain(
                "semantics:codec",
                format!("`{t}.encode` on a {}", other.type_name()),
            ),
        }
    }

    fn codec_encode_val(&mut self, v: &Value) -> Result<Dyn, Flow> {
        match v {
            Value::Int(n) => Ok(Dyn::I(*n)),
            Value::Float(F64(f)) => Ok(Dyn::F(*f)),
            Value::Bool(b) => Ok(Dyn::B(*b)),
            Value::Str(s) => Ok(Dyn::S(s.clone())),
            // Value passes through verbatim in both directions (C-209)
            Value::Dyn(d) => Ok(d.clone()),
            Value::List(items) => {
                let items = items.clone();
                let mut out = Vec::with_capacity(items.len());
                for it in items.iter() {
                    out.push(self.codec_encode_val(it)?);
                }
                Ok(Dyn::A(Rc::new(out)))
            }
            Value::Record {
                type_name: Some(t), ..
            } => {
                let t = t.to_string();
                self.codec_encode(&t, v)
            }
            Value::Variant { type_name, .. } => {
                let t = type_name.to_string();
                self.codec_encode(&t, v)
            }
            other => self.abstain(
                "semantics:codec-field-type",
                format!("encoding a {}", other.type_name()),
            ),
        }
    }
}
