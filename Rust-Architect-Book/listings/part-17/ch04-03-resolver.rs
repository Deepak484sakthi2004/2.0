// verify: debug ok
// Listing 17.4-3: name resolution for Ore. Pass 1 collects every fn (so calls may precede
// definitions); pass 2 walks each body with a stack of scopes, resolving each name use to a local
// or a fn and recording it in a side table. Errors come with "did you mean" suggestions.
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


// ---------------- name resolution ----------------
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Prim { Int, Bool }

/// What a name refers to. Values and types live in separate namespaces, as in Rust.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Res { Local(u32), Fn(u32) }

pub struct LocalInfo { pub name: String, pub mutable: bool, pub decl: Span }

pub struct Diag { pub span: Span, pub msg: String, pub help: Option<String> }

pub struct Resolver<'a> {
    fns: HashMap<&'a str, (u32, Span)>, // module scope: every fn, visible everywhere in the module
    scopes: Vec<HashMap<String, u32>>,   // local scopes, innermost last
    pub locals: Vec<LocalInfo>,
    pub res: HashMap<u32, Res>,          // side table: span.lo of each value use -> resolution
    pub types: HashMap<u32, Prim>,       // side table: span.lo of each type annotation -> type
    pub diags: Vec<Diag>,
}

/// Levenshtein distance, for "did you mean" suggestions.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, &cb) in b.iter().enumerate() {
            cur.push((prev[j] + (ca != cb) as usize).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// The closest candidate within a length-scaled distance (similar in spirit to rustc's threshold).
fn best_match<'c>(name: &str, candidates: impl Iterator<Item = &'c str>) -> Option<&'c str> {
    let limit = name.len().max(3) / 3;
    candidates
        .map(|c| (edit_distance(name, c), c))
        .filter(|&(d, _)| d <= limit)
        .min()
        .map(|(_, c)| c)
}

impl<'a> Resolver<'a> {
    pub fn new() -> Self {
        Resolver { fns: HashMap::new(), scopes: Vec::new(), locals: Vec::new(), res: HashMap::new(),
                   types: HashMap::new(), diags: Vec::new() }
    }

    fn error(&mut self, span: Span, msg: String, help: Option<String>) {
        self.diags.push(Diag { span, msg, help });
    }

    /// Pass 1: collect items. Every fn is visible in every body, so calls may precede definitions.
    pub fn collect_items(&mut self, items: &'a [FnDecl], src: &str) {
        for (i, f) in items.iter().enumerate() {
            let name_span = Span { lo: f.span.lo + 3, hi: f.span.lo + 3 + f.name.len() as u32 };
            if let Some(&(_, prev)) = self.fns.get(f.name.as_str()) {
                self.error(name_span, format!("the name `{}` is defined multiple times", f.name),
                           Some(format!("previous definition of `{}` is on line {}", f.name, line_col(src, prev.lo).0)));
            } else {
                self.fns.insert(&f.name, (i as u32, name_span));
            }
        }
    }

    fn resolve_type(&mut self, name: &str, span: Span) {
        match name {
            "int" => { self.types.insert(span.lo, Prim::Int); }
            "bool" => { self.types.insert(span.lo, Prim::Bool); }
            _ => {
                let help = best_match(name, ["int", "bool"].into_iter())
                    .map(|m| format!("a type with a similar name exists: `{m}`"));
                self.error(span, format!("cannot find type `{name}` in this scope"), help);
            }
        }
    }

    fn declare(&mut self, name: &str, mutable: bool, decl: Span) {
        let id = self.locals.len() as u32;
        self.locals.push(LocalInfo { name: name.to_string(), mutable, decl });
        self.scopes.last_mut().unwrap().insert(name.to_string(), id);
    }

    fn lookup(&self, name: &str) -> Option<Res> {
        for scope in self.scopes.iter().rev() {
            if let Some(&id) = scope.get(name) {
                return Some(Res::Local(id));
            }
        }
        self.fns.get(name).map(|&(id, _)| Res::Fn(id))
    }

    /// Pass 2: resolve one body. (The AST stores annotation names without spans, so this listing finds
    /// them in `src`; a real front end would keep a span on every type annotation.)
    pub fn resolve_fn(&mut self, f: &FnDecl, src: &str) {
        self.scopes.push(HashMap::new());
        let mut search_from = f.span.lo as usize;
        for p in &f.params {
            if let Some(ty) = &p.ty {
                let lo = src[search_from..].find(ty.as_str()).unwrap() + search_from;
                search_from = lo + ty.len();
                self.resolve_type(ty, Span { lo: lo as u32, hi: (lo + ty.len()) as u32 });
            }
            self.declare(&p.name, false, p.span); // parameters are immutable in Ore v0
        }
        if let Some(ret) = &f.ret {
            let lo = src[search_from..].find(ret.as_str()).unwrap() + search_from;
            self.resolve_type(ret, Span { lo: lo as u32, hi: (lo + ret.len()) as u32 });
        }
        self.block(&f.body);
        self.scopes.pop();
    }

    fn block(&mut self, b: &Block) {
        self.scopes.push(HashMap::new());
        for s in &b.stmts {
            match s {
                Stmt::Let { name, mutable, init, span, .. } => {
                    self.expr(init); // the initializer is resolved BEFORE the new name exists
                    let decl = Span { lo: span.lo, hi: span.lo + 3 }; // the `let` keyword
                    self.declare(name, *mutable, decl);
                }
                Stmt::Expr(e) => self.expr(e),
            }
        }
        if let Some(t) = &b.tail {
            self.expr(t);
        }
        self.scopes.pop();
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Return(None) => {}
            ExprKind::Var(name) => match self.lookup(name) {
                Some(r) => { self.res.insert(e.span.lo, r); }
                None => {
                    let visible: Vec<String> = self.scopes.iter().flat_map(|s| s.keys().cloned())
                        .chain(self.fns.keys().map(|k| k.to_string())).collect();
                    let help = best_match(name, visible.iter().map(|s| s.as_str()))
                        .map(|m| format!("a local variable or fn with a similar name exists: `{m}`"));
                    self.error(e.span, format!("cannot find value `{name}` in this scope"), help);
                }
            },
            ExprKind::Assign(lhs, rhs) => {
                self.expr(rhs);
                self.expr(lhs);
                if let (ExprKind::Var(name), Some(r)) = (&lhs.kind, self.res.get(&lhs.span.lo).copied()) {
                    let ok = matches!(r, Res::Local(id) if self.locals[id as usize].mutable);
                    if !ok {
                        self.error(lhs.span, format!("cannot assign twice to immutable variable `{name}`"),
                                   Some("consider making this binding mutable".to_string()));
                    }
                }
            }
            ExprKind::Unary(_, x) | ExprKind::Return(Some(x)) => self.expr(x),
            ExprKind::Binary(_, a, b) => { self.expr(a); self.expr(b); }
            ExprKind::Call(f, args) => {
                self.expr(f);
                for a in args { self.expr(a); }
            }
            ExprKind::If(c, t, els) => {
                self.expr(c);
                self.block(t);
                if let Some(e) = els { self.expr(e); }
            }
            ExprKind::While(c, b) => { self.expr(c); self.block(b); }
            ExprKind::Block(b) => self.block(b),
        }
    }
}

fn line_col(src: &str, pos: u32) -> (usize, usize) {
    let before = &src[..pos as usize];
    let line = before.matches('\n').count() + 1;
    let col = pos as usize - before.rfind('\n').map_or(0, |i| i + 1) + 1;
    (line, col)
}

fn main() {
    let src = "fn main() -> int {
    let x = 1;
    let x = x + 1;
    let int = 5;
    helper(x, int)
}

fn helper(a: int, b: int) -> int {
    let total = a + b;
    if total > 3 { let y = total; y } else { totl }
}

fn helper(q: int) -> int { q }

fn bad(n: int) -> intt {
    n = 1;
    n
}";
    let items = parse_program(src).expect("parses");
    let mut r = Resolver::new();
    r.collect_items(&items, src);
    for f in &items {
        r.resolve_fn(f, src);
    }

    println!("resolutions in `main` (value namespace):");
    let main_end = items[0].span.hi;
    let mut uses: Vec<(&u32, &Res)> = r.res.iter().filter(|&(&lo, _)| lo < main_end).collect();
    uses.sort_by_key(|(lo, _)| **lo);
    for (&lo, res) in uses {
        let (l, c) = line_col(src, lo);
        let name: String = src[lo as usize..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        let target = match res {
            Res::Local(id) => {
                let info = &r.locals[*id as usize];
                let (dl, dc) = line_col(src, info.decl.lo);
                format!("local #{id} `{}` declared at {dl}:{dc}", info.name)
            }
            Res::Fn(id) => format!("fn #{id} `{}`", items[*id as usize].name),
        };
        println!("  {l}:{c:<3} {name:<7} -> {target}");
    }
    println!("type annotations resolved: {} (int/bool live in the type namespace)", r.types.len());

    println!("\n{} errors:", r.diags.len());
    r.diags.sort_by_key(|d| d.span.lo);
    for d in &r.diags {
        let (l, c) = line_col(src, d.span.lo);
        println!("  {l}:{c}: error: {}", d.msg);
        if let Some(h) = &d.help {
            println!("         help: {h}");
        }
    }
}
