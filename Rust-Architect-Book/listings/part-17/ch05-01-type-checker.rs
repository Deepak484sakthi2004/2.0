// verify: debug ok
// Listing 17.5-1: a bidirectional type checker for Ore. `infer` computes a type bottom-up;
// `check` pushes an expected type down into `if` branches and block tails, so a mismatch is
// reported at the innermost wrong expression. An `{error}` type stops one mistake from cascading.
// (Names are looked up in scoped environments here: resolution and checking are fused, as small
// compilers often do. Listing 17.4-3 shows resolution as its own pass.)
// The lexer and parser are listing 17.3-1's, unchanged (printing helpers included).
#![allow(dead_code)]

use std::fmt::Write as _;

// ---------------- lexer (condensed from listing 17.2-1) ----------------
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span { pub lo: u32, pub hi: u32 }

impl Span {
    fn to(self, other: Span) -> Span {
        Span { lo: self.lo, hi: other.hi }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Int(i64), Ident(String),
    Fn, Let, Mut, If, Else, While, Return, True, False,
    LParen, RParen, LBrace, RBrace, Comma, Semi, Colon, Arrow,
    Plus, Minus, Star, Slash, Percent, Assign, EqEq, Ne, Lt, Le, Gt, Ge, AndAnd, OrOr, Bang,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token { pub tok: Tok, pub span: Span }

#[derive(Debug)]
pub struct ParseError { pub span: Span, pub msg: String }

pub fn lex(src: &str) -> Result<Vec<Token>, ParseError> {
    let b = src.as_bytes();
    let (mut i, mut out) = (0usize, Vec::new());
    while i < b.len() {
        let (c, start) = (b[i], i);
        if c.is_ascii_whitespace() { i += 1; continue; }
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' { i += 1; }
            continue;
        }
        let tok = if c.is_ascii_digit() {
            while i < b.len() && b[i].is_ascii_digit() { i += 1; }
            match src[start..i].parse() {
                Ok(n) => Tok::Int(n),
                Err(_) => return Err(ParseError { span: Span { lo: start as u32, hi: i as u32 }, msg: "integer too large".into() }),
            }
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') { i += 1; }
            match &src[start..i] {
                "fn" => Tok::Fn, "let" => Tok::Let, "mut" => Tok::Mut, "if" => Tok::If, "else" => Tok::Else,
                "while" => Tok::While, "return" => Tok::Return, "true" => Tok::True, "false" => Tok::False,
                w => Tok::Ident(w.to_string()),
            }
        } else {
            let (t, len) = match (c, b.get(i + 1).copied()) {
                (b'-', Some(b'>')) => (Tok::Arrow, 2), (b'=', Some(b'=')) => (Tok::EqEq, 2),
                (b'!', Some(b'=')) => (Tok::Ne, 2), (b'<', Some(b'=')) => (Tok::Le, 2),
                (b'>', Some(b'=')) => (Tok::Ge, 2), (b'&', Some(b'&')) => (Tok::AndAnd, 2),
                (b'|', Some(b'|')) => (Tok::OrOr, 2),
                (b'(', _) => (Tok::LParen, 1), (b')', _) => (Tok::RParen, 1), (b'{', _) => (Tok::LBrace, 1),
                (b'}', _) => (Tok::RBrace, 1), (b',', _) => (Tok::Comma, 1), (b';', _) => (Tok::Semi, 1),
                (b':', _) => (Tok::Colon, 1), (b'+', _) => (Tok::Plus, 1), (b'-', _) => (Tok::Minus, 1),
                (b'*', _) => (Tok::Star, 1), (b'/', _) => (Tok::Slash, 1), (b'%', _) => (Tok::Percent, 1),
                (b'=', _) => (Tok::Assign, 1), (b'<', _) => (Tok::Lt, 1), (b'>', _) => (Tok::Gt, 1),
                (b'!', _) => (Tok::Bang, 1),
                _ => return Err(ParseError { span: Span { lo: i as u32, hi: i as u32 + 1 }, msg: "unexpected character".into() }),
            };
            i += len;
            t
        };
        out.push(Token { tok, span: Span { lo: start as u32, hi: i as u32 } });
    }
    out.push(Token { tok: Tok::Eof, span: Span { lo: b.len() as u32, hi: b.len() as u32 } });
    Ok(out)
}

// ---------------- AST ----------------
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp { Add, Sub, Mul, Div, Rem, Eq, Ne, Lt, Le, Gt, Ge, And, Or }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp { Neg, Not }

#[derive(Debug)]
pub struct Expr { pub kind: ExprKind, pub span: Span }

#[derive(Debug)]
pub enum ExprKind {
    Int(i64),
    Bool(bool),
    Var(String),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Assign(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    If(Box<Expr>, Block, Option<Box<Expr>>), // else branch: a Block expr or another If
    While(Box<Expr>, Block),
    Block(Block),
    Return(Option<Box<Expr>>),
}

#[derive(Debug)]
pub struct Block { pub stmts: Vec<Stmt>, pub tail: Option<Box<Expr>>, pub span: Span }

#[derive(Debug)]
pub enum Stmt {
    Let { name: String, mutable: bool, ty: Option<String>, init: Expr, span: Span },
    Expr(Expr),
}

#[derive(Debug)]
pub struct Param { pub name: String, pub ty: Option<String>, pub span: Span }

#[derive(Debug)]
pub struct FnDecl { pub name: String, pub params: Vec<Param>, pub ret: Option<String>, pub body: Block, pub span: Span }

// ---------------- parser ----------------
pub struct Parser { toks: Vec<Token>, at: usize }

type PResult<T> = Result<T, ParseError>;

impl Parser {
    pub fn new(toks: Vec<Token>) -> Self {
        Parser { toks, at: 0 }
    }
    fn peek(&self) -> &Tok {
        &self.toks[self.at].tok
    }
    fn span(&self) -> Span {
        self.toks[self.at].span
    }
    fn bump(&mut self) -> Token {
        let t = self.toks[self.at].clone();
        if t.tok != Tok::Eof { self.at += 1; }
        t
    }
    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == t { self.bump(); true } else { false }
    }
    fn expect(&mut self, t: &Tok, what: &str) -> PResult<Token> {
        if self.peek() == t { Ok(self.bump()) } else { self.error(format!("expected {what}")) }
    }
    fn error<T>(&self, msg: impl Into<String>) -> PResult<T> {
        Err(ParseError { span: self.span(), msg: format!("{}, found {:?}", msg.into(), self.peek()) })
    }
    fn ident(&mut self) -> PResult<(String, Span)> {
        match self.peek().clone() {
            Tok::Ident(name) => Ok((name, self.bump().span)),
            _ => self.error("expected a name"),
        }
    }

    // program := fn_decl*
    pub fn program(&mut self) -> PResult<Vec<FnDecl>> {
        let mut items = Vec::new();
        while *self.peek() != Tok::Eof {
            items.push(self.fn_decl()?);
        }
        Ok(items)
    }

    // fn_decl := "fn" IDENT "(" (param ("," param)*)? ")" ("->" IDENT)? block
    fn fn_decl(&mut self) -> PResult<FnDecl> {
        let lo = self.expect(&Tok::Fn, "`fn`")?.span;
        let (name, _) = self.ident()?;
        self.expect(&Tok::LParen, "`(`")?;
        let mut params = Vec::new();
        while *self.peek() != Tok::RParen {
            let (pname, pspan) = self.ident()?;
            let ty = if self.eat(&Tok::Colon) { Some(self.ident()?.0) } else { None };
            params.push(Param { name: pname, ty, span: pspan });
            if !self.eat(&Tok::Comma) { break; }
        }
        self.expect(&Tok::RParen, "`)`")?;
        let ret = if self.eat(&Tok::Arrow) { Some(self.ident()?.0) } else { None };
        let body = self.block()?;
        Ok(FnDecl { name, params, ret, span: lo.to(body.span), body })
    }

    // block := "{" stmt* expr? "}"
    fn block(&mut self) -> PResult<Block> {
        let lo = self.expect(&Tok::LBrace, "`{`")?.span;
        let (mut stmts, mut tail) = (Vec::new(), None);
        loop {
            match self.peek() {
                Tok::RBrace => break,
                Tok::Let => stmts.push(self.let_stmt()?),
                _ => {
                    let e = self.expr()?;
                    let block_like = matches!(e.kind, ExprKind::If(..) | ExprKind::While(..) | ExprKind::Block(_));
                    if self.eat(&Tok::Semi) {
                        stmts.push(Stmt::Expr(e));
                    } else if *self.peek() == Tok::RBrace {
                        tail = Some(Box::new(e));
                        break;
                    } else if block_like {
                        stmts.push(Stmt::Expr(e)); // `if ... {}` needs no `;`, as in Rust
                    } else {
                        return self.error("expected `;` or `}` after expression");
                    }
                }
            }
        }
        let hi = self.expect(&Tok::RBrace, "`}`")?.span;
        Ok(Block { stmts, tail, span: lo.to(hi) })
    }

    // let_stmt := "let" "mut"? IDENT (":" IDENT)? "=" expr ";"
    fn let_stmt(&mut self) -> PResult<Stmt> {
        let lo = self.bump().span;
        let mutable = self.eat(&Tok::Mut);
        let (name, _) = self.ident()?;
        let ty = if self.eat(&Tok::Colon) { Some(self.ident()?.0) } else { None };
        self.expect(&Tok::Assign, "`=`")?;
        let init = self.expr()?;
        let hi = self.expect(&Tok::Semi, "`;`")?.span;
        Ok(Stmt::Let { name, mutable, ty, init, span: lo.to(hi) })
    }

    // expr := assign
    pub fn expr(&mut self) -> PResult<Expr> {
        self.assign()
    }

    // assign := or ("=" assign)?          right-associative: a = b = c is a = (b = c)
    fn assign(&mut self) -> PResult<Expr> {
        let lhs = self.or()?;
        if *self.peek() == Tok::Assign {
            self.bump();
            let rhs = self.assign()?;
            if !matches!(lhs.kind, ExprKind::Var(_)) {
                return Err(ParseError { span: lhs.span, msg: "invalid left-hand side of assignment".into() });
            }
            let span = lhs.span.to(rhs.span);
            return Ok(Expr { kind: ExprKind::Assign(Box::new(lhs), Box::new(rhs)), span });
        }
        Ok(lhs)
    }

    /// One left-associative precedence level: operand (op operand)*.
    fn left_assoc(&mut self, ops: &[(Tok, BinOp)], operand: fn(&mut Self) -> PResult<Expr>) -> PResult<Expr> {
        let mut lhs = operand(self)?;
        while let Some(&(_, op)) = ops.iter().find(|(t, _)| t == self.peek()) {
            self.bump();
            let rhs = operand(self)?;
            let span = lhs.span.to(rhs.span);
            lhs = Expr { kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), span };
        }
        Ok(lhs)
    }

    fn or(&mut self) -> PResult<Expr> {
        self.left_assoc(&[(Tok::OrOr, BinOp::Or)], Self::and)
    }
    fn and(&mut self) -> PResult<Expr> {
        self.left_assoc(&[(Tok::AndAnd, BinOp::And)], Self::cmp)
    }

    // cmp := add (cmp_op add)?            non-associative, as in Rust: a < b < c is an error
    fn cmp(&mut self) -> PResult<Expr> {
        let lhs = self.add()?;
        let op = match self.peek() {
            Tok::EqEq => BinOp::Eq, Tok::Ne => BinOp::Ne, Tok::Lt => BinOp::Lt,
            Tok::Le => BinOp::Le, Tok::Gt => BinOp::Gt, Tok::Ge => BinOp::Ge,
            _ => return Ok(lhs),
        };
        self.bump();
        let rhs = self.add()?;
        if matches!(self.peek(), Tok::EqEq | Tok::Ne | Tok::Lt | Tok::Le | Tok::Gt | Tok::Ge) {
            return self.error("comparison operators cannot be chained; use `&&`");
        }
        let span = lhs.span.to(rhs.span);
        Ok(Expr { kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), span })
    }

    fn add(&mut self) -> PResult<Expr> {
        self.left_assoc(&[(Tok::Plus, BinOp::Add), (Tok::Minus, BinOp::Sub)], Self::mul)
    }
    fn mul(&mut self) -> PResult<Expr> {
        self.left_assoc(&[(Tok::Star, BinOp::Mul), (Tok::Slash, BinOp::Div), (Tok::Percent, BinOp::Rem)], Self::unary)
    }

    // unary := ("-" | "!") unary | postfix
    fn unary(&mut self) -> PResult<Expr> {
        let op = match self.peek() {
            Tok::Minus => UnOp::Neg,
            Tok::Bang => UnOp::Not,
            _ => return self.postfix(),
        };
        let lo = self.bump().span;
        let e = self.unary()?;
        let span = lo.to(e.span);
        Ok(Expr { kind: ExprKind::Unary(op, Box::new(e)), span })
    }

    // postfix := primary ("(" args ")")*
    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        while self.eat(&Tok::LParen) {
            let mut args = Vec::new();
            while *self.peek() != Tok::RParen {
                args.push(self.expr()?);
                if !self.eat(&Tok::Comma) { break; }
            }
            let hi = self.expect(&Tok::RParen, "`)` after arguments")?.span;
            let span = e.span.to(hi);
            e = Expr { kind: ExprKind::Call(Box::new(e), args), span };
        }
        Ok(e)
    }

    fn primary(&mut self) -> PResult<Expr> {
        let span = self.span();
        let kind = match self.peek().clone() {
            Tok::Int(n) => { self.bump(); ExprKind::Int(n) }
            Tok::True => { self.bump(); ExprKind::Bool(true) }
            Tok::False => { self.bump(); ExprKind::Bool(false) }
            Tok::Ident(name) => { self.bump(); ExprKind::Var(name) }
            Tok::LParen => {
                self.bump();
                let e = self.expr()?;
                self.expect(&Tok::RParen, "`)`")?;
                return Ok(e);
            }
            Tok::LBrace => {
                let b = self.block()?;
                return Ok(Expr { span: b.span, kind: ExprKind::Block(b) });
            }
            Tok::If => return self.if_expr(),
            Tok::While => {
                self.bump();
                let cond = self.expr()?;
                let body = self.block()?;
                return Ok(Expr { span: span.to(body.span), kind: ExprKind::While(Box::new(cond), body) });
            }
            Tok::Return => {
                self.bump();
                if matches!(self.peek(), Tok::Semi | Tok::RBrace) {
                    ExprKind::Return(None)
                } else {
                    let e = self.expr()?;
                    return Ok(Expr { span: span.to(e.span), kind: ExprKind::Return(Some(Box::new(e))) });
                }
            }
            _ => return self.error("expected an expression"),
        };
        Ok(Expr { kind, span })
    }

    // if_expr := "if" expr block ("else" (block | if_expr))?
    fn if_expr(&mut self) -> PResult<Expr> {
        let lo = self.bump().span;
        let cond = self.expr()?;
        let then = self.block()?;
        let mut span = lo.to(then.span);
        let els = if self.eat(&Tok::Else) {
            let e = if *self.peek() == Tok::If {
                self.if_expr()?
            } else {
                let b = self.block()?;
                Expr { span: b.span, kind: ExprKind::Block(b) }
            };
            span = span.to(e.span);
            Some(Box::new(e))
        } else {
            None
        };
        Ok(Expr { kind: ExprKind::If(Box::new(cond), then, els), span })
    }
}

// ---------------- printing ----------------
fn op_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*", BinOp::Div => "/", BinOp::Rem => "%",
        BinOp::Eq => "==", BinOp::Ne => "!=", BinOp::Lt => "<", BinOp::Le => "<=", BinOp::Gt => ">",
        BinOp::Ge => ">=", BinOp::And => "&&", BinOp::Or => "||",
    }
}

pub fn sexpr(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Int(n) => n.to_string(),
        ExprKind::Bool(b) => b.to_string(),
        ExprKind::Var(v) => v.clone(),
        ExprKind::Unary(op, x) => format!("({} {})", if *op == UnOp::Neg { "-" } else { "!" }, sexpr(x)),
        ExprKind::Binary(op, a, b) => format!("({} {} {})", op_str(*op), sexpr(a), sexpr(b)),
        ExprKind::Assign(a, b) => format!("(= {} {})", sexpr(a), sexpr(b)),
        ExprKind::Call(f, args) => {
            let mut s = format!("(call {}", sexpr(f));
            for a in args { write!(s, " {}", sexpr(a)).unwrap(); }
            s + ")"
        }
        ExprKind::If(c, t, e) => match e {
            Some(e) => format!("(if {} {} {})", sexpr(c), block_str(t), sexpr(e)),
            None => format!("(if {} {})", sexpr(c), block_str(t)),
        },
        ExprKind::While(c, b) => format!("(while {} {})", sexpr(c), block_str(b)),
        ExprKind::Block(b) => block_str(b),
        ExprKind::Return(e) => match e {
            Some(e) => format!("(return {})", sexpr(e)),
            None => "(return)".into(),
        },
    }
}

fn block_str(b: &Block) -> String {
    let mut s = String::from("(block");
    for st in &b.stmts {
        match st {
            Stmt::Let { name, mutable, ty, init, .. } => {
                let m = if *mutable { "mut " } else { "" };
                let t = ty.as_ref().map(|t| format!(":{t}")).unwrap_or_default();
                write!(s, " (let {m}{name}{t} {})", sexpr(init)).unwrap();
            }
            Stmt::Expr(e) => write!(s, " {}", sexpr(e)).unwrap(),
        }
    }
    if let Some(t) = &b.tail { write!(s, " {}", sexpr(t)).unwrap(); }
    s + ")"
}

pub fn parse_program(src: &str) -> PResult<Vec<FnDecl>> {
    Parser::new(lex(src)?).program()
}


// ---------------- type checking (bidirectional: `infer` synthesizes, `check` pushes an expectation) ----------------
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Int,
    Bool,
    Unit,
    Never, // the type of `return`: coerces to every type, like Rust's `!`
    Error, // an already-reported error: compatible with everything, so errors don't cascade
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            Ty::Int => "int", Ty::Bool => "bool", Ty::Unit => "()", Ty::Never => "!", Ty::Error => "{error}",
        })
    }
}

fn compatible(found: &Ty, expected: &Ty) -> bool {
    found == expected || matches!(found, Ty::Never | Ty::Error) || *expected == Ty::Error
}

struct Sig { params: Vec<Ty>, ret: Ty }

pub struct Checker<'a> {
    src: &'a str,
    fns: HashMap<String, Sig>,
    scopes: Vec<HashMap<String, Ty>>,
    ret: Ty,
    pub errors: Vec<(Span, String)>,
    pub let_types: Vec<(String, Ty)>,
}

fn ann(name: Option<&String>) -> Result<Ty, String> {
    match name.map(|s| s.as_str()) {
        None => Ok(Ty::Unit),
        Some("int") => Ok(Ty::Int),
        Some("bool") => Ok(Ty::Bool),
        Some(other) => Err(format!("unknown type `{other}`")),
    }
}

impl<'a> Checker<'a> {
    pub fn new(src: &'a str, items: &[FnDecl]) -> Self {
        let mut fns = HashMap::new();
        for f in items {
            let params = f.params.iter().map(|p| ann(p.ty.as_ref()).unwrap_or(Ty::Error)).collect();
            fns.insert(f.name.clone(), Sig { params, ret: ann(f.ret.as_ref()).unwrap_or(Ty::Error) });
        }
        Checker { src, fns, scopes: Vec::new(), ret: Ty::Unit, errors: Vec::new(), let_types: Vec::new() }
    }

    fn error(&mut self, span: Span, msg: String) -> Ty {
        self.errors.push((span, msg));
        Ty::Error
    }

    fn text(&self, span: Span) -> &str {
        &self.src[span.lo as usize..span.hi as usize]
    }

    pub fn check_fn(&mut self, f: &FnDecl) {
        let sig = &self.fns[&f.name];
        self.ret = sig.ret.clone();
        let params: HashMap<String, Ty> =
            f.params.iter().zip(sig.params.clone()).map(|(p, t)| (p.name.clone(), t)).collect();
        self.scopes = vec![params];
        let ret = self.ret.clone();
        self.check_block(&f.body, &ret);
    }

    /// Checking mode: the expectation flows DOWN into branches and tails, so a mismatch is
    /// reported at the innermost expression that is wrong.
    fn check(&mut self, e: &Expr, expected: &Ty) {
        match &e.kind {
            ExprKind::If(c, then, els) => {
                self.check(c, &Ty::Bool);
                self.check_block(then, expected);
                match els {
                    Some(els) => self.check(els, expected),
                    None if !compatible(&Ty::Unit, expected) => {
                        self.error(e.span, format!("`if` without `else` has type (), expected {expected}"));
                    }
                    None => {}
                }
            }
            ExprKind::Block(b) => self.check_block(b, expected),
            _ => {
                let found = self.infer(e);
                if !compatible(&found, expected) {
                    self.error(e.span, format!("mismatched types: expected {expected}, found {found}"));
                }
            }
        }
    }

    fn check_block(&mut self, b: &Block, expected: &Ty) {
        self.scopes.push(HashMap::new());
        let diverges = self.stmts(&b.stmts);
        match &b.tail {
            Some(t) => self.check(t, expected),
            None if !diverges && !compatible(&Ty::Unit, expected) => {
                self.error(Span { lo: b.span.hi - 1, hi: b.span.hi }, format!("block has type (), expected {expected}"));
            }
            None => {}
        }
        self.scopes.pop();
    }

    fn infer_block(&mut self, b: &Block) -> Ty {
        self.scopes.push(HashMap::new());
        let diverges = self.stmts(&b.stmts);
        let t = match &b.tail {
            Some(t) => self.infer(t),
            None if diverges => Ty::Never,
            None => Ty::Unit,
        };
        self.scopes.pop();
        t
    }

    /// Checks a block's statements in the current scope; returns true if one of them diverges.
    fn stmts(&mut self, stmts: &[Stmt]) -> bool {
        let mut diverges = false;
        for s in stmts {
            match s {
                Stmt::Let { name, ty, init, .. } => {
                    let t = match ann(ty.as_ref()) {
                        Ok(Ty::Unit) if ty.is_none() => self.infer(init), // no annotation: synthesize
                        Ok(t) => { self.check(init, &t); t }             // annotation: check against it
                        Err(msg) => self.error(init.span, msg),
                    };
                    self.let_types.push((name.clone(), t.clone()));
                    self.scopes.last_mut().unwrap().insert(name.clone(), t);
                }
                Stmt::Expr(e) => {
                    diverges |= self.infer(e) == Ty::Never;
                }
            }
        }
        diverges
    }

    fn lookup(&self, name: &str) -> Option<Ty> {
        self.scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }

    /// Synthesis mode: compute the type from the expression alone.
    fn infer(&mut self, e: &Expr) -> Ty {
        match &e.kind {
            ExprKind::Int(_) => Ty::Int,
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::Var(name) => match self.lookup(name) {
                Some(t) => t,
                None => self.error(e.span, format!("cannot find value `{name}`")),
            },
            ExprKind::Unary(op, x) => {
                let want = if *op == UnOp::Neg { Ty::Int } else { Ty::Bool };
                self.check(x, &want);
                want
            }
            ExprKind::Binary(op, a, b) => {
                let (ta, tb) = (self.infer(a), self.infer(b));
                if ta == Ty::Error || tb == Ty::Error {
                    return Ty::Error; // already reported: stay quiet
                }
                let (operands, result) = match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => (Ty::Int, Ty::Int),
                    BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => (Ty::Int, Ty::Bool),
                    BinOp::And | BinOp::Or => (Ty::Bool, Ty::Bool),
                    BinOp::Eq | BinOp::Ne => {
                        if !compatible(&tb, &ta) {
                            return self.error(e.span, format!("cannot compare {ta} with {tb}"));
                        }
                        return Ty::Bool;
                    }
                };
                if !compatible(&ta, &operands) || !compatible(&tb, &operands) {
                    let op_text = self.text(Span { lo: a.span.hi, hi: b.span.lo }).trim().to_string();
                    return self.error(e.span, format!("no implementation for `{ta} {op_text} {tb}`"));
                }
                result
            }
            ExprKind::Assign(lhs, rhs) => {
                let t = self.infer(lhs);
                self.check(rhs, &t);
                Ty::Unit
            }
            ExprKind::Call(callee, args) => {
                let name = match &callee.kind {
                    ExprKind::Var(n) => n.clone(),
                    _ => return self.error(callee.span, "only named functions can be called".into()),
                };
                let Some(sig) = self.fns.get(&name) else {
                    return self.error(callee.span, format!("cannot find function `{name}`"));
                };
                let (params, ret) = (sig.params.clone(), sig.ret.clone());
                if params.len() != args.len() {
                    return self.error(e.span, format!(
                        "this function takes {} argument{} but {} {} supplied",
                        params.len(), if params.len() == 1 { "" } else { "s" },
                        args.len(), if args.len() == 1 { "was" } else { "were" }));
                }
                for (a, p) in args.iter().zip(&params) {
                    self.check(a, p);
                }
                ret
            }
            ExprKind::If(c, then, els) => {
                self.check(c, &Ty::Bool);
                match els {
                    None => { self.check_block(then, &Ty::Unit); Ty::Unit }
                    Some(els) => {
                        // synthesize from the then-branch, then check the else-branch against it
                        let t = self.infer_block(then);
                        self.check(els, &t);
                        t
                    }
                }
            }
            ExprKind::While(c, body) => {
                self.check(c, &Ty::Bool);
                self.check_block(body, &Ty::Unit);
                Ty::Unit
            }
            ExprKind::Block(b) => self.infer_block(b),
            ExprKind::Return(value) => {
                let ret = self.ret.clone();
                match value {
                    Some(v) => self.check(v, &ret),
                    None if ret != Ty::Unit => { self.error(e.span, format!("`return;` in a function returning {ret}")); }
                    None => {}
                }
                Ty::Never
            }
        }
    }
}

fn line_col(src: &str, pos: u32) -> (usize, usize) {
    let before = &src[..pos as usize];
    (before.matches('\n').count() + 1, pos as usize - before.rfind('\n').map_or(0, |i| i + 1) + 1)
}

fn main() {
    let src = "fn add(a: int, b: int) -> int { a + b }

fn good(flag: bool) -> int {
    let n = add(2, 3) * 4;
    let big = n > 10;
    let v: int = if flag { n } else { return 0 };
    if big && flag { v } else { -v }
}

fn main() -> int {
    let flag = 3 > 2;
    let total: int = if flag { 10 } else { false };
    let y = add(1);
    let z = flag + 1;
    let w = z * 2;
    while total { }
    if flag { return true; }
    total
}";
    let items = parse_program(src).expect("parses");
    let mut ck = Checker::new(src, &items);
    for f in &items {
        ck.let_types.clear();
        let before = ck.errors.len();
        ck.check_fn(f);
        let lets: Vec<String> = ck.let_types.iter().map(|(n, t)| format!("{n}: {t}")).collect();
        println!("fn {:<5} lets [{}]  {} errors", f.name, lets.join(", "), ck.errors.len() - before);
    }
    println!();
    for (span, msg) in &ck.errors {
        let (l, c) = line_col(src, span.lo);
        println!("{l}:{c}: error: {msg}\n      at `{}`", &src[span.lo as usize..span.hi as usize]);
    }
}
