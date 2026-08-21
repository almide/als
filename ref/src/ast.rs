//! AST — one variant per syntactic form of the ALS grammar (GRAMMAR.md EBNF,
//! ALS-E*/ST*/DL* sections). The evaluator's `match` over these enums is
//! EXHAUSTIVE by construction (ADR-0015 clause 2): a form the evaluator does
//! not implement is an explicit `Abstain` arm naming its class, never a
//! wildcard.

use crate::value::F64;

#[derive(Clone, Debug)]
pub struct Program {
    pub module: Option<Vec<String>>,
    pub imports: Vec<Import>,
    pub decls: Vec<Decl>,
}

#[derive(Clone, Debug)]
pub struct Import {
    pub path: Vec<String>,
    /// `import mod.{ a, B }`
    pub names: Option<Vec<String>>,
    /// `import mod as alias`
    pub alias: Option<String>,
    pub line: usize,
}

#[derive(Clone, Debug)]
pub enum Vis {
    Public,
    Mod,
    Local,
}

#[derive(Clone, Debug)]
pub enum Decl {
    Fn(FnDecl),
    Type(TypeDecl),
    TopLet {
        vis: Vis,
        mutable: bool,
        name: String,
        ty: Option<TypeExpr>,
        expr: Expr,
        line: usize,
    },
    Protocol {
        name: String,
        generics: Vec<String>,
        methods: Vec<FnSig>,
        line: usize,
    },
    Test {
        name: String,
        body: Expr,
        line: usize,
    },
}

#[derive(Clone, Debug)]
pub struct Attr {
    pub name: String,
    pub args: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct FnSig {
    pub effect: bool,
    pub owner: Option<String>,
    pub name: String,
    pub generics: Vec<String>,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
}

#[derive(Clone, Debug)]
pub struct FnDecl {
    pub attrs: Vec<Attr>,
    pub vis: Vis,
    pub sig: FnSig,
    pub body: Option<Expr>,
    pub line: usize,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub is_self: bool,
    pub mutable: bool,
    pub ty: Option<TypeExpr>,
    pub default: Option<Expr>,
}

#[derive(Clone, Debug)]
pub struct TypeDecl {
    pub vis: Vis,
    pub name: String,
    pub generics: Vec<String>,
    pub conventions: Vec<String>,
    pub body: TypeBody,
    pub line: usize,
}

#[derive(Clone, Debug)]
pub enum TypeBody {
    Record(Vec<FieldDecl>),
    Variant(Vec<VariantCase>),
    Alias(TypeExpr),
}

#[derive(Clone, Debug)]
pub struct FieldDecl {
    pub name: String,
    pub alias: Option<String>,
    pub ty: TypeExpr,
    pub default: Option<Expr>,
}

#[derive(Clone, Debug)]
pub enum VariantCase {
    Unit(String),
    Tuple(String, Vec<TypeExpr>),
    Record(String, Vec<FieldDecl>),
}

#[derive(Clone, Debug)]
pub enum TypeExpr {
    Named {
        module: Option<String>,
        name: String,
        args: Vec<TypeExpr>,
    },
    Record {
        fields: Vec<(String, TypeExpr)>,
        open: bool,
    },
    Unit,
    Tuple(Vec<TypeExpr>),
    Fn {
        effect: bool,
        params: Vec<TypeExpr>,
        ret: Box<TypeExpr>,
    },
    /// `T?`
    Option(Box<TypeExpr>),
    /// `T!` / `T!E`
    Fallible(Box<TypeExpr>, Option<Box<TypeExpr>>),
    Const(i64),
}

#[derive(Clone, Debug)]
pub enum StrSeg {
    Text(String),
    Expr(Expr),
}

#[derive(Clone, Debug)]
pub struct MatchArm {
    pub pat: Pattern,
    pub guard: Option<Expr>,
    pub body: Expr,
}

#[derive(Clone, Debug)]
pub struct LParam {
    pub names: Vec<String>, // len 1 = plain binder; len > 1 = tuple-destructuring param
    pub ty: Option<TypeExpr>,
}

#[derive(Clone, Debug)]
pub enum Arg {
    Pos(Expr),
    Named(String, Expr),
    Placeholder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Debug)]
pub enum Expr {
    Int(i64),
    /// a literal above i64::MAX — the UInt64 upper half (C-179)
    BigInt(u64),
    Float(F64),
    Bool(bool),
    Unit,
    /// interpolated or plain string
    Str(Vec<StrSeg>),
    Ident(String),
    /// bare constructor / type reference `Green`, possibly module-qualified
    TypeName {
        module: Option<String>,
        name: String,
    },
    List(Vec<Expr>),
    Map(Vec<(Expr, Expr)>),
    EmptyMap,
    Tuple(Vec<Expr>),
    Record {
        module: Option<String>,
        type_name: Option<String>,
        spread: Option<Box<Expr>>,
        fields: Vec<(String, Expr)>,
    },
    Block(Vec<Stmt>),
    If {
        cond: Box<Expr>,
        then: Box<Expr>,
        els: Option<Box<Expr>>,
    },
    IfLet {
        name: String,
        scrut: Box<Expr>,
        then: Box<Expr>,
        els: Box<Expr>,
    },
    Match {
        subject: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    PipeMatch {
        arms: Vec<MatchArm>,
    },
    For {
        binders: Vec<String>,
        iter: Box<Expr>,
        body: Box<Expr>,
    },
    While {
        cond: Box<Expr>,
        body: Box<Expr>,
    },
    /// `fan { a; b }` and the block heads `fan.any { … }` / `fan.settle { … }` /
    /// `fan.bounded(budget) { … }` — `head` is None for the plain block
    Fan {
        head: Option<String>,
        head_args: Vec<Expr>,
        arms: Vec<Expr>,
    },
    Lambda {
        params: Vec<LParam>,
        body: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        type_args: Vec<TypeExpr>,
        args: Vec<Arg>,
        line: usize,
    },
    Index {
        obj: Box<Expr>,
        idx: Box<Expr>,
    },
    Member {
        obj: Box<Expr>,
        name: String,
    },
    TupleIndex {
        obj: Box<Expr>,
        k: usize,
    },
    Unwrap(Box<Expr>),
    ToOption(Box<Expr>),
    OptChain {
        obj: Box<Expr>,
        name: String,
    },
    UnwrapOr {
        expr: Box<Expr>,
        fallback: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    Pipe {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Compose {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Range {
        lo: Box<Expr>,
        hi: Box<Expr>,
        inclusive: bool,
    },
    Some(Box<Expr>),
    None,
    Ok(Box<Expr>),
    Err(Box<Expr>),
    Todo(String),
    Hole,
    Break,
    Continue,
    Ascription {
        expr: Box<Expr>,
        ty: TypeExpr,
    },
    Paren(Box<Expr>),
}

#[derive(Clone, Debug)]
pub enum LetPat {
    Name(String),
    Wild,
    Tuple(Vec<LetPat>),
    Record(Vec<String>),
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Let {
        pat: LetPat,
        ty: Option<TypeExpr>,
        expr: Expr,
        line: usize,
    },
    Var {
        name: String,
        ty: Option<TypeExpr>,
        expr: Expr,
        line: usize,
    },
    Assign {
        place: Expr,
        expr: Expr,
        line: usize,
    },
    Guard {
        cond: Expr,
        els: Expr,
        line: usize,
    },
    GuardLet {
        name: String,
        scrut: Expr,
        els: Expr,
        line: usize,
    },
    Expr(Expr, usize),
}

#[derive(Clone, Debug)]
pub enum Pattern {
    Wild,
    Bind(String),
    Int(i64),
    Float(F64),
    Str(String),
    Bool(bool),
    None,
    Some(Box<Pattern>),
    Ok(Box<Pattern>),
    Err(Box<Pattern>),
    Ctor {
        module: Option<String>,
        name: String,
        args: Vec<Pattern>,
    },
    CtorRecord {
        module: Option<String>,
        name: String,
        fields: Vec<(String, Option<Pattern>)>,
        rest: bool,
    },
    Tuple(Vec<Pattern>),
    List(Vec<Pattern>),
}
