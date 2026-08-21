//! Recursive-descent parser written from the ALS grammar (GRAMMAR.md EBNF —
//! declarations, types, expressions, patterns, the precedence table and its
//! two asymmetries: `|>` takes a single postfix/compose chain, `??` takes a
//! unary fallback). Newlines are statement separators; a binary operator may
//! start a continuation line except a `-` glued to its operand (GRAMMAR
//! §Notes). Divergences between the grammar text and the accepted corpus are
//! listed in `docs/ref/PARSER-NOTES.md` and resolved in favour of the corpus.

use crate::ast::*;
use crate::lexer::{lex, StrPart, Tok, Token};
use crate::value::F64;

#[derive(Debug)]
pub struct ParseError {
    pub line: usize,
    pub msg: String,
}

type PResult<T> = Result<T, ParseError>;

const KEYWORDS: &[&str] = &[
    "module", "import", "type", "fn", "effect", "let", "var", "guard", "if", "then", "else",
    "match", "for", "in", "while", "fan", "test", "protocol", "local", "mod", "pub", "mut", "and",
    "or", "not", "true", "false", "break", "continue", "todo", "where",
];

pub fn parse_program(src: &str) -> PResult<Program> {
    let toks = lex(src).map_err(|e| ParseError {
        line: e.line,
        msg: e.msg,
    })?;
    let mut p = P {
        toks,
        pos: 0,
        no_brace_literal: false,
    };
    p.program()
}

/// Parse a standalone expression (interpolation segments).
fn parse_expr_src(src: &str, line: usize) -> PResult<Expr> {
    let toks = lex(src).map_err(|e| ParseError { line, msg: e.msg })?;
    let mut p = P {
        toks,
        pos: 0,
        no_brace_literal: false,
    };
    p.skip_nl();
    let e = p.expr()?;
    p.skip_nl();
    if !matches!(p.peek(), Tok::Eof) {
        return Err(ParseError {
            line,
            msg: format!("trailing tokens in interpolation: {:?}", p.peek()),
        });
    }
    Ok(e)
}

struct P {
    toks: Vec<Token>,
    pos: usize,
    /// while parsing a `while`/`for`/`match`/`if let` head, a `{` after a
    /// TypeName is the body, not a record literal
    no_brace_literal: bool,
}

impl P {
    // ── token helpers ───────────────────────────────────────────────────
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }
    fn peek_at(&self, k: usize) -> &Tok {
        let i = (self.pos + k).min(self.toks.len() - 1);
        &self.toks[i].tok
    }
    fn tok(&self) -> &Token {
        &self.toks[self.pos]
    }
    fn line(&self) -> usize {
        self.toks[self.pos].line
    }
    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }
    fn err<T>(&self, msg: impl Into<String>) -> PResult<T> {
        Err(ParseError {
            line: self.line(),
            msg: msg.into(),
        })
    }
    fn at_sym(&self, s: &str) -> bool {
        matches!(self.peek(), Tok::Sym(x) if *x == s)
    }
    fn at_sym_n(&self, k: usize, s: &str) -> bool {
        matches!(self.peek_at(k), Tok::Sym(x) if *x == s)
    }
    fn eat_sym(&mut self, s: &str) -> bool {
        if self.at_sym(s) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn expect_sym(&mut self, s: &str) -> PResult<()> {
        if self.eat_sym(s) {
            Ok(())
        } else {
            self.err(format!("expected {s:?}, got {:?}", self.peek()))
        }
    }
    fn at_kw(&self, k: &str) -> bool {
        matches!(self.peek(), Tok::Ident(x) if x == k)
    }
    fn at_kw_n(&self, n: usize, k: &str) -> bool {
        matches!(self.peek_at(n), Tok::Ident(x) if x == k)
    }
    fn eat_kw(&mut self, k: &str) -> bool {
        if self.at_kw(k) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn expect_kw(&mut self, k: &str) -> PResult<()> {
        if self.eat_kw(k) {
            Ok(())
        } else {
            self.err(format!("expected keyword {k:?}, got {:?}", self.peek()))
        }
    }
    fn at_nl(&self) -> bool {
        matches!(self.peek(), Tok::Newline)
    }
    fn skip_nl(&mut self) {
        while self.at_nl() {
            self.bump();
        }
    }
    fn at_eof(&self) -> bool {
        matches!(self.peek(), Tok::Eof)
    }
    /// identifier that is not a keyword (escaped identifiers always qualify)
    fn ident(&mut self) -> PResult<String> {
        match self.peek().clone() {
            Tok::Ident(s) if !KEYWORDS.contains(&s.as_str()) || s == "self" => {
                self.bump();
                Ok(s)
            }
            Tok::TypeName(s) => {
                // uppercase-initial binding names (`let PI = …`, `let MAX = …`)
                self.bump();
                Ok(s)
            }
            Tok::EscIdent(s) => {
                self.bump();
                Ok(s)
            }
            other => self.err(format!("expected identifier, got {other:?}")),
        }
    }
    /// a field / member name: identifiers AND the soft keywords ok/err/some/none/todo
    fn member_name(&mut self) -> PResult<String> {
        match self.peek().clone() {
            Tok::Ident(s)
                if !KEYWORDS.contains(&s.as_str())
                    || matches!(s.as_str(), "ok" | "err" | "some" | "none" | "todo" | "self") =>
            {
                self.bump();
                Ok(s)
            }
            Tok::EscIdent(s) => {
                self.bump();
                Ok(s)
            }
            other => self.err(format!("expected a name, got {other:?}")),
        }
    }
    fn type_name(&mut self) -> PResult<String> {
        match self.peek().clone() {
            Tok::TypeName(s) => {
                self.bump();
                Ok(s)
            }
            other => self.err(format!("expected type name, got {other:?}")),
        }
    }
    fn is_decl_start(&self) -> bool {
        if self.at_eof() {
            return true;
        }
        if self.at_sym("@") {
            return true;
        }
        for k in [
            "fn", "effect", "type", "protocol", "test", "import", "module", "pub", "local", "mod",
        ] {
            if self.at_kw(k) {
                return true;
            }
        }
        false
    }

    // ── program / declarations ──────────────────────────────────────────
    fn program(&mut self) -> PResult<Program> {
        let mut prog = Program {
            module: None,
            imports: Vec::new(),
            decls: Vec::new(),
        };
        self.skip_nl();
        if self.at_kw("module") {
            self.bump();
            prog.module = Some(self.dotted()?);
            self.skip_nl();
        }
        while self.at_kw("import") {
            let line = self.line();
            self.bump();
            let mut path = vec![self.member_name()?];
            let mut names = None;
            while self.eat_sym(".") {
                if self.at_sym("{") {
                    self.bump();
                    self.skip_nl();
                    let mut ns = Vec::new();
                    while !self.at_sym("}") {
                        ns.push(match self.peek().clone() {
                            Tok::TypeName(s) => {
                                self.bump();
                                s
                            }
                            _ => self.member_name()?,
                        });
                        self.skip_nl();
                        if !self.eat_sym(",") {
                            break;
                        }
                        self.skip_nl();
                    }
                    self.skip_nl();
                    self.expect_sym("}")?;
                    names = Some(ns);
                    break;
                }
                path.push(self.member_name()?);
            }
            let alias = if self.eat_kw("as") || self.at_kw("as") {
                Some(self.ident()?)
            } else {
                None
            };
            prog.imports.push(Import {
                path,
                names,
                alias,
                line,
            });
            self.skip_nl();
        }
        while !self.at_eof() {
            let d = self.decl()?;
            prog.decls.push(d);
            self.skip_nl();
        }
        Ok(prog)
    }

    fn dotted(&mut self) -> PResult<Vec<String>> {
        let mut v = vec![self.member_name()?];
        while self.at_sym(".") && !matches!(self.peek_at(1), Tok::Sym("{")) {
            self.bump();
            v.push(self.member_name()?);
        }
        Ok(v)
    }

    fn attrs(&mut self) -> PResult<Vec<Attr>> {
        let mut out = Vec::new();
        while self.at_sym("@") {
            self.bump();
            let name = self.member_name()?;
            let mut args = Vec::new();
            if self.eat_sym("(") {
                let mut depth = 1;
                let mut cur = String::new();
                loop {
                    match self.peek().clone() {
                        Tok::Sym("(") => {
                            depth += 1;
                            cur.push('(');
                            self.bump();
                        }
                        Tok::Sym(")") => {
                            depth -= 1;
                            self.bump();
                            if depth == 0 {
                                break;
                            }
                            cur.push(')');
                        }
                        Tok::Sym(",") if depth == 1 => {
                            args.push(std::mem::take(&mut cur));
                            self.bump();
                        }
                        Tok::Eof => return self.err("unterminated attribute"),
                        t => {
                            cur.push_str(&tok_text(&t));
                            self.bump();
                        }
                    }
                }
                if !cur.is_empty() {
                    args.push(cur);
                }
            }
            out.push(Attr { name, args });
            self.skip_nl();
        }
        Ok(out)
    }

    fn vis(&mut self) -> Vis {
        self.eat_kw("pub");
        if self.eat_kw("local") {
            Vis::Local
        } else if self.eat_kw("mod") {
            Vis::Mod
        } else {
            Vis::Public
        }
    }

    fn decl(&mut self) -> PResult<Decl> {
        let line = self.line();
        let attrs = self.attrs()?;
        let vis = self.vis();
        if self.at_kw("effect") || self.at_kw("fn") {
            return self.fn_decl(attrs, vis, line);
        }
        if self.at_kw("type") {
            return self.type_decl(vis, line);
        }
        if self.at_kw("protocol") {
            self.bump();
            let name = self.type_name()?;
            let generics = self.generic_params()?;
            self.skip_nl();
            self.expect_sym("{")?;
            let mut methods = Vec::new();
            loop {
                self.skip_nl();
                if self.eat_sym("}") {
                    break;
                }
                methods.push(self.fn_sig()?);
            }
            return Ok(Decl::Protocol {
                name,
                generics,
                methods,
                line,
            });
        }
        if self.at_kw("let") || self.at_kw("var") {
            let mutable = self.at_kw("var");
            self.bump();
            let name = self.ident()?;
            let ty = if self.eat_sym(":") {
                Some(self.ty()?)
            } else {
                None
            };
            self.expect_sym("=")?;
            self.skip_nl();
            let expr = self.expr()?;
            return Ok(Decl::TopLet {
                vis,
                mutable,
                name,
                ty,
                expr,
                line,
            });
        }
        if self.at_kw("test") {
            self.bump();
            let name = match self.peek().clone() {
                Tok::Str(parts) => {
                    self.bump();
                    parts
                        .iter()
                        .map(|p| match p {
                            StrPart::Text(t) => t.clone(),
                            StrPart::Interp(s) => format!("${{{s}}}"),
                        })
                        .collect::<String>()
                }
                Tok::PlainStr(s) => {
                    self.bump();
                    s
                }
                _ => return self.err("expected test name string"),
            };
            if self.at_kw("where") {
                return self
                    .err("`test … where` clauses are not supported by the reference parser yet");
            }
            self.skip_nl();
            let body = self.block()?;
            return Ok(Decl::Test { name, body, line });
        }
        self.err(format!("expected a declaration, got {:?}", self.peek()))
    }

    fn fn_sig(&mut self) -> PResult<FnSig> {
        let effect = self.eat_kw("effect");
        self.expect_kw("fn")?;
        let (owner, name) = match self.peek().clone() {
            Tok::TypeName(t) => {
                self.bump();
                self.expect_sym(".")?;
                (Some(t), self.member_name()?)
            }
            _ => (None, self.member_name()?),
        };
        let generics = self.generic_params()?;
        self.expect_sym("(")?;
        let mut params = Vec::new();
        self.skip_nl();
        while !self.at_sym(")") {
            let pattrs = self.attrs()?;
            let _ = pattrs;
            let mutable = self.eat_kw("mut");
            let is_self = self.at_kw("self");
            let pname = if is_self {
                self.bump();
                "self".to_string()
            } else {
                self.ident()?
            };
            let ty = if self.eat_sym(":") {
                Some(self.ty()?)
            } else {
                None
            };
            let default = if self.eat_sym("=") {
                Some(self.expr()?)
            } else {
                None
            };
            params.push(Param {
                name: pname,
                is_self,
                mutable,
                ty,
                default,
            });
            self.skip_nl();
            if !self.eat_sym(",") {
                break;
            }
            self.skip_nl();
        }
        self.skip_nl();
        self.expect_sym(")")?;
        let ret = if self.eat_sym("->") {
            self.ty()?
        } else {
            TypeExpr::Unit
        };
        Ok(FnSig {
            effect,
            owner,
            name,
            generics,
            params,
            ret,
        })
    }

    fn fn_decl(&mut self, attrs: Vec<Attr>, vis: Vis, line: usize) -> PResult<Decl> {
        let sig = self.fn_sig()?;
        let body = if self.at_sym("=") {
            self.bump();
            self.skip_nl();
            Some(self.fn_body()?)
        } else {
            None
        };
        Ok(Decl::Fn(FnDecl {
            attrs,
            vis,
            sig,
            body,
            line,
        }))
    }

    /// `fn_body = expr | braceless_block` — a body starting with let/var/guard
    /// collects statements until the next top-level declaration (GRAMMAR) —
    /// or a `let`/`var` at column 1, which is a top-level let (PARSER-NOTES).
    fn fn_body(&mut self) -> PResult<Expr> {
        if self.at_kw("let") || self.at_kw("var") || self.at_kw("guard") {
            let mut stmts = Vec::new();
            loop {
                self.skip_nl();
                if self.is_decl_start() {
                    break;
                }
                if (self.at_kw("let") || self.at_kw("var")) && self.tok().col == 1 {
                    break;
                }
                stmts.push(self.stmt()?);
                if !(self.at_nl() || self.eat_sym(";") || self.at_eof()) {
                    return self.err(format!("expected end of statement, got {:?}", self.peek()));
                }
            }
            return Ok(Expr::Block(stmts));
        }
        self.expr()
    }

    fn generic_params(&mut self) -> PResult<Vec<String>> {
        let mut out = Vec::new();
        if self.eat_sym("[") {
            loop {
                self.skip_nl();
                let n = self.type_name()?;
                if self.eat_sym(":") {
                    // bounds: TypeName (+ TypeName)* | structural { … }
                    if self.at_sym("{") {
                        let _ = self.ty()?;
                    } else {
                        let _ = self.type_name()?;
                        while self.eat_sym("+") {
                            let _ = self.type_name()?;
                        }
                    }
                }
                out.push(n);
                self.skip_nl();
                if !self.eat_sym(",") {
                    break;
                }
            }
            self.skip_nl();
            self.expect_sym("]")?;
        }
        Ok(out)
    }

    fn type_decl(&mut self, vis: Vis, line: usize) -> PResult<Decl> {
        self.expect_kw("type")?;
        let name = self.type_name()?;
        let generics = self.generic_params()?;
        let mut conventions = Vec::new();
        if self.eat_sym(":") {
            conventions.push(self.type_name()?);
            while self.eat_sym(",") {
                conventions.push(self.type_name()?);
            }
        }
        self.expect_sym("=")?;
        self.skip_nl();
        let variant_head = self.at_sym("|")
            || (matches!(self.peek(), Tok::TypeName(_))
                && (matches!(self.peek_at(1), Tok::Sym("(") | Tok::Sym("{"))
                    || self.variant_follows()));
        let body = if self.at_sym("{") {
            TypeBody::Record(self.field_decls()?)
        } else if variant_head {
            TypeBody::Variant(self.variant_cases()?)
        } else {
            TypeBody::Alias(self.ty()?)
        };
        Ok(Decl::Type(TypeDecl {
            vis,
            name,
            generics,
            conventions,
            body,
            line,
        }))
    }

    /// after a bare TypeName: is this `A | B …` (variant) rather than an alias?
    fn variant_follows(&self) -> bool {
        // TypeName [generic args]? then `|`
        let mut k = 1;
        if matches!(self.peek_at(k), Tok::Sym("[")) {
            let mut depth = 0;
            loop {
                match self.peek_at(k) {
                    Tok::Sym("[") => depth += 1,
                    Tok::Sym("]") => {
                        depth -= 1;
                        if depth == 0 {
                            k += 1;
                            break;
                        }
                    }
                    Tok::Eof => return false,
                    _ => {}
                }
                k += 1;
            }
        }
        // allow a newline before `|`
        while matches!(self.peek_at(k), Tok::Newline) {
            k += 1;
        }
        matches!(self.peek_at(k), Tok::Sym("|"))
    }

    fn variant_cases(&mut self) -> PResult<Vec<VariantCase>> {
        let mut cases = Vec::new();
        self.skip_nl();
        self.eat_sym("|");
        loop {
            self.skip_nl();
            let name = self.type_name()?;
            let case = if self.at_sym("(") {
                self.bump();
                let mut tys = Vec::new();
                self.skip_nl();
                while !self.at_sym(")") {
                    tys.push(self.ty()?);
                    self.skip_nl();
                    if !self.eat_sym(",") {
                        break;
                    }
                    self.skip_nl();
                }
                self.skip_nl();
                self.expect_sym(")")?;
                VariantCase::Tuple(name, tys)
            } else if self.at_sym("{") {
                VariantCase::Record(name, self.field_decls()?)
            } else {
                VariantCase::Unit(name)
            };
            cases.push(case);
            // continue on `|`, possibly after a newline
            let save = self.pos;
            self.skip_nl();
            if self.eat_sym("|") {
                continue;
            }
            self.pos = save;
            break;
        }
        Ok(cases)
    }

    fn field_decls(&mut self) -> PResult<Vec<FieldDecl>> {
        self.expect_sym("{")?;
        let mut out = Vec::new();
        loop {
            self.skip_nl();
            if self.eat_sym("}") {
                break;
            }
            let _ = self.attrs()?;
            let name = self.member_name()?;
            let alias = if self.eat_kw("as") {
                match self.peek().clone() {
                    Tok::Str(parts) => {
                        self.bump();
                        Some(
                            parts
                                .iter()
                                .map(|p| {
                                    if let StrPart::Text(t) = p {
                                        t.clone()
                                    } else {
                                        String::new()
                                    }
                                })
                                .collect(),
                        )
                    }
                    Tok::PlainStr(s) => {
                        self.bump();
                        Some(s)
                    }
                    _ => return self.err("expected alias string"),
                }
            } else {
                None
            };
            self.expect_sym(":")?;
            let ty = self.ty()?;
            let default = if self.eat_sym("=") {
                Some(self.expr()?)
            } else {
                None
            };
            out.push(FieldDecl {
                name,
                alias,
                ty,
                default,
            });
            self.skip_nl();
            if !self.eat_sym(",") {
                self.skip_nl();
                self.expect_sym("}")?;
                break;
            }
        }
        Ok(out)
    }

    // ── types ────────────────────────────────────────────────────────────
    fn ty(&mut self) -> PResult<TypeExpr> {
        let mut t = self.ty_atom()?;
        loop {
            if self.at_sym("?") && !self.tok().spaced {
                self.bump();
                t = TypeExpr::Option(Box::new(t));
            } else if self.at_sym("!") && !self.tok().spaced {
                self.bump();
                let e = if matches!(self.peek(), Tok::TypeName(_)) && !self.tok().spaced {
                    Some(Box::new(self.ty_atom()?))
                } else {
                    None
                };
                t = TypeExpr::Fallible(Box::new(t), e);
            } else {
                break;
            }
        }
        Ok(t)
    }

    fn ty_atom(&mut self) -> PResult<TypeExpr> {
        match self.peek().clone() {
            Tok::Sym("(") => {
                self.bump();
                self.skip_nl();
                if self.eat_sym(")") {
                    if self.eat_sym("->") {
                        let ret = self.ty()?;
                        return Ok(TypeExpr::Fn {
                            effect: false,
                            params: vec![],
                            ret: Box::new(ret),
                        });
                    }
                    return Ok(TypeExpr::Unit);
                }
                let mut items = vec![self.ty()?];
                self.skip_nl();
                while self.eat_sym(",") {
                    self.skip_nl();
                    if self.at_sym(")") {
                        break;
                    }
                    items.push(self.ty()?);
                    self.skip_nl();
                }
                self.expect_sym(")")?;
                if self.eat_sym("->") {
                    let ret = self.ty()?;
                    return Ok(TypeExpr::Fn {
                        effect: false,
                        params: items,
                        ret: Box::new(ret),
                    });
                }
                if items.len() == 1 {
                    return Ok(items.pop().unwrap());
                }
                Ok(TypeExpr::Tuple(items))
            }
            Tok::Ident(k) if k == "fn" || k == "Fn" || k == "effect" => {
                let effect = k == "effect";
                self.bump();
                if effect {
                    self.eat_kw("fn");
                }
                self.expect_sym("(")?;
                let mut params = Vec::new();
                self.skip_nl();
                while !self.at_sym(")") {
                    params.push(self.ty()?);
                    self.skip_nl();
                    if !self.eat_sym(",") {
                        break;
                    }
                    self.skip_nl();
                }
                self.expect_sym(")")?;
                self.expect_sym("->")?;
                let ret = self.ty()?;
                Ok(TypeExpr::Fn {
                    effect,
                    params,
                    ret: Box::new(ret),
                })
            }
            Tok::Sym("{") => {
                self.bump();
                let mut fields = Vec::new();
                let mut open = false;
                loop {
                    self.skip_nl();
                    if self.eat_sym("}") {
                        break;
                    }
                    if self.eat_sym("..") {
                        open = true;
                        self.skip_nl();
                        self.eat_sym(",");
                        self.skip_nl();
                        self.expect_sym("}")?;
                        break;
                    }
                    let n = self.member_name()?;
                    self.expect_sym(":")?;
                    let t = self.ty()?;
                    fields.push((n, t));
                    self.skip_nl();
                    if !self.eat_sym(",") {
                        self.skip_nl();
                        if self.eat_sym("..") {
                            open = true;
                            self.skip_nl();
                        }
                        self.expect_sym("}")?;
                        break;
                    }
                }
                Ok(TypeExpr::Record { fields, open })
            }
            Tok::TypeName(name) => {
                self.bump();
                let args = self.type_args()?;
                Ok(TypeExpr::Named {
                    module: None,
                    name,
                    args,
                })
            }
            Tok::Ident(m)
                if matches!(self.peek_at(1), Tok::Sym("."))
                    && matches!(self.peek_at(2), Tok::TypeName(_)) =>
            {
                self.bump();
                self.bump();
                let name = self.type_name()?;
                let args = self.type_args()?;
                Ok(TypeExpr::Named {
                    module: Some(m),
                    name,
                    args,
                })
            }
            Tok::Int(n) => {
                self.bump();
                Ok(TypeExpr::Const(n as i64))
            }
            other => self.err(format!("expected a type, got {other:?}")),
        }
    }

    fn type_args(&mut self) -> PResult<Vec<TypeExpr>> {
        let mut args = Vec::new();
        if self.at_sym("[") {
            self.bump();
            loop {
                self.skip_nl();
                args.push(self.ty()?);
                self.skip_nl();
                if !self.eat_sym(",") {
                    break;
                }
            }
            self.skip_nl();
            self.expect_sym("]")?;
        }
        Ok(args)
    }

    // ── statements & blocks ──────────────────────────────────────────────
    fn block(&mut self) -> PResult<Expr> {
        self.expect_sym("{")?;
        let mut stmts = Vec::new();
        loop {
            self.skip_nl();
            while self.eat_sym(";") {
                self.skip_nl();
            }
            if self.eat_sym("}") {
                break;
            }
            stmts.push(self.stmt()?);
            if self.at_sym("}") {
                continue;
            }
            if !(self.at_nl() || self.eat_sym(";")) {
                return self.err(format!(
                    "expected newline or `}}` after statement, got {:?}",
                    self.peek()
                ));
            }
        }
        Ok(Expr::Block(stmts))
    }

    fn let_pat(&mut self) -> PResult<LetPat> {
        if self.at_sym("(") {
            self.bump();
            let mut items = Vec::new();
            loop {
                self.skip_nl();
                items.push(self.let_pat()?);
                self.skip_nl();
                if !self.eat_sym(",") {
                    break;
                }
            }
            self.expect_sym(")")?;
            return Ok(LetPat::Tuple(items));
        }
        if self.at_sym("{") {
            self.bump();
            let mut names = Vec::new();
            loop {
                self.skip_nl();
                if self.at_sym("}") {
                    break;
                }
                names.push(self.member_name()?);
                self.skip_nl();
                if !self.eat_sym(",") {
                    break;
                }
            }
            self.skip_nl();
            self.expect_sym("}")?;
            return Ok(LetPat::Record(names));
        }
        let n = self.ident()?;
        if n == "_" {
            Ok(LetPat::Wild)
        } else {
            Ok(LetPat::Name(n))
        }
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        let line = self.line();
        if self.at_kw("let") {
            self.bump();
            let pat = self.let_pat()?;
            let ty = if self.eat_sym(":") {
                Some(self.ty()?)
            } else {
                None
            };
            self.expect_sym("=")?;
            self.skip_nl();
            let expr = self.expr()?;
            return Ok(Stmt::Let {
                pat,
                ty,
                expr,
                line,
            });
        }
        if self.at_kw("var") {
            self.bump();
            let name = self.ident()?;
            let ty = if self.eat_sym(":") {
                Some(self.ty()?)
            } else {
                None
            };
            self.expect_sym("=")?;
            self.skip_nl();
            let expr = self.expr()?;
            return Ok(Stmt::Var {
                name,
                ty,
                expr,
                line,
            });
        }
        if self.at_kw("guard") {
            self.bump();
            if self.eat_kw("let") {
                let name = self.ident()?;
                self.expect_sym("=")?;
                let scrut = self.expr()?;
                self.expect_kw("else")?;
                let els = self.expr()?;
                return Ok(Stmt::GuardLet {
                    name,
                    scrut,
                    els,
                    line,
                });
            }
            let cond = self.expr()?;
            self.expect_kw("else")?;
            self.skip_nl();
            let els = self.expr()?;
            return Ok(Stmt::Guard { cond, els, line });
        }
        let e = self.expr()?;
        if self.at_sym("=") {
            self.bump();
            self.skip_nl();
            let rhs = self.expr()?;
            return Ok(Stmt::Assign {
                place: e,
                expr: rhs,
                line,
            });
        }
        Ok(Stmt::Expr(e, line))
    }

    // ── expressions ──────────────────────────────────────────────────────
    pub fn expr(&mut self) -> PResult<Expr> {
        self.or_expr()
    }

    /// Newline continuation: a binary operator may start the next line,
    /// except a `-` glued to its operand (that starts a statement).
    fn continues_with(&self, syms: &[&str], kws: &[&str]) -> bool {
        if !self.at_nl() {
            return false;
        }
        let mut k = 1;
        while matches!(self.peek_at(k), Tok::Newline) {
            k += 1;
        }
        match self.peek_at(k) {
            Tok::Sym(s) => {
                if !syms.contains(s) {
                    return false;
                }
                if *s == "-" {
                    // glued `-`: the token after it is not spaced
                    let i = (self.pos + k + 1).min(self.toks.len() - 1);
                    return self.toks[i].spaced;
                }
                true
            }
            Tok::Ident(k2) => kws.contains(&k2.as_str()),
            _ => false,
        }
    }
    fn cont(&mut self, syms: &[&str], kws: &[&str]) {
        if self.continues_with(syms, kws) {
            self.skip_nl();
        }
    }

    fn or_expr(&mut self) -> PResult<Expr> {
        let mut lhs = self.and_expr()?;
        loop {
            self.cont(&[], &["or"]);
            if self.eat_kw("or") {
                self.skip_nl();
                let rhs = self.and_expr()?;
                lhs = Expr::Binary {
                    op: BinOp::Or,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                };
            } else {
                return Ok(lhs);
            }
        }
    }
    fn and_expr(&mut self) -> PResult<Expr> {
        let mut lhs = self.cmp_expr()?;
        loop {
            self.cont(&[], &["and"]);
            if self.eat_kw("and") {
                self.skip_nl();
                let rhs = self.cmp_expr()?;
                lhs = Expr::Binary {
                    op: BinOp::And,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                };
            } else {
                return Ok(lhs);
            }
        }
    }
    fn cmp_expr(&mut self) -> PResult<Expr> {
        let lhs = self.pipe_expr()?;
        self.cont(&["==", "!=", "<", "<=", ">", ">="], &[]);
        let op = match self.peek() {
            Tok::Sym("==") => BinOp::Eq,
            Tok::Sym("!=") => BinOp::Ne,
            Tok::Sym("<") => BinOp::Lt,
            Tok::Sym("<=") => BinOp::Le,
            Tok::Sym(">") => BinOp::Gt,
            Tok::Sym(">=") => BinOp::Ge,
            _ => return Ok(lhs),
        };
        self.bump();
        self.skip_nl();
        let rhs = self.pipe_expr()?;
        if matches!(self.peek(), Tok::Sym("==" | "!=" | "<" | "<=" | ">" | ">=")) {
            return self.err("comparison operators are non-associative: use `and`");
        }
        Ok(Expr::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
    }
    fn pipe_expr(&mut self) -> PResult<Expr> {
        let mut lhs = self.range_expr()?;
        loop {
            self.cont(&["|>"], &[]);
            if self.eat_sym("|>") {
                self.skip_nl();
                let rhs = if self.at_kw("match") {
                    self.bump();
                    self.skip_nl();
                    let arms = self.match_arms()?;
                    Expr::PipeMatch { arms }
                } else {
                    self.compose_expr()?
                };
                lhs = Expr::Pipe {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                };
            } else {
                return Ok(lhs);
            }
        }
    }
    fn range_expr(&mut self) -> PResult<Expr> {
        let lhs = self.add_expr()?;
        if self.at_sym("..<") || self.at_sym("...") {
            let inclusive = self.at_sym("...");
            self.bump();
            let rhs = self.add_expr()?;
            return Ok(Expr::Range {
                lo: Box::new(lhs),
                hi: Box::new(rhs),
                inclusive,
            });
        }
        Ok(lhs)
    }
    fn add_expr(&mut self) -> PResult<Expr> {
        let mut lhs = self.mul_expr()?;
        loop {
            self.cont(&["+", "-"], &[]);
            let op = match self.peek() {
                Tok::Sym("+") => BinOp::Add,
                Tok::Sym("-") => BinOp::Sub,
                _ => return Ok(lhs),
            };
            self.bump();
            self.skip_nl();
            let rhs = self.mul_expr()?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
    }
    fn mul_expr(&mut self) -> PResult<Expr> {
        let mut lhs = self.pow_expr()?;
        loop {
            self.cont(&["*", "/", "%"], &[]);
            let op = match self.peek() {
                Tok::Sym("*") => BinOp::Mul,
                Tok::Sym("/") => BinOp::Div,
                Tok::Sym("%") => BinOp::Rem,
                _ => return Ok(lhs),
            };
            self.bump();
            self.skip_nl();
            let rhs = self.pow_expr()?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
    }
    fn pow_expr(&mut self) -> PResult<Expr> {
        let lhs = self.compose_expr()?;
        if self.at_sym("^") || self.at_sym("**") {
            self.bump();
            self.skip_nl();
            let rhs = self.pow_expr()?; // right-assoc
            return Ok(Expr::Binary {
                op: BinOp::Pow,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            });
        }
        Ok(lhs)
    }
    fn compose_expr(&mut self) -> PResult<Expr> {
        let mut lhs = self.unary_expr()?;
        loop {
            self.cont(&[">>"], &[]);
            if self.eat_sym(">>") {
                self.skip_nl();
                let rhs = self.unary_expr()?;
                lhs = Expr::Compose {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                };
            } else {
                return Ok(lhs);
            }
        }
    }
    fn unary_expr(&mut self) -> PResult<Expr> {
        if self.at_sym("-") {
            self.bump();
            // ALS-E1: a minus directly before a literal folds into the literal
            if let Tok::Int(n) = self.peek().clone() {
                self.bump();
                // ALS-E1: the minus folds into the literal, so i64::MIN is writable
                if n == (i64::MAX as u64) + 1 {
                    return self.postfix_tail(Expr::Int(i64::MIN));
                }
                if n > i64::MAX as u64 {
                    return self.err("integer literal out of range for Int");
                }
                return self.postfix_tail(Expr::Int(-(n as i64)));
            }
            if let Tok::Float(f) = self.peek().clone() {
                self.bump();
                let v = parse_float_text(&f);
                return self.postfix_tail(Expr::Float(F64(-v)));
            }
            let e = self.unary_expr()?;
            return Ok(Expr::Unary {
                op: UnOp::Neg,
                expr: Box::new(e),
            });
        }
        if self.at_kw("not") {
            self.bump();
            let e = self.unary_expr()?;
            return Ok(Expr::Unary {
                op: UnOp::Not,
                expr: Box::new(e),
            });
        }
        if self.at_sym("!") {
            return self.err("prefix `!` is not accepted — use `not`");
        }
        self.postfix_expr()
    }
    fn postfix_expr(&mut self) -> PResult<Expr> {
        let p = self.primary()?;
        self.postfix_tail(p)
    }
    fn postfix_tail(&mut self, mut e: Expr) -> PResult<Expr> {
        loop {
            if self.at_sym("(") {
                // a call — but not across a newline (a `(` on the next line is a new statement)
                let line = self.line();
                let args = self.call_args()?;
                e = Expr::Call {
                    callee: Box::new(e),
                    type_args: vec![],
                    args,
                    line,
                };
                continue;
            }
            if self.at_sym("[") && !self.tok().spaced {
                // explicit type args `f[Int](x)` or index `xs[i]`
                if let Some(tys) = self.try_type_args_call()? {
                    let line = self.line();
                    let args = self.call_args()?;
                    e = Expr::Call {
                        callee: Box::new(e),
                        type_args: tys,
                        args,
                        line,
                    };
                    continue;
                }
                self.bump();
                self.skip_nl();
                let idx = self.expr()?;
                self.skip_nl();
                self.expect_sym("]")?;
                e = Expr::Index {
                    obj: Box::new(e),
                    idx: Box::new(idx),
                };
                continue;
            }
            if self.at_sym(".") {
                self.bump();
                match self.peek().clone() {
                    Tok::Int(k) => {
                        self.bump();
                        e = Expr::TupleIndex {
                            obj: Box::new(e),
                            k: k as usize,
                        };
                    }
                    Tok::Float(f) => {
                        return self.err(format!(
                            "chained tuple index `.{f}` is not accepted — write `(t.0).1`"
                        ));
                    }
                    Tok::TypeName(t) => {
                        self.bump();
                        // module-qualified type / constructor: m.Type
                        let module = match e {
                            Expr::Ident(m) => m,
                            _ => return self.err("a type name may only follow a module name"),
                        };
                        e = Expr::TypeName {
                            module: Some(module),
                            name: t,
                        };
                        if self.at_sym("{") && !self.no_brace_literal {
                            e = self.record_lit_after_typename(e)?;
                        }
                    }
                    _ => {
                        let name = self.member_name()?;
                        e = Expr::Member {
                            obj: Box::new(e),
                            name,
                        };
                    }
                }
                continue;
            }
            if self.at_sym("!") {
                self.bump();
                e = Expr::Unwrap(Box::new(e));
                continue;
            }
            if self.at_sym("?.") {
                self.bump();
                let name = self.member_name()?;
                e = Expr::OptChain {
                    obj: Box::new(e),
                    name,
                };
                continue;
            }
            if self.at_sym("?") {
                self.bump();
                e = Expr::ToOption(Box::new(e));
                continue;
            }
            if self.at_sym("??") {
                self.bump();
                self.skip_nl();
                let fb = self.unary_expr()?;
                e = Expr::UnwrapOr {
                    expr: Box::new(e),
                    fallback: Box::new(fb),
                };
                continue;
            }
            return Ok(e);
        }
    }

    /// `[T, U]` immediately followed by `(` → explicit type arguments.
    fn try_type_args_call(&mut self) -> PResult<Option<Vec<TypeExpr>>> {
        // scan to the matching `]` and check for `(`
        let mut k = 0;
        let mut depth = 0;
        loop {
            match self.peek_at(k) {
                Tok::Sym("[") => depth += 1,
                Tok::Sym("]") => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                Tok::Eof | Tok::Newline => return Ok(None),
                _ => {}
            }
            k += 1;
        }
        if !matches!(self.peek_at(k + 1), Tok::Sym("(")) {
            return Ok(None);
        }
        // only type-looking content: first token must be a TypeName / `(` / `{`
        if !matches!(
            self.peek_at(1),
            Tok::TypeName(_) | Tok::Sym("(") | Tok::Sym("{")
        ) {
            return Ok(None);
        }
        let save = self.pos;
        match self.type_args() {
            Ok(t) => Ok(Some(t)),
            Err(_) => {
                self.pos = save;
                Ok(None)
            }
        }
    }

    fn call_args(&mut self) -> PResult<Vec<Arg>> {
        self.expect_sym("(")?;
        let mut args = Vec::new();
        let save_flag = self.no_brace_literal;
        self.no_brace_literal = false;
        loop {
            self.skip_nl();
            if self.eat_sym(")") {
                break;
            }
            // named argument `name: expr`
            if matches!(self.peek(), Tok::Ident(_) | Tok::EscIdent(_)) && self.at_sym_n(1, ":") {
                let n = self.member_name()?;
                self.bump();
                self.skip_nl();
                let v = self.expr()?;
                args.push(Arg::Named(n, v));
            } else if matches!(self.peek(), Tok::Ident(s) if s == "_")
                && matches!(self.peek_at(1), Tok::Sym(",") | Tok::Sym(")"))
            {
                self.bump();
                args.push(Arg::Placeholder);
            } else {
                args.push(Arg::Pos(self.expr()?));
            }
            self.skip_nl();
            if !self.eat_sym(",") {
                self.skip_nl();
                self.expect_sym(")")?;
                break;
            }
        }
        self.no_brace_literal = save_flag;
        Ok(args)
    }

    fn primary(&mut self) -> PResult<Expr> {
        let line = self.line();
        match self.peek().clone() {
            Tok::Int(n) => {
                self.bump();
                Ok(if n <= i64::MAX as u64 {
                    Expr::Int(n as i64)
                } else {
                    Expr::BigInt(n)
                })
            }
            Tok::Float(f) => {
                self.bump();
                Ok(Expr::Float(F64(parse_float_text(&f))))
            }
            Tok::PlainStr(s) => {
                self.bump();
                Ok(Expr::Str(vec![StrSeg::Text(s)]))
            }
            Tok::Str(parts) => {
                self.bump();
                let mut segs = Vec::new();
                for p in parts {
                    match p {
                        StrPart::Text(t) => segs.push(StrSeg::Text(t)),
                        StrPart::Interp(src) => {
                            segs.push(StrSeg::Expr(parse_expr_src(&src, line)?))
                        }
                    }
                }
                Ok(Expr::Str(segs))
            }
            Tok::Sym("(") => self.paren_or_lambda(),
            Tok::Sym("[") => self.list_or_map(),
            Tok::Sym("{") => self.block_or_record(),
            Tok::TypeName(t) => {
                self.bump();
                let e = Expr::TypeName {
                    module: None,
                    name: t,
                };
                if self.at_sym("{") && !self.no_brace_literal {
                    return self.record_lit_after_typename(e);
                }
                Ok(e)
            }
            Tok::EscIdent(s) => {
                self.bump();
                Ok(Expr::Ident(s))
            }
            Tok::Ident(s) => self.ident_primary(s),
            other => self.err(format!("expected an expression, got {other:?}")),
        }
    }

    fn ident_primary(&mut self, s: String) -> PResult<Expr> {
        match s.as_str() {
            "true" => {
                self.bump();
                Ok(Expr::Bool(true))
            }
            "false" => {
                self.bump();
                Ok(Expr::Bool(false))
            }
            "none" => {
                self.bump();
                Ok(Expr::None)
            }
            "some" | "ok" | "err" if self.at_sym_n(1, "(") => {
                self.bump();
                self.expect_sym("(")?;
                self.skip_nl();
                let e = self.expr()?;
                self.skip_nl();
                self.expect_sym(")")?;
                Ok(match s.as_str() {
                    "some" => Expr::Some(Box::new(e)),
                    "ok" => Expr::Ok(Box::new(e)),
                    _ => Expr::Err(Box::new(e)),
                })
            }
            "todo" if self.at_sym_n(1, "(") => {
                self.bump();
                self.expect_sym("(")?;
                let msg = match self.peek().clone() {
                    Tok::Str(parts) => {
                        self.bump();
                        parts
                            .iter()
                            .map(|p| {
                                if let StrPart::Text(t) = p {
                                    t.clone()
                                } else {
                                    String::new()
                                }
                            })
                            .collect()
                    }
                    Tok::PlainStr(s) => {
                        self.bump();
                        s
                    }
                    _ => String::new(),
                };
                self.expect_sym(")")?;
                Ok(Expr::Todo(msg))
            }
            "break" => {
                self.bump();
                Ok(Expr::Break)
            }
            "continue" => {
                self.bump();
                Ok(Expr::Continue)
            }
            "_" => {
                self.bump();
                Ok(Expr::Hole)
            }
            "if" => self.if_expr(),
            "match" => self.match_expr(),
            "for" => self.for_expr(),
            "while" => self.while_expr(),
            "fan" => self.fan_expr(),
            _ => {
                if KEYWORDS.contains(&s.as_str()) && s != "self" {
                    return self.err(format!("unexpected keyword `{s}` in expression"));
                }
                self.bump();
                Ok(Expr::Ident(s))
            }
        }
    }

    fn record_lit_after_typename(&mut self, tn: Expr) -> PResult<Expr> {
        let (module, type_name) = match tn {
            Expr::TypeName { module, name } => (module, Some(name)),
            _ => (None, None),
        };
        self.record_body(module, type_name)
    }

    /// `{ ...base, f: e, g: e2 }` / `{ f: e }` — after the TypeName (if any)
    fn record_body(&mut self, module: Option<String>, type_name: Option<String>) -> PResult<Expr> {
        self.expect_sym("{")?;
        let mut spread = None;
        let mut fields = Vec::new();
        loop {
            self.skip_nl();
            if self.eat_sym("}") {
                break;
            }
            if self.eat_sym("...") {
                let e = self.expr()?;
                spread = Some(Box::new(e));
            } else {
                let n = self.member_name()?;
                self.expect_sym(":")?;
                self.skip_nl();
                let e = self.expr()?;
                fields.push((n, e));
            }
            self.skip_nl();
            if !self.eat_sym(",") {
                self.skip_nl();
                self.expect_sym("}")?;
                break;
            }
        }
        Ok(Expr::Record {
            module,
            type_name,
            spread,
            fields,
        })
    }

    fn block_or_record(&mut self) -> PResult<Expr> {
        // `{` Ident `:` … → anonymous record; `{` `...` → spread record; else block
        let mut k = 1;
        while matches!(self.peek_at(k), Tok::Newline) {
            k += 1;
        }
        let is_record = match self.peek_at(k) {
            Tok::Sym("...") => true,
            Tok::Ident(_) | Tok::EscIdent(_) => matches!(self.peek_at(k + 1), Tok::Sym(":")),
            Tok::Sym("}") => false,
            _ => false,
        };
        if is_record {
            return self.record_body(None, None);
        }
        self.block()
    }

    fn list_or_map(&mut self) -> PResult<Expr> {
        self.expect_sym("[")?;
        let save_flag = self.no_brace_literal;
        self.no_brace_literal = false;
        self.skip_nl();
        if self.eat_sym(":") {
            self.skip_nl();
            self.expect_sym("]")?;
            self.no_brace_literal = save_flag;
            return Ok(Expr::EmptyMap);
        }
        if self.eat_sym("]") {
            self.no_brace_literal = save_flag;
            return Ok(Expr::List(vec![]));
        }
        let first = self.expr()?;
        self.skip_nl();
        if self.eat_sym(":") {
            self.skip_nl();
            let v = self.expr()?;
            let mut pairs = vec![(first, v)];
            loop {
                self.skip_nl();
                if !self.eat_sym(",") {
                    break;
                }
                self.skip_nl();
                if self.at_sym("]") {
                    break;
                }
                let k = self.expr()?;
                self.skip_nl();
                self.expect_sym(":")?;
                self.skip_nl();
                let v = self.expr()?;
                pairs.push((k, v));
            }
            self.skip_nl();
            self.expect_sym("]")?;
            self.no_brace_literal = save_flag;
            return Ok(Expr::Map(pairs));
        }
        let mut items = vec![first];
        loop {
            self.skip_nl();
            if !self.eat_sym(",") {
                break;
            }
            self.skip_nl();
            if self.at_sym("]") {
                break;
            }
            items.push(self.expr()?);
        }
        self.skip_nl();
        self.expect_sym("]")?;
        self.no_brace_literal = save_flag;
        Ok(Expr::List(items))
    }

    /// Is the `(` at the current position the start of a lambda parameter list?
    fn lambda_ahead(&self) -> bool {
        let mut k = 0;
        let mut depth = 0;
        loop {
            match self.peek_at(k) {
                Tok::Sym("(") => depth += 1,
                Tok::Sym(")") => {
                    depth -= 1;
                    if depth == 0 {
                        return matches!(self.peek_at(k + 1), Tok::Sym("=>"));
                    }
                }
                Tok::Eof => return false,
                _ => {}
            }
            k += 1;
        }
    }

    fn paren_or_lambda(&mut self) -> PResult<Expr> {
        if self.lambda_ahead() {
            return self.lambda();
        }
        self.expect_sym("(")?;
        let save_flag = self.no_brace_literal;
        self.no_brace_literal = false;
        self.skip_nl();
        if self.eat_sym(")") {
            self.no_brace_literal = save_flag;
            return Ok(Expr::Unit);
        }
        let first = self.expr()?;
        self.skip_nl();
        if self.eat_sym(":") {
            let ty = self.ty()?;
            self.skip_nl();
            self.expect_sym(")")?;
            self.no_brace_literal = save_flag;
            return Ok(Expr::Ascription {
                expr: Box::new(first),
                ty,
            });
        }
        if self.at_sym(",") {
            let mut items = vec![first];
            while self.eat_sym(",") {
                self.skip_nl();
                if self.at_sym(")") {
                    break;
                }
                items.push(self.expr()?);
                self.skip_nl();
            }
            self.expect_sym(")")?;
            self.no_brace_literal = save_flag;
            return Ok(Expr::Tuple(items));
        }
        self.expect_sym(")")?;
        self.no_brace_literal = save_flag;
        Ok(Expr::Paren(Box::new(first)))
    }

    fn lambda(&mut self) -> PResult<Expr> {
        self.expect_sym("(")?;
        let mut params = Vec::new();
        loop {
            self.skip_nl();
            if self.at_sym(")") {
                break;
            }
            if self.at_sym("(") {
                // tuple-destructuring param
                self.bump();
                let mut names = Vec::new();
                loop {
                    self.skip_nl();
                    names.push(self.ident()?);
                    self.skip_nl();
                    if !self.eat_sym(",") {
                        break;
                    }
                }
                self.expect_sym(")")?;
                params.push(LParam { names, ty: None });
            } else {
                let n = self.ident()?;
                let ty = if self.eat_sym(":") {
                    Some(self.ty()?)
                } else {
                    None
                };
                params.push(LParam { names: vec![n], ty });
            }
            self.skip_nl();
            if !self.eat_sym(",") {
                break;
            }
        }
        self.skip_nl();
        self.expect_sym(")")?;
        self.expect_sym("=>")?;
        self.skip_nl();
        let body = self.expr()?;
        Ok(Expr::Lambda {
            params,
            body: Box::new(body),
        })
    }

    fn if_expr(&mut self) -> PResult<Expr> {
        self.expect_kw("if")?;
        if self.at_kw("let") {
            self.bump();
            let name = self.ident()?;
            self.expect_sym("=")?;
            let save = self.no_brace_literal;
            self.no_brace_literal = true;
            let scrut = self.expr()?;
            self.no_brace_literal = save;
            let then = self.block()?;
            self.skip_nl();
            self.expect_kw("else")?;
            self.skip_nl();
            let els = self.block()?;
            return Ok(Expr::IfLet {
                name,
                scrut: Box::new(scrut),
                then: Box::new(then),
                els: Box::new(els),
            });
        }
        let cond = self.expr()?;
        self.skip_nl();
        if !self.eat_kw("then") {
            return self.err("if requires 'then'");
        }
        self.skip_nl();
        let then = self.expr()?;
        // `else` may follow on the same line or the next
        let save = self.pos;
        self.skip_nl();
        if self.eat_kw("else") {
            self.skip_nl();
            let els = self.expr()?;
            return Ok(Expr::If {
                cond: Box::new(cond),
                then: Box::new(then),
                els: Some(Box::new(els)),
            });
        }
        self.pos = save;
        Ok(Expr::If {
            cond: Box::new(cond),
            then: Box::new(then),
            els: None,
        })
    }

    fn match_expr(&mut self) -> PResult<Expr> {
        self.expect_kw("match")?;
        let save = self.no_brace_literal;
        self.no_brace_literal = true;
        let subject = self.expr()?;
        self.no_brace_literal = save;
        self.skip_nl();
        let arms = self.match_arms()?;
        Ok(Expr::Match {
            subject: Box::new(subject),
            arms,
        })
    }

    fn match_arms(&mut self) -> PResult<Vec<MatchArm>> {
        self.expect_sym("{")?;
        let mut arms = Vec::new();
        loop {
            self.skip_nl();
            if self.eat_sym("}") {
                break;
            }
            let pat = self.pattern()?;
            let guard = if self.eat_kw("if") {
                Some(self.expr()?)
            } else {
                None
            };
            self.expect_sym("=>")?;
            self.skip_nl();
            let body = self.expr()?;
            arms.push(MatchArm { pat, guard, body });
            if !(self.eat_sym(",") || self.at_nl() || self.at_sym("}")) {
                return self.err(format!(
                    "expected `,` or newline after match arm, got {:?}",
                    self.peek()
                ));
            }
        }
        Ok(arms)
    }

    fn for_expr(&mut self) -> PResult<Expr> {
        self.expect_kw("for")?;
        let mut binders = Vec::new();
        if self.eat_sym("(") {
            loop {
                self.skip_nl();
                binders.push(self.ident()?);
                self.skip_nl();
                if !self.eat_sym(",") {
                    break;
                }
            }
            self.expect_sym(")")?;
        } else {
            binders.push(self.ident()?);
        }
        self.expect_kw("in")?;
        let save = self.no_brace_literal;
        self.no_brace_literal = true;
        let iter = self.expr()?;
        self.no_brace_literal = save;
        self.skip_nl();
        let body = self.block()?;
        Ok(Expr::For {
            binders,
            iter: Box::new(iter),
            body: Box::new(body),
        })
    }

    fn while_expr(&mut self) -> PResult<Expr> {
        self.expect_kw("while")?;
        let save = self.no_brace_literal;
        self.no_brace_literal = true;
        let cond = self.expr()?;
        self.no_brace_literal = save;
        self.skip_nl();
        let body = self.block()?;
        Ok(Expr::While {
            cond: Box::new(cond),
            body: Box::new(body),
        })
    }

    fn fan_expr(&mut self) -> PResult<Expr> {
        // `fan { … }`, `fan.any { … }`, `fan.settle { … }`, `fan.bounded(b) { … }`,
        // or a plain call `fan.map(xs, f)` (falls through to the postfix machinery)
        self.expect_kw("fan")?;
        if self.at_sym("{") {
            let arms = self.fan_arms()?;
            return Ok(Expr::Fan {
                head: None,
                head_args: vec![],
                arms,
            });
        }
        if self.at_sym(".") {
            let save = self.pos;
            self.bump();
            let head = self.member_name()?;
            let mut head_args = Vec::new();
            if self.at_sym("(") {
                let args = self.call_args()?;
                let mut is_block_head = false;
                if self.at_sym("{") {
                    is_block_head = true;
                }
                if !is_block_head {
                    // plain call form
                    let line = self.line();
                    let callee = Expr::Member {
                        obj: Box::new(Expr::Ident("fan".into())),
                        name: head,
                    };
                    return Ok(Expr::Call {
                        callee: Box::new(callee),
                        type_args: vec![],
                        args,
                        line,
                    });
                }
                for a in args {
                    match a {
                        Arg::Pos(e) => head_args.push(e),
                        _ => return self.err("fan head arguments must be positional"),
                    }
                }
            }
            if self.at_sym("{") {
                let arms = if head == "bounded" {
                    vec![self.block()?]
                } else {
                    self.fan_arms()?
                };
                return Ok(Expr::Fan {
                    head: Some(head),
                    head_args,
                    arms,
                });
            }
            self.pos = save;
        }
        Ok(Expr::Ident("fan".into()))
    }

    fn fan_arms(&mut self) -> PResult<Vec<Expr>> {
        self.expect_sym("{")?;
        let mut arms = Vec::new();
        loop {
            self.skip_nl();
            if self.eat_sym("}") {
                break;
            }
            arms.push(self.expr()?);
            if !(self.eat_sym(",") || self.at_nl() || self.at_sym("}")) {
                return self.err("fan arms are separated by `,` or a newline");
            }
        }
        Ok(arms)
    }

    // ── patterns ─────────────────────────────────────────────────────────
    fn pattern(&mut self) -> PResult<Pattern> {
        match self.peek().clone() {
            Tok::Ident(s) => match s.as_str() {
                "_" => {
                    self.bump();
                    Ok(Pattern::Wild)
                }
                "true" => {
                    self.bump();
                    Ok(Pattern::Bool(true))
                }
                "false" => {
                    self.bump();
                    Ok(Pattern::Bool(false))
                }
                "none" => {
                    self.bump();
                    Ok(Pattern::None)
                }
                "some" | "ok" | "err" if self.at_sym_n(1, "(") => {
                    self.bump();
                    self.expect_sym("(")?;
                    let inner = self.pattern()?;
                    self.expect_sym(")")?;
                    Ok(match s.as_str() {
                        "some" => Pattern::Some(Box::new(inner)),
                        "ok" => Pattern::Ok(Box::new(inner)),
                        _ => Pattern::Err(Box::new(inner)),
                    })
                }
                _ if self.at_sym_n(1, ".") && matches!(self.peek_at(2), Tok::TypeName(_)) => {
                    self.bump();
                    self.bump();
                    self.ctor_pattern(Some(s))
                }
                _ => {
                    let n = self.ident()?;
                    Ok(Pattern::Bind(n))
                }
            },
            Tok::EscIdent(s) => {
                self.bump();
                Ok(Pattern::Bind(s))
            }
            Tok::Int(n) => {
                self.bump();
                if n > i64::MAX as u64 {
                    return self.err("integer pattern out of range");
                }
                Ok(Pattern::Int(n as i64))
            }
            Tok::Float(f) => {
                self.bump();
                Ok(Pattern::Float(F64(parse_float_text(&f))))
            }
            Tok::Sym("-") => {
                self.bump();
                match self.peek().clone() {
                    Tok::Int(n) => {
                        self.bump();
                        if n == (i64::MAX as u64) + 1 {
                            return Ok(Pattern::Int(i64::MIN));
                        }
                        if n > i64::MAX as u64 {
                            return self.err("integer pattern out of range");
                        }
                        Ok(Pattern::Int(-(n as i64)))
                    }
                    Tok::Float(f) => {
                        self.bump();
                        Ok(Pattern::Float(F64(-parse_float_text(&f))))
                    }
                    _ => self.err("expected a number after `-` in pattern"),
                }
            }
            Tok::Str(parts) => {
                self.bump();
                let mut s = String::new();
                for p in parts {
                    match p {
                        StrPart::Text(t) => s.push_str(&t),
                        StrPart::Interp(_) => {
                            return self.err("interpolation is not allowed in a pattern")
                        }
                    }
                }
                Ok(Pattern::Str(s))
            }
            Tok::PlainStr(s) => {
                self.bump();
                Ok(Pattern::Str(s))
            }
            Tok::TypeName(_) => self.ctor_pattern(None),
            Tok::Sym("(") => {
                self.bump();
                let mut items = Vec::new();
                loop {
                    self.skip_nl();
                    items.push(self.pattern()?);
                    self.skip_nl();
                    if !self.eat_sym(",") {
                        break;
                    }
                }
                self.expect_sym(")")?;
                Ok(Pattern::Tuple(items))
            }
            Tok::Sym("[") => {
                self.bump();
                let mut items = Vec::new();
                self.skip_nl();
                while !self.at_sym("]") {
                    items.push(self.pattern()?);
                    self.skip_nl();
                    if !self.eat_sym(",") {
                        break;
                    }
                    self.skip_nl();
                }
                self.expect_sym("]")?;
                Ok(Pattern::List(items))
            }
            other => self.err(format!("expected a pattern, got {other:?}")),
        }
    }

    fn ctor_pattern(&mut self, module: Option<String>) -> PResult<Pattern> {
        let name = self.type_name()?;
        if self.at_sym("(") {
            self.bump();
            let mut args = Vec::new();
            loop {
                self.skip_nl();
                args.push(self.pattern()?);
                self.skip_nl();
                if !self.eat_sym(",") {
                    break;
                }
            }
            self.expect_sym(")")?;
            return Ok(Pattern::Ctor { module, name, args });
        }
        if self.at_sym("{") {
            self.bump();
            let mut fields = Vec::new();
            let mut rest = false;
            loop {
                self.skip_nl();
                if self.eat_sym("}") {
                    break;
                }
                if self.eat_sym("..") {
                    rest = true;
                    self.skip_nl();
                    self.eat_sym(",");
                    self.skip_nl();
                    self.expect_sym("}")?;
                    break;
                }
                let n = self.member_name()?;
                let sub = if self.eat_sym(":") {
                    Some(self.pattern()?)
                } else {
                    None
                };
                fields.push((n, sub));
                self.skip_nl();
                if !self.eat_sym(",") {
                    self.skip_nl();
                    self.expect_sym("}")?;
                    break;
                }
            }
            return Ok(Pattern::CtorRecord {
                module,
                name,
                fields,
                rest,
            });
        }
        Ok(Pattern::Ctor {
            module,
            name,
            args: vec![],
        })
    }
}

fn tok_text(t: &Tok) -> String {
    match t {
        Tok::Ident(s) | Tok::EscIdent(s) | Tok::TypeName(s) => s.clone(),
        Tok::Int(n) => n.to_string(),
        Tok::Float(f) => f.clone(),
        Tok::Str(parts) => parts
            .iter()
            .map(|p| match p {
                StrPart::Text(t) => format!("{t:?}"),
                StrPart::Interp(s) => format!("${{{s}}}"),
            })
            .collect(),
        Tok::PlainStr(s) => format!("{s:?}"),
        Tok::Sym(s) => s.to_string(),
        Tok::Newline => "\n".into(),
        Tok::Eof => String::new(),
    }
}

/// Float literal text → binary64: the evaluator's own correctly rounded
/// conversion (ALS-T2 semantics; fmtfloat::parse_decimal, exact big-integer
/// rounding for any digit count).
pub fn parse_float_text(text: &str) -> f64 {
    let (mant, exp) = match text.find('e') {
        Some(i) => (&text[..i], text[i + 1..].parse_i64_manual()),
        None => (text, 0i64),
    };
    let (int_part, frac_part) = match mant.find('.') {
        Some(i) => (&mant[..i], &mant[i + 1..]),
        None => (mant, ""),
    };
    let digits: String = int_part.chars().chain(frac_part.chars()).collect();
    let scale = exp - frac_part.len() as i64;
    crate::fmtfloat::parse_decimal(&digits, scale)
}

trait ParseI64Manual {
    fn parse_i64_manual(&self) -> i64;
}
impl ParseI64Manual for str {
    fn parse_i64_manual(&self) -> i64 {
        let mut neg = false;
        let mut v: i64 = 0;
        for (i, c) in self.chars().enumerate() {
            if i == 0 && c == '-' {
                neg = true;
                continue;
            }
            if i == 0 && c == '+' {
                continue;
            }
            v = v * 10 + (c as i64 - '0' as i64);
        }
        if neg {
            -v
        } else {
            v
        }
    }
}
