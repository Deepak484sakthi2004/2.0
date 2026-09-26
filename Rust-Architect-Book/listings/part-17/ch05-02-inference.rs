// verify: debug ok
// Listing 17.5-2: Hindley-Milner type inference for Ore functions without annotations.
// Type variables live in a union-find (find = follow bindings with path compression, union = bind);
// `unify` makes two types equal or fails; the occurs check rejects infinite types; top-level
// functions are generalized (let-polymorphism), so `pick` and `id` work at several types.
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


// ---------------- Hindley-Milner inference (unification over a union-find of type variables) ----------------
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Ty { Var(u32), Int, Bool, Unit, Fn(Vec<Ty>, Box<Ty>) }

/// A type scheme: `forall vars. ty`. Top-level fns are generalized; locals are not.
#[derive(Debug, Clone)]
pub struct Scheme { vars: Vec<u32>, ty: Ty }

pub struct Infer {
    parent: Vec<Option<Ty>>, // union-find: a type variable is unbound (a root) or points at a type
    pub log: Option<Vec<String>>,
}

impl Infer {
    fn fresh(&mut self) -> Ty {
        self.parent.push(None);
        Ty::Var(self.parent.len() as u32 - 1)
    }

    /// find(): follow bindings to the representative, compressing the path as we go.
    fn find(&mut self, t: &Ty) -> Ty {
        if let Ty::Var(v) = t {
            if let Some(bound) = self.parent[*v as usize].clone() {
                let root = self.find(&bound);
                self.parent[*v as usize] = Some(root.clone());
                return root;
            }
        }
        t.clone()
    }

    /// Substitute everything we know, all the way down.
    fn zonk(&mut self, t: &Ty) -> Ty {
        match self.find(t) {
            Ty::Fn(ps, r) => Ty::Fn(ps.iter().map(|p| self.zonk(p)).collect(), Box::new(self.zonk(&r))),
            other => other,
        }
    }

    fn occurs(&mut self, v: u32, t: &Ty) -> bool {
        match self.find(t) {
            Ty::Var(w) => v == w,
            Ty::Fn(ps, r) => ps.iter().any(|p| self.occurs(v, p)) || self.occurs(v, &r),
            _ => false,
        }
    }

    /// unify(): make two types equal by binding variables, or fail.
    fn unify(&mut self, a: &Ty, b: &Ty) -> Result<(), String> {
        let (a, b) = (self.find(a), self.find(b));
        if let Some(log) = &mut self.log {
            log.push(format!("unify {} ~ {}", show(&a, &[]), show(&b, &[])));
        }
        match (&a, &b) {
            (Ty::Var(x), Ty::Var(y)) if x == y => Ok(()),
            (Ty::Var(v), t) | (t, Ty::Var(v)) => {
                if self.occurs(*v, t) {
                    let t = self.zonk(t);
                    return Err(format!("infinite type: t{v} = {}", show(&t, &[])));
                }
                self.parent[*v as usize] = Some(t.clone()); // union
                Ok(())
            }
            (Ty::Int, Ty::Int) | (Ty::Bool, Ty::Bool) | (Ty::Unit, Ty::Unit) => Ok(()),
            (Ty::Fn(p1, r1), Ty::Fn(p2, r2)) if p1.len() == p2.len() => {
                for (x, y) in p1.iter().zip(p2) {
                    self.unify(x, y)?;
                }
                self.unify(r1, r2)
            }
            _ => {
                let (a, b) = (self.zonk(&a), self.zonk(&b));
                Err(format!("mismatched types: {} vs {}", show(&a, &[]), show(&b, &[])))
            }
        }
    }

    fn instantiate(&mut self, s: &Scheme) -> Ty {
        let map: HashMap<u32, Ty> = s.vars.iter().map(|&v| (v, self.fresh())).collect();
        fn go(t: &Ty, map: &HashMap<u32, Ty>) -> Ty {
            match t {
                Ty::Var(v) => map.get(v).cloned().unwrap_or(Ty::Var(*v)),
                Ty::Fn(ps, r) => Ty::Fn(ps.iter().map(|p| go(p, map)).collect(), Box::new(go(r, map))),
                other => other.clone(),
            }
        }
        go(&s.ty, &map)
    }

    fn generalize(&mut self, t: &Ty) -> Scheme {
        let ty = self.zonk(t);
        fn free(t: &Ty, out: &mut Vec<u32>) {
            match t {
                Ty::Var(v) => if !out.contains(v) { out.push(*v) },
                Ty::Fn(ps, r) => { ps.iter().for_each(|p| free(p, out)); free(r, out) }
                _ => {}
            }
        }
        let mut vars = Vec::new();
        free(&ty, &mut vars); // at top level the environment is closed, so every free variable generalizes
        Scheme { vars, ty }
    }
}

/// Pretty-print, naming quantified variables a, b, c...
fn show(t: &Ty, names: &[u32]) -> String {
    match t {
        Ty::Var(v) => match names.iter().position(|n| n == v) {
            Some(i) => ((b'a' + i as u8) as char).to_string(),
            None => format!("t{v}"),
        },
        Ty::Int => "int".into(),
        Ty::Bool => "bool".into(),
        Ty::Unit => "()".into(),
        Ty::Fn(ps, r) => {
            let ps: Vec<String> = ps.iter().map(|p| show(p, names)).collect();
            format!("fn({}) -> {}", ps.join(", "), show(r, names))
        }
    }
}

struct Ctx<'e> { inf: &'e mut Infer, scopes: Vec<HashMap<String, Scheme>>, ret: Ty }

impl Ctx<'_> {
    fn lookup(&mut self, name: &str) -> Result<Ty, String> {
        let s = self.scopes.iter().rev().find_map(|s| s.get(name)).cloned()
            .ok_or(format!("cannot find `{name}`"))?;
        Ok(self.inf.instantiate(&s))
    }
    fn bind(&mut self, name: &str, t: Ty) {
        self.scopes.last_mut().unwrap().insert(name.to_string(), Scheme { vars: vec![], ty: t });
    }

    fn block(&mut self, b: &Block) -> Result<Ty, String> {
        self.scopes.push(HashMap::new());
        for s in &b.stmts {
            match s {
                Stmt::Let { name, ty, init, .. } => {
                    let t = self.expr(init)?;
                    if let Some(ann) = ty { self.inf.unify(&t, &annotation(ann)?)?; }
                    self.bind(name, t); // monomorphic: locals are not generalized in this listing
                }
                Stmt::Expr(e) => { self.expr(e)?; }
            }
        }
        let t = match &b.tail { Some(t) => self.expr(t)?, None => Ty::Unit };
        self.scopes.pop();
        Ok(t)
    }

    fn expr(&mut self, e: &Expr) -> Result<Ty, String> {
        Ok(match &e.kind {
            ExprKind::Int(_) => Ty::Int,
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::Var(name) => self.lookup(name)?,
            ExprKind::Unary(op, x) => {
                let t = if *op == UnOp::Neg { Ty::Int } else { Ty::Bool };
                let tx = self.expr(x)?;
                self.inf.unify(&tx, &t)?;
                t
            }
            ExprKind::Binary(op, a, b) => {
                let (ta, tb) = (self.expr(a)?, self.expr(b)?);
                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                        self.inf.unify(&ta, &Ty::Int)?; self.inf.unify(&tb, &Ty::Int)?; Ty::Int
                    }
                    BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        self.inf.unify(&ta, &Ty::Int)?; self.inf.unify(&tb, &Ty::Int)?; Ty::Bool
                    }
                    BinOp::And | BinOp::Or => {
                        self.inf.unify(&ta, &Ty::Bool)?; self.inf.unify(&tb, &Ty::Bool)?; Ty::Bool
                    }
                    BinOp::Eq | BinOp::Ne => { self.inf.unify(&ta, &tb)?; Ty::Bool }
                }
            }
            ExprKind::Assign(lhs, rhs) => {
                let (tl, tr) = (self.expr(lhs)?, self.expr(rhs)?);
                self.inf.unify(&tl, &tr)?;
                Ty::Unit
            }
            ExprKind::Call(callee, args) => {
                let tf = self.expr(callee)?;
                let targs = args.iter().map(|a| self.expr(a)).collect::<Result<Vec<_>, _>>()?;
                let ret = self.inf.fresh();
                self.inf.unify(&tf, &Ty::Fn(targs, Box::new(ret.clone())))?;
                ret
            }
            ExprKind::If(c, then, els) => {
                let tc = self.expr(c)?;
                self.inf.unify(&tc, &Ty::Bool)?;
                let tt = self.block(then)?;
                match els {
                    Some(els) => { let te = self.expr(els)?; self.inf.unify(&tt, &te)?; tt }
                    None => { self.inf.unify(&tt, &Ty::Unit)?; Ty::Unit }
                }
            }
            ExprKind::While(c, body) => {
                let tc = self.expr(c)?;
                self.inf.unify(&tc, &Ty::Bool)?;
                self.block(body)?;
                Ty::Unit
            }
            ExprKind::Block(b) => self.block(b)?,
            ExprKind::Return(v) => {
                let tv = match v { Some(v) => self.expr(v)?, None => Ty::Unit };
                let ret = self.ret.clone();
                self.inf.unify(&tv, &ret)?;
                self.inf.fresh() // `return` has every type: a fresh, unconstrained variable
            }
        })
    }
}

fn annotation(name: &str) -> Result<Ty, String> {
    match name { "int" => Ok(Ty::Int), "bool" => Ok(Ty::Bool), _ => Err(format!("unknown type `{name}`")) }
}

/// Infers one top-level fn. Missing annotations (params or return) become fresh type variables.
fn infer_fn(inf: &mut Infer, globals: &HashMap<String, Scheme>, f: &FnDecl) -> Result<Scheme, String> {
    let mut params = Vec::new();
    for p in &f.params {
        params.push(match &p.ty { Some(t) => annotation(t)?, None => inf.fresh() });
    }
    let ret = match &f.ret { Some(t) => annotation(t)?, None => inf.fresh() };
    let fn_ty = Ty::Fn(params.clone(), Box::new(ret.clone()));
    let mut scope: HashMap<String, Scheme> = HashMap::new();
    // recursion: inside its own body the fn is monomorphic
    scope.insert(f.name.clone(), Scheme { vars: vec![], ty: fn_ty.clone() });
    for (p, t) in f.params.iter().zip(&params) {
        scope.insert(p.name.clone(), Scheme { vars: vec![], ty: t.clone() });
    }
    let mut ctx = Ctx { inf, scopes: vec![globals.clone(), scope], ret: ret.clone() };
    let body = ctx.block(&f.body)?;
    ctx.inf.unify(&body, &ret)?;
    Ok(ctx.inf.generalize(&fn_ty))
}

fn main() {
    let src = "
fn add(a, b) { a + b }
fn pick(c, a, b) { if c { a } else { b } }
fn id(x) { x }
fn twice(f, x) { f(f(x)) }
fn fact(n) { if n < 2 { 1 } else { n * fact(n - 1) } }
fn use_them() {
    let w = if pick(false, true, false) { 1 } else { 0 };
    pick(true, 1, 2) + add(3, 4) + twice(id, 5) + w
}
fn self_apply(x) { x(x) }
fn branches(x) { if x { 1 } else { false } }
fn plus_bool(n) { n + true }";
    let items = parse_program(src).expect("parses");
    let mut inf = Infer { parent: Vec::new(), log: None };
    let mut globals: HashMap<String, Scheme> = HashMap::new();
    for f in &items {
        inf.log = (f.name == "twice").then(Vec::new);
        match infer_fn(&mut inf, &globals, f) {
            Ok(s) => {
                let quant = if s.vars.is_empty() { String::new() } else {
                    let names: Vec<String> = (0..s.vars.len()).map(|i| ((b'a' + i as u8) as char).to_string()).collect();
                    format!("forall {}. ", names.join(" "))
                };
                println!("{:<10} : {quant}{}", f.name, show(&s.ty, &s.vars));
                globals.insert(f.name.clone(), s);
            }
            Err(e) => println!("{:<10} : error: {e}", f.name),
        }
        if let Some(log) = inf.log.take() {
            for line in log {
                println!("             {line}");
            }
        }
    }
}
