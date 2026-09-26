// verify: debug ok
// Listing 17.7-1: an optimizer. Ore source -> CFG (17.6-1) -> SSA (17.6-2) -> four passes:
//   simplify: constant folding/propagation, copy propagation, trivial phis, branch folding
//   gvn:      global value numbering over the dominator tree (common subexpressions)
//   dce:      dead-code elimination by mark and sweep, keeping anything that might trap
//   merge:    straight-line blocks joined by a jump become one block
// Every function is run before and after on many inputs: the results must be identical.
// The lexer and parser are listing 17.3-1's; lowering and SSA are listings 17.6-1/17.6-2's, unchanged.
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


// ---------------- IR: three-address code in basic blocks ----------------
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Val { Var(u32), Const(i64) }

#[derive(Debug, Clone)]
pub enum Inst {
    Copy { dst: u32, src: Val },
    Bin { dst: u32, op: BinOp, a: Val, b: Val }, // never And/Or: those are control flow
    Un { dst: u32, op: UnOp, a: Val },
    Call { dst: u32, func: String, args: Vec<Val> },
}

#[derive(Debug, Clone)]
pub enum Term { Jump(usize), Branch { cond: Val, then: usize, els: usize }, Return(Val) }

#[derive(Debug, Clone, Default)]
pub struct BasicBlock { pub insts: Vec<Inst>, pub term: Option<Term> }

#[derive(Debug, Clone)]
pub struct FnIr { pub name: String, pub params: Vec<u32>, pub vars: Vec<String>, pub blocks: Vec<BasicBlock> }

impl Term {
    pub fn succs(&self) -> Vec<usize> {
        match self {
            Term::Jump(t) => vec![*t],
            Term::Branch { then, els, .. } => vec![*then, *els],
            Term::Return(_) => vec![],
        }
    }
}

// ---------------- lowering: AST -> CFG ----------------
struct Lower {
    f: FnIr,
    cur: usize,
    scopes: Vec<HashMap<String, u32>>,
    temps: u32,
}

impl Lower {
    fn new_block(&mut self) -> usize {
        self.f.blocks.push(BasicBlock::default());
        self.f.blocks.len() - 1
    }
    fn new_var(&mut self, name: &str) -> u32 {
        let uses = self.f.vars.iter().filter(|v| v.split('.').next() == Some(name)).count();
        self.f.vars.push(if uses == 0 { name.to_string() } else { format!("{name}.{uses}") });
        self.f.vars.len() as u32 - 1
    }
    fn temp(&mut self) -> u32 {
        self.f.vars.push(format!("%{}", self.temps));
        self.temps += 1;
        self.f.vars.len() as u32 - 1
    }
    fn emit(&mut self, i: Inst) {
        self.f.blocks[self.cur].insts.push(i);
    }
    fn terminate(&mut self, t: Term) {
        let b = &mut self.f.blocks[self.cur];
        if b.term.is_none() {
            b.term = Some(t);
        }
    }
    fn lookup(&self, name: &str) -> u32 {
        *self.scopes.iter().rev().find_map(|s| s.get(name)).expect("resolved and type-checked earlier")
    }

    fn block(&mut self, b: &Block) -> Val {
        self.scopes.push(HashMap::new());
        for s in &b.stmts {
            match s {
                Stmt::Let { name, init, .. } => {
                    let v = self.expr(init);
                    let var = self.new_var(name);
                    self.emit(Inst::Copy { dst: var, src: v });
                    self.scopes.last_mut().unwrap().insert(name.clone(), var);
                }
                Stmt::Expr(e) => { self.expr(e); }
            }
        }
        let v = match &b.tail { Some(t) => self.expr(t), None => Val::Const(0) };
        self.scopes.pop();
        v
    }

    fn expr(&mut self, e: &Expr) -> Val {
        match &e.kind {
            ExprKind::Int(n) => Val::Const(*n),
            ExprKind::Bool(b) => Val::Const(*b as i64),
            ExprKind::Var(name) => Val::Var(self.lookup(name)),
            ExprKind::Unary(op, x) => {
                let a = self.expr(x);
                let dst = self.temp();
                self.emit(Inst::Un { dst, op: *op, a });
                Val::Var(dst)
            }
            // Short-circuit operators become control flow: the right operand runs only when needed.
            ExprKind::Binary(op @ (BinOp::And | BinOp::Or), a, b) => {
                let r = self.temp();
                let va = self.expr(a);
                let (rhs, short, join) = (self.new_block(), self.new_block(), self.new_block());
                let (then, els) = if *op == BinOp::And { (rhs, short) } else { (short, rhs) };
                self.terminate(Term::Branch { cond: va, then, els });
                self.cur = rhs;
                let vb = self.expr(b);
                self.emit(Inst::Copy { dst: r, src: vb });
                self.terminate(Term::Jump(join));
                self.cur = short;
                self.emit(Inst::Copy { dst: r, src: Val::Const((*op == BinOp::Or) as i64) });
                self.terminate(Term::Jump(join));
                self.cur = join;
                Val::Var(r)
            }
            ExprKind::Binary(op, a, b) => {
                let (va, vb) = (self.expr(a), self.expr(b));
                let dst = self.temp();
                self.emit(Inst::Bin { dst, op: *op, a: va, b: vb });
                Val::Var(dst)
            }
            ExprKind::Assign(lhs, rhs) => {
                let v = self.expr(rhs);
                let ExprKind::Var(name) = &lhs.kind else { unreachable!() };
                let dst = self.lookup(name);
                self.emit(Inst::Copy { dst, src: v });
                Val::Const(0)
            }
            ExprKind::Call(callee, args) => {
                let ExprKind::Var(func) = &callee.kind else { unreachable!() };
                let args = args.iter().map(|a| self.expr(a)).collect();
                let dst = self.temp();
                self.emit(Inst::Call { dst, func: func.clone(), args });
                Val::Var(dst)
            }
            ExprKind::If(c, then_b, els) => {
                let vc = self.expr(c);
                let r = self.temp();
                let (tb, join) = (self.new_block(), self.new_block());
                let eb = if els.is_some() { self.new_block() } else { join };
                self.terminate(Term::Branch { cond: vc, then: tb, els: eb });
                self.cur = tb;
                let v = self.block(then_b);
                self.emit(Inst::Copy { dst: r, src: v });
                self.terminate(Term::Jump(join));
                if let Some(els) = els {
                    self.cur = eb;
                    let v = self.expr(els);
                    self.emit(Inst::Copy { dst: r, src: v });
                    self.terminate(Term::Jump(join));
                }
                self.cur = join;
                Val::Var(r)
            }
            ExprKind::While(c, body) => {
                let header = self.new_block();
                self.terminate(Term::Jump(header));
                self.cur = header;
                let vc = self.expr(c);
                let (body_b, exit) = (self.new_block(), self.new_block());
                self.terminate(Term::Branch { cond: vc, then: body_b, els: exit });
                self.cur = body_b;
                self.block(body);
                self.terminate(Term::Jump(header));
                self.cur = exit;
                Val::Const(0)
            }
            ExprKind::Block(b) => self.block(b),
            ExprKind::Return(v) => {
                let v = match v { Some(v) => self.expr(v), None => Val::Const(0) };
                self.terminate(Term::Return(v));
                self.cur = self.new_block(); // code after `return` goes into an unreachable block
                Val::Const(0)
            }
        }
    }
}

/// Removes blocks unreachable from the entry, renumbering the rest in order.
fn remove_unreachable(f: &mut FnIr) {
    let mut seen = vec![false; f.blocks.len()];
    let mut stack = vec![0];
    while let Some(b) = stack.pop() {
        if std::mem::replace(&mut seen[b], true) { continue; }
        if let Some(t) = &f.blocks[b].term { stack.extend(t.succs()); }
    }
    let mut new_id = vec![usize::MAX; f.blocks.len()];
    let mut n = 0;
    for (b, &s) in seen.iter().enumerate() {
        if s { new_id[b] = n; n += 1; }
    }
    let old = std::mem::take(&mut f.blocks);
    for (b, mut bb) in old.into_iter().enumerate() {
        if !seen[b] { continue; }
        bb.term = Some(match bb.term.unwrap_or(Term::Return(Val::Const(0))) {
            Term::Jump(t) => Term::Jump(new_id[t]),
            Term::Branch { cond, then, els } => Term::Branch { cond, then: new_id[then], els: new_id[els] },
            r => r,
        });
        f.blocks.push(bb);
    }
}

pub fn lower_fn(fd: &FnDecl) -> FnIr {
    let mut l = Lower {
        f: FnIr { name: fd.name.clone(), params: Vec::new(), vars: Vec::new(), blocks: Vec::new() },
        cur: 0, scopes: vec![HashMap::new()], temps: 0,
    };
    l.new_block();
    for p in &fd.params {
        let v = l.new_var(&p.name);
        l.f.params.push(v);
        l.scopes[0].insert(p.name.clone(), v);
    }
    let v = l.block(&fd.body);
    l.terminate(Term::Return(v));
    remove_unreachable(&mut l.f);
    l.f
}

// ---------------- printing ----------------
pub fn val(f: &FnIr, v: Val) -> String {
    match v { Val::Var(x) => f.vars[x as usize].clone(), Val::Const(c) => c.to_string() }
}

pub fn print_fn(f: &FnIr) {
    let ps: Vec<&str> = f.params.iter().map(|&p| f.vars[p as usize].as_str()).collect();
    println!("fn {}({}):", f.name, ps.join(", "));
    for (i, b) in f.blocks.iter().enumerate() {
        println!("  bb{i}:");
        for inst in &b.insts {
            let s = match inst {
                Inst::Copy { dst, src } => format!("{} = {}", f.vars[*dst as usize], val(f, *src)),
                Inst::Bin { dst, op, a, b } => format!("{} = {} {} {}", f.vars[*dst as usize], val(f, *a), op_str(*op), val(f, *b)),
                Inst::Un { dst, op, a } => format!("{} = {}{}", f.vars[*dst as usize], if *op == UnOp::Neg { "-" } else { "!" }, val(f, *a)),
                Inst::Call { dst, func, args } => {
                    let a: Vec<String> = args.iter().map(|&x| val(f, x)).collect();
                    format!("{} = call {func}({})", f.vars[*dst as usize], a.join(", "))
                }
            };
            println!("      {s}");
        }
        let t = match b.term.as_ref().unwrap() {
            Term::Jump(t) => format!("jump bb{t}"),
            Term::Branch { cond, then, els } => format!("branch {} ? bb{then} : bb{els}", val(f, *cond)),
            Term::Return(v) => format!("return {}", val(f, *v)),
        };
        println!("      {t}");
    }
}

// ---------------- dominators (Cooper, Harvey & Kennedy 2001) and dominance frontiers ----------------
pub fn preds(f: &FnIr) -> Vec<Vec<usize>> {
    let mut p = vec![Vec::new(); f.blocks.len()];
    for (b, bb) in f.blocks.iter().enumerate() {
        for s in bb.term.as_ref().unwrap().succs() { p[s].push(b); }
    }
    p
}

pub fn reverse_postorder(f: &FnIr) -> Vec<usize> {
    fn dfs(f: &FnIr, b: usize, seen: &mut Vec<bool>, out: &mut Vec<usize>) {
        seen[b] = true;
        for s in f.blocks[b].term.as_ref().unwrap().succs() {
            if !seen[s] { dfs(f, s, seen, out); }
        }
        out.push(b);
    }
    let mut out = Vec::new();
    dfs(f, 0, &mut vec![false; f.blocks.len()], &mut out);
    out.reverse();
    out
}

pub fn dominators(f: &FnIr) -> Vec<usize> {
    let rpo = reverse_postorder(f);
    let mut order = vec![0; f.blocks.len()];
    for (i, &b) in rpo.iter().enumerate() { order[b] = i; }
    let preds = preds(f);
    let mut idom = vec![usize::MAX; f.blocks.len()];
    idom[0] = 0;
    let mut changed = true;
    while changed {
        changed = false;
        for &b in rpo.iter().skip(1) {
            let mut new = usize::MAX;
            for &p in &preds[b] {
                if idom[p] == usize::MAX { continue; } // not processed yet
                new = if new == usize::MAX { p } else {
                    let (mut x, mut y) = (p, new);
                    while x != y {
                        while order[x] > order[y] { x = idom[x]; }
                        while order[y] > order[x] { y = idom[y]; }
                    }
                    x
                };
            }
            if idom[b] != new { idom[b] = new; changed = true; }
        }
    }
    idom
}

pub fn dominance_frontiers(f: &FnIr, idom: &[usize]) -> Vec<Vec<usize>> {
    let preds = preds(f);
    let mut df = vec![Vec::new(); f.blocks.len()];
    for b in 0..f.blocks.len() {
        if preds[b].len() < 2 { continue; }
        for &p in &preds[b] {
            let mut runner = p;
            while runner != idom[b] {
                if !df[runner].contains(&b) { df[runner].push(b); }
                runner = idom[runner];
            }
        }
    }
    df
}

fn dominates(idom: &[usize], a: usize, mut b: usize) -> bool {
    loop {
        if a == b { return true; }
        if b == 0 { return false; }
        b = idom[b];
    }
}

// ---------------- an interpreter for the IR ----------------
pub fn run(fns: &HashMap<String, FnIr>, name: &str, args: &[i64], steps: &mut u64) -> Result<i64, String> {
    let f = &fns[name];
    let mut vars = vec![0i64; f.vars.len()];
    for (p, a) in f.params.iter().zip(args) { vars[*p as usize] = *a; }
    let get = |vars: &[i64], v: Val| match v { Val::Var(x) => vars[x as usize], Val::Const(c) => c };
    let mut b = 0;
    loop {
        for inst in &f.blocks[b].insts {
            *steps += 1;
            match inst {
                Inst::Copy { dst, src } => vars[*dst as usize] = get(&vars, *src),
                Inst::Un { dst, op, a } => {
                    let x = get(&vars, *a);
                    vars[*dst as usize] = if *op == UnOp::Neg { x.wrapping_neg() } else { (x == 0) as i64 };
                }
                Inst::Bin { dst, op, a, b } => {
                    let (x, y) = (get(&vars, *a), get(&vars, *b));
                    vars[*dst as usize] = match op {
                        BinOp::Add => x.wrapping_add(y), BinOp::Sub => x.wrapping_sub(y), BinOp::Mul => x.wrapping_mul(y),
                        BinOp::Div | BinOp::Rem if y == 0 => return Err(format!("division by zero in {name}")),
                        BinOp::Div => x.wrapping_div(y), BinOp::Rem => x.wrapping_rem(y),
                        BinOp::Eq => (x == y) as i64, BinOp::Ne => (x != y) as i64, BinOp::Lt => (x < y) as i64,
                        BinOp::Le => (x <= y) as i64, BinOp::Gt => (x > y) as i64, BinOp::Ge => (x >= y) as i64,
                        BinOp::And | BinOp::Or => unreachable!("lowered to branches"),
                    };
                }
                Inst::Call { dst, func, args } => {
                    let a: Vec<i64> = args.iter().map(|&v| get(&vars, v)).collect();
                    vars[*dst as usize] = run(fns, func, &a, steps)?;
                }
            }
        }
        *steps += 1;
        match f.blocks[b].term.as_ref().unwrap() {
            Term::Jump(t) => b = *t,
            Term::Branch { cond, then, els } => b = if get(&vars, *cond) != 0 { *then } else { *els },
            Term::Return(v) => return Ok(get(&vars, *v)),
        }
    }
}


// ---------------- SSA construction (Cytron et al. 1991) ----------------
// 1. place phi nodes at the iterated dominance frontier of each variable's definitions;
// 2. rename: walk the dominator tree with a stack of current versions per variable.

#[derive(Debug, Clone)]
pub struct Phi { pub dst: u32, pub var: u32, pub args: Vec<(usize, Val)> } // (predecessor block, value)

#[derive(Debug, Clone)]
pub struct SsaBlock { pub phis: Vec<Phi>, pub insts: Vec<Inst>, pub term: Term }

#[derive(Clone)]
pub struct SsaFn { pub name: String, pub params: Vec<u32>, pub names: Vec<String>, pub blocks: Vec<SsaBlock> }

fn def_of(i: &Inst) -> u32 {
    match i { Inst::Copy { dst, .. } | Inst::Bin { dst, .. } | Inst::Un { dst, .. } | Inst::Call { dst, .. } => *dst }
}

pub fn to_ssa(f: &FnIr) -> SsaFn {
    let n = f.blocks.len();
    let idom = dominators(f);
    let df = dominance_frontiers(f, &idom);
    let preds = preds(f);

    // Which blocks define each variable (parameters are defined on entry).
    let mut defs: Vec<Vec<usize>> = vec![Vec::new(); f.vars.len()];
    let mut def_count = vec![0usize; f.vars.len()];
    for &p in &f.params { defs[p as usize].push(0); def_count[p as usize] += 1; }
    for (b, bb) in f.blocks.iter().enumerate() {
        for i in &bb.insts {
            let d = def_of(i) as usize;
            def_count[d] += 1;
            if !defs[d].contains(&b) { defs[d].push(b); }
        }
    }
    // A variable assigned more than once gets numbered versions; a single definition keeps its name.
    let multi_def: Vec<bool> = def_count.iter().map(|&c| c > 1).collect();

    // Step 1: phi placement at the iterated dominance frontier.
    let mut phi_vars: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (v, blocks) in defs.iter().enumerate() {
        if blocks.len() < 2 { continue; } // one definition dominates all its uses: no phi needed
        let mut work = blocks.clone();
        let mut has_def = blocks.clone();
        while let Some(b) = work.pop() {
            for &d in &df[b] {
                if !phi_vars[d].contains(&(v as u32)) {
                    phi_vars[d].push(v as u32);
                    if !has_def.contains(&d) { has_def.push(d); work.push(d); }
                }
            }
        }
    }

    // Step 2: renaming over the dominator tree.
    let mut names = f.vars.clone(); // version ids are appended to this table
    let mut counter = vec![0u32; f.vars.len()];
    let mut blocks: Vec<SsaBlock> = f.blocks.iter().enumerate().map(|(b, bb)| SsaBlock {
        phis: phi_vars[b].iter().map(|&var| Phi { dst: var, var, args: Vec::new() }).collect(),
        insts: bb.insts.clone(),
        term: bb.term.clone().unwrap(),
    }).collect();
    let mut children = vec![Vec::new(); n];
    for b in 1..n { children[idom[b]].push(b); }

    struct R<'a> {
        stacks: Vec<Vec<u32>>, names: &'a mut Vec<String>, counter: &'a mut Vec<u32>, multi: &'a [bool],
    }
    impl R<'_> {
        fn new_version(&mut self, v: u32) -> u32 {
            if !self.multi[v as usize] { self.stacks[v as usize].push(v); return v; } // single def keeps its name
            self.counter[v as usize] += 1;
            let base = self.names[v as usize].clone();
            self.names.push(format!("{base}#{}", self.counter[v as usize]));
            let id = self.names.len() as u32 - 1;
            self.stacks[v as usize].push(id);
            id
        }
        fn use_of(&self, v: Val) -> Val {
            match v {
                Val::Var(x) => Val::Var(*self.stacks[x as usize].last().unwrap_or(&x)),
                c => c,
            }
        }
    }
    let mut r = R { stacks: vec![Vec::new(); f.vars.len()], names: &mut names, counter: &mut counter, multi: &multi_def };
    for &p in &f.params { r.stacks[p as usize].push(p); }

    fn rename(b: usize, blocks: &mut Vec<SsaBlock>, children: &[Vec<usize>], preds: &[Vec<usize>], r: &mut R) {
        let mut pushed = Vec::new();
        for phi in blocks[b].phis.iter_mut() {
            phi.dst = r.new_version(phi.var);
            pushed.push(phi.var);
        }
        for inst in blocks[b].insts.iter_mut() {
            match inst {
                Inst::Copy { src, .. } => *src = r.use_of(*src),
                Inst::Bin { a, b, .. } => { *a = r.use_of(*a); *b = r.use_of(*b); }
                Inst::Un { a, .. } => *a = r.use_of(*a),
                Inst::Call { args, .. } => args.iter_mut().for_each(|x| *x = r.use_of(*x)),
            }
            let v = def_of(inst);
            let new = r.new_version(v);
            match inst {
                Inst::Copy { dst, .. } | Inst::Bin { dst, .. } | Inst::Un { dst, .. } | Inst::Call { dst, .. } => *dst = new,
            }
            pushed.push(v);
        }
        blocks[b].term = match blocks[b].term.clone() {
            Term::Branch { cond, then, els } => Term::Branch { cond: r.use_of(cond), then, els },
            Term::Return(v) => Term::Return(r.use_of(v)),
            j => j,
        };
        // Fill this block's slot in each successor's phis.
        for s in blocks[b].term.succs() {
            debug_assert!(preds[s].contains(&b));
            for phi in blocks[s].phis.iter_mut() {
                let v = r.use_of(Val::Var(phi.var));
                let v = if v == Val::Var(phi.var) && r.multi[phi.var as usize] { Val::Const(0) } else { v }; // undefined on this path
                phi.args.push((b, v));
            }
        }
        for &c in &children[b] { rename(c, blocks, children, preds, r); }
        for v in pushed { r.stacks[v as usize].pop(); }
    }
    rename(0, &mut blocks, &children, &preds, &mut r);
    SsaFn { name: f.name.clone(), params: f.params.clone(), names, blocks }
}

fn sval(f: &SsaFn, v: Val) -> String {
    match v { Val::Var(x) => f.names[x as usize].clone(), Val::Const(c) => c.to_string() }
}

pub fn print_ssa(f: &SsaFn) {
    println!("fn {} (SSA):", f.name);
    for (i, b) in f.blocks.iter().enumerate() {
        println!("  bb{i}:");
        for phi in &b.phis {
            let args: Vec<String> = phi.args.iter().map(|(p, v)| format!("bb{p}: {}", sval(f, *v))).collect();
            println!("      {} = phi({})", f.names[phi.dst as usize], args.join(", "));
        }
        for inst in &b.insts {
            let s = match inst {
                Inst::Copy { dst, src } => format!("{} = {}", f.names[*dst as usize], sval(f, *src)),
                Inst::Bin { dst, op, a, b } => format!("{} = {} {} {}", f.names[*dst as usize], sval(f, *a), op_str(*op), sval(f, *b)),
                Inst::Un { dst, op, a } => format!("{} = {}{}", f.names[*dst as usize], if *op == UnOp::Neg { "-" } else { "!" }, sval(f, *a)),
                Inst::Call { dst, func, args } => {
                    let a: Vec<String> = args.iter().map(|&x| sval(f, x)).collect();
                    format!("{} = call {func}({})", f.names[*dst as usize], a.join(", "))
                }
            };
            println!("      {s}");
        }
        println!("      {}", match &b.term {
            Term::Jump(t) => format!("jump bb{t}"),
            Term::Branch { cond, then, els } => format!("branch {} ? bb{then} : bb{els}", sval(f, *cond)),
            Term::Return(v) => format!("return {}", sval(f, *v)),
        });
    }
}

/// Runs SSA code. Phis read their inputs all at once on block entry (parallel-copy semantics).
pub fn run_ssa(fns: &HashMap<String, SsaFn>, name: &str, args: &[i64], steps: &mut u64) -> Result<i64, String> {
    let f = &fns[name];
    let mut vals = vec![0i64; f.names.len()];
    for (p, a) in f.params.iter().zip(args) { vals[*p as usize] = *a; }
    let get = |vals: &[i64], v: Val| match v { Val::Var(x) => vals[x as usize], Val::Const(c) => c };
    let (mut b, mut prev) = (0usize, usize::MAX);
    loop {
        let incoming: Vec<(u32, i64)> = f.blocks[b].phis.iter()
            .map(|phi| (phi.dst, get(&vals, phi.args.iter().find(|(p, _)| *p == prev).map(|a| a.1).unwrap_or(Val::Const(0)))))
            .collect();
        for (d, v) in incoming { vals[d as usize] = v; }
        *steps += (f.blocks[b].phis.len() + f.blocks[b].insts.len() + 1) as u64;
        for inst in &f.blocks[b].insts {
            match inst {
                Inst::Copy { dst, src } => vals[*dst as usize] = get(&vals, *src),
                Inst::Un { dst, op, a } => {
                    let x = get(&vals, *a);
                    vals[*dst as usize] = if *op == UnOp::Neg { x.wrapping_neg() } else { (x == 0) as i64 };
                }
                Inst::Bin { dst, op, a, b } => {
                    let (x, y) = (get(&vals, *a), get(&vals, *b));
                    vals[*dst as usize] = match op {
                        BinOp::Add => x.wrapping_add(y), BinOp::Sub => x.wrapping_sub(y), BinOp::Mul => x.wrapping_mul(y),
                        BinOp::Div | BinOp::Rem if y == 0 => return Err("division by zero".into()),
                        BinOp::Div => x.wrapping_div(y), BinOp::Rem => x.wrapping_rem(y),
                        BinOp::Eq => (x == y) as i64, BinOp::Ne => (x != y) as i64, BinOp::Lt => (x < y) as i64,
                        BinOp::Le => (x <= y) as i64, BinOp::Gt => (x > y) as i64, BinOp::Ge => (x >= y) as i64,
                        BinOp::And | BinOp::Or => unreachable!(),
                    };
                }
                Inst::Call { dst, func, args } => {
                    let a: Vec<i64> = args.iter().map(|&v| get(&vals, v)).collect();
                    vals[*dst as usize] = run_ssa(fns, func, &a, steps)?;
                }
            }
        }
        prev = b;
        match &f.blocks[b].term {
            Term::Jump(t) => b = *t,
            Term::Branch { cond, then, els } => b = if get(&vals, *cond) != 0 { *then } else { *els },
            Term::Return(v) => return Ok(get(&vals, *v)),
        }
    }
}


// ---------------- optimization passes on SSA ----------------

fn operands_mut(i: &mut Inst) -> Vec<&mut Val> {
    match i {
        Inst::Copy { src, .. } => vec![src],
        Inst::Bin { a, b, .. } => vec![a, b],
        Inst::Un { a, .. } => vec![a],
        Inst::Call { args, .. } => args.iter_mut().collect(),
    }
}

fn operands(i: &Inst) -> Vec<Val> {
    match i {
        Inst::Copy { src, .. } => vec![*src],
        Inst::Bin { a, b, .. } => vec![*a, *b],
        Inst::Un { a, .. } => vec![*a],
        Inst::Call { args, .. } => args.clone(),
    }
}

/// Replace every use of `from` with `to` (valid in SSA: `from`'s single definition dominates its uses,
/// and `to` is available wherever `from` was).
fn replace_uses(f: &mut SsaFn, from: u32, to: Val) {
    let fix = |v: &mut Val| if *v == Val::Var(from) { *v = to };
    for b in f.blocks.iter_mut() {
        for phi in b.phis.iter_mut() { phi.args.iter_mut().for_each(|(_, v)| fix(v)); }
        for inst in b.insts.iter_mut() { operands_mut(inst).into_iter().for_each(fix); }
        match &mut b.term {
            Term::Branch { cond, .. } => fix(cond),
            Term::Return(v) => fix(v),
            Term::Jump(_) => {}
        }
    }
}

/// Can executing this instruction fail? Division by a value that might be zero can.
fn may_trap(i: &Inst) -> bool {
    match i {
        Inst::Bin { op: BinOp::Div | BinOp::Rem, b, .. } => !matches!(b, Val::Const(c) if *c != 0),
        Inst::Call { .. } => true, // calls may trap or not terminate: never delete or merge them
        _ => false,
    }
}

fn fold(op: BinOp, x: i64, y: i64) -> Option<i64> {
    Some(match op {
        BinOp::Add => x.wrapping_add(y), BinOp::Sub => x.wrapping_sub(y), BinOp::Mul => x.wrapping_mul(y),
        BinOp::Div | BinOp::Rem if y == 0 => return None, // keep the run-time error; never fold it away
        BinOp::Div => x.wrapping_div(y), BinOp::Rem => x.wrapping_rem(y),
        BinOp::Eq => (x == y) as i64, BinOp::Ne => (x != y) as i64, BinOp::Lt => (x < y) as i64,
        BinOp::Le => (x <= y) as i64, BinOp::Gt => (x > y) as i64, BinOp::Ge => (x >= y) as i64,
        BinOp::And | BinOp::Or => return None,
    })
}

/// Pass 1: constant folding + propagation, copy propagation, trivial phis, branch folding.
/// Repeats until nothing changes. Returns the number of rewrites.
fn simplify(f: &mut SsaFn) -> usize {
    let mut total = 0;
    loop {
        let mut rewrites: Vec<(u32, Val)> = Vec::new();
        for b in &f.blocks {
            for phi in &b.phis {
                let mut distinct: Vec<Val> = Vec::new();
                for &(_, v) in &phi.args {
                    if v != Val::Var(phi.dst) && !distinct.contains(&v) { distinct.push(v); }
                }
                if distinct.len() == 1 { rewrites.push((phi.dst, distinct[0])); } // phi(v, v, ...) is just v
            }
            for inst in &b.insts {
                match *inst {
                    Inst::Copy { dst, src } => rewrites.push((dst, src)),
                    Inst::Un { dst, op, a: Val::Const(x) } =>
                        rewrites.push((dst, Val::Const(if op == UnOp::Neg { x.wrapping_neg() } else { (x == 0) as i64 }))),
                    Inst::Bin { dst, op, a: Val::Const(x), b: Val::Const(y) } => {
                        if let Some(c) = fold(op, x, y) { rewrites.push((dst, Val::Const(c))); }
                    }
                    // algebraic identities
                    Inst::Bin { dst, op: BinOp::Add, a, b: Val::Const(0) } | Inst::Bin { dst, op: BinOp::Add, a: Val::Const(0), b: a }
                    | Inst::Bin { dst, op: BinOp::Mul, a, b: Val::Const(1) } | Inst::Bin { dst, op: BinOp::Mul, a: Val::Const(1), b: a }
                    | Inst::Bin { dst, op: BinOp::Sub, a, b: Val::Const(0) } => rewrites.push((dst, a)),
                    Inst::Bin { dst, op: BinOp::Mul, b: Val::Const(0), .. } | Inst::Bin { dst, op: BinOp::Mul, a: Val::Const(0), .. } =>
                        rewrites.push((dst, Val::Const(0))),
                    _ => {}
                }
            }
        }
        // Branches on constants become jumps; the dropped successor loses this predecessor.
        let mut changed_branch = false;
        for b in 0..f.blocks.len() {
            if let Term::Branch { cond: Val::Const(c), then, els } = f.blocks[b].term.clone() {
                let (keep, drop) = if c != 0 { (then, els) } else { (els, then) };
                f.blocks[b].term = Term::Jump(keep);
                if keep != drop {
                    for phi in f.blocks[drop].phis.iter_mut() { phi.args.retain(|(p, _)| *p != b); }
                }
                changed_branch = true;
            }
        }
        if rewrites.is_empty() && !changed_branch { break; }
        total += rewrites.len() + changed_branch as usize;
        // Resolve chains inside the batch (a -> b, b -> 3 means a -> 3) before applying anything.
        let map: HashMap<u32, Val> = rewrites.iter().copied().collect();
        let resolve = |mut v: Val| {
            for _ in 0..map.len() {
                match v { Val::Var(x) if map.contains_key(&x) => v = map[&x], _ => break }
            }
            v
        };
        let rewrites: Vec<(u32, Val)> = rewrites.iter().map(|&(d, v)| (d, resolve(v))).collect();
        for (dst, v) in rewrites {
            if v == Val::Var(dst) { continue; } // a cycle of trivial phis: leave it to DCE
            replace_uses(f, dst, v);
            for b in f.blocks.iter_mut() {
                b.phis.retain(|p| p.dst != dst);
                b.insts.retain(|i| !matches!(i, Inst::Copy { dst: d, .. } | Inst::Bin { dst: d, .. } | Inst::Un { dst: d, .. } if *d == dst));
            }
        }
        remove_unreachable_ssa(f);
    }
    total
}

fn remove_unreachable_ssa(f: &mut SsaFn) {
    let mut seen = vec![false; f.blocks.len()];
    let mut stack = vec![0];
    while let Some(b) = stack.pop() {
        if std::mem::replace(&mut seen[b], true) { continue; }
        stack.extend(f.blocks[b].term.succs());
    }
    if seen.iter().all(|&s| s) { return; }
    let mut new_id = vec![usize::MAX; f.blocks.len()];
    let mut n = 0;
    for (b, &s) in seen.iter().enumerate() { if s { new_id[b] = n; n += 1; } }
    let old = std::mem::take(&mut f.blocks);
    for (b, mut bb) in old.into_iter().enumerate() {
        if !seen[b] { continue; }
        bb.term = match bb.term {
            Term::Jump(t) => Term::Jump(new_id[t]),
            Term::Branch { cond, then, els } => Term::Branch { cond, then: new_id[then], els: new_id[els] },
            r => r,
        };
        for phi in bb.phis.iter_mut() {
            phi.args.retain(|(p, _)| seen[*p]);
            phi.args.iter_mut().for_each(|(p, _)| *p = new_id[*p]);
        }
        f.blocks.push(bb);
    }
}

/// The CFG shape of an SSA function, so the dominator code from listing 17.6-1 can be reused.
fn skeleton(f: &SsaFn) -> FnIr {
    FnIr { name: f.name.clone(), params: vec![], vars: vec![],
           blocks: f.blocks.iter().map(|b| BasicBlock { insts: vec![], term: Some(b.term.clone()) }).collect() }
}

/// Pass 2: global value numbering over the dominator tree. An expression computed in a block is
/// available in every block that block dominates, so a later identical computation reuses it.
fn gvn(f: &mut SsaFn) -> usize {
    let idom = dominators(&skeleton(f));
    let mut children = vec![Vec::new(); f.blocks.len()];
    for b in 1..f.blocks.len() { children[idom[b]].push(b); }
    let mut table: Vec<((u8, Val, Val), u32)> = Vec::new(); // a scoped table: truncated on the way back up
    let mut rewrites = Vec::new();
    let mut merged: HashMap<u32, u32> = HashMap::new(); // values already replaced, applied to later keys
    fn rank(v: Val) -> (u8, i64) {
        match v { Val::Var(x) => (0, x as i64), Val::Const(c) => (1, c) }
    }
    fn walk(b: usize, f: &SsaFn, children: &[Vec<usize>], table: &mut Vec<((u8, Val, Val), u32)>,
            rewrites: &mut Vec<(u32, u32)>, merged: &mut HashMap<u32, u32>) {
        let mark = table.len();
        let canon = |v: Val, merged: &HashMap<u32, u32>| match v {
            Val::Var(x) => Val::Var(*merged.get(&x).unwrap_or(&x)),
            c => c,
        };
        for inst in &f.blocks[b].insts {
            let (dst, key) = match *inst {
                Inst::Bin { dst, op, a, b } if !may_trap(inst) => {
                    let (a, b) = (canon(a, merged), canon(b, merged));
                    let commutative = matches!(op, BinOp::Add | BinOp::Mul | BinOp::Eq | BinOp::Ne);
                    let (a, b) = if commutative && rank(b) < rank(a) { (b, a) } else { (a, b) };
                    (dst, (op as u8, a, b))
                }
                Inst::Un { dst, op, a } => {
                    let a = canon(a, merged);
                    (dst, (100 + op as u8, a, a))
                }
                _ => continue,
            };
            match table.iter().find(|(k, _)| *k == key) {
                Some(&(_, existing)) => { rewrites.push((dst, existing)); merged.insert(dst, existing); }
                None => table.push((key, dst)),
            }
        }
        for &c in &children[b] { walk(c, f, children, table, rewrites, merged); }
        table.truncate(mark);
    }
    walk(0, f, &children, &mut table, &mut rewrites, &mut merged);
    for &(dst, existing) in &rewrites {
        replace_uses(f, dst, Val::Var(existing));
        for b in f.blocks.iter_mut() {
            b.insts.retain(|i| !matches!(i, Inst::Bin { dst: d, .. } | Inst::Un { dst: d, .. } if *d == dst));
        }
    }
    rewrites.len()
}

/// Pass 3: dead-code elimination (mark and sweep). Roots: terminators, and instructions that may
/// trap or have effects. Everything they transitively use is live; the rest is deleted.
fn dce(f: &mut SsaFn) -> usize {
    let mut live = vec![false; f.names.len()];
    let mut work: Vec<Val> = Vec::new();
    for b in &f.blocks {
        match &b.term { Term::Branch { cond, .. } => work.push(*cond), Term::Return(v) => work.push(*v), Term::Jump(_) => {} }
        for i in &b.insts {
            if may_trap(i) { work.extend(operands(i)); }
        }
    }
    // def site of each value: the operands it needs
    let mut def_uses: Vec<Vec<Val>> = vec![Vec::new(); f.names.len()];
    for b in &f.blocks {
        for phi in &b.phis { def_uses[phi.dst as usize] = phi.args.iter().map(|a| a.1).collect(); }
        for i in &b.insts {
            let d = match i { Inst::Copy { dst, .. } | Inst::Bin { dst, .. } | Inst::Un { dst, .. } | Inst::Call { dst, .. } => *dst };
            def_uses[d as usize] = operands(i);
        }
    }
    while let Some(v) = work.pop() {
        if let Val::Var(x) = v {
            if !std::mem::replace(&mut live[x as usize], true) { work.extend(def_uses[x as usize].iter().copied()); }
        }
    }
    let mut removed = 0;
    for b in f.blocks.iter_mut() {
        let before = b.phis.len() + b.insts.len();
        b.phis.retain(|p| live[p.dst as usize]);
        b.insts.retain(|i| may_trap(i) || match i {
            Inst::Copy { dst, .. } | Inst::Bin { dst, .. } | Inst::Un { dst, .. } | Inst::Call { dst, .. } => live[*dst as usize],
        });
        removed += before - b.phis.len() - b.insts.len();
    }
    removed
}

/// Pass 4: merge a block into its predecessor when it is the only successor of a block that is
/// its only predecessor (straight-line code split by a `jump`). LLVM's SimplifyCFG does this and more.
fn merge_blocks(f: &mut SsaFn) -> usize {
    let mut merged = 0;
    loop {
        let preds = preds(&skeleton(f));
        let candidate = (0..f.blocks.len()).find_map(|a| match f.blocks[a].term {
            Term::Jump(b) if b != 0 && b != a && preds[b] == [a] && f.blocks[b].phis.is_empty() => Some((a, b)),
            _ => None,
        });
        let Some((a, b)) = candidate else { break };
        let moved = std::mem::take(&mut f.blocks[b].insts);
        f.blocks[a].insts.extend(moved);
        f.blocks[a].term = f.blocks[b].term.clone();
        f.blocks[b].term = Term::Return(Val::Const(0)); // b is now unreachable
        for s in f.blocks[a].term.succs() {
            for phi in f.blocks[s].phis.iter_mut() {
                phi.args.iter_mut().for_each(|(p, _)| if *p == b { *p = a });
            }
        }
        remove_unreachable_ssa(f);
        merged += 1;
    }
    merged
}

fn count(f: &SsaFn) -> usize {
    f.blocks.iter().map(|b| b.phis.len() + b.insts.len() + 1).sum()
}

fn main() {
    let src = "
fn demo(x: int) -> int {
    let scale = 4 * 25;
    let a = x * scale + 1;
    let b = x * scale + 1;
    let unused = a / 3;
    let debug = false;
    if debug { a } else { a + b }
}
fn guarded(x: int, d: int) -> int {
    let unused = x / d;
    if d != 0 { x / d } else { 0 }
}
fn sum_to(n: int) -> int {
    let mut i = 0;
    let mut total = 0;
    while i < n {
        i = i + 1;
        total = total + i;
    }
    total
}
fn classify(a: int, b: int) -> int {
    if a > 0 && b / a > 2 { 1 } else if a == 0 || b < 0 { 2 } else { 3 }
}";
    let items = parse_program(src).expect("parses");
    let before: HashMap<String, SsaFn> = items.iter().map(|f| (f.name.clone(), to_ssa(&lower_fn(f)))).collect();
    let mut after = before.clone();

    print_ssa(&before["demo"]);
    for name in ["demo", "guarded", "sum_to", "classify"] {
        let f = after.get_mut(name).unwrap();
        let s = simplify(f);
        let g = gvn(f);
        let d = dce(f);
        let s2 = simplify(f); // clean up what GVN and DCE exposed
        let m = merge_blocks(f);
        println!("{name:<9} {:>3} -> {:>3} instructions, {} -> {} blocks  (simplify {s}+{s2}, gvn {g}, dce {d}, merge {m})",
            count(&before[name]), count(f), before[name].blocks.len(), f.blocks.len());
    }
    println!();
    print_ssa(&after["demo"]);
    print_ssa(&after["guarded"]);

    // Same answers, fewer instructions executed?
    let (mut steps_before, mut steps_after, mut runs) = (0u64, 0u64, 0);
    for x in -6..=6i64 {
        for y in -3..=9i64 {
            for (name, args) in [("demo", vec![x]), ("guarded", vec![x, y]), ("sum_to", vec![y]), ("classify", vec![x, y])] {
                let r1 = run_ssa(&before, name, &args, &mut steps_before);
                let r2 = run_ssa(&after, name, &args, &mut steps_after);
                assert_eq!(r1, r2, "{name}{args:?}");
                runs += 1;
            }
        }
    }
    println!("identical results on {runs} runs; dynamic IR instructions {steps_before} -> {steps_after}");
    println!("guarded(1, 0) = {:?} (the dead `x / d` is kept: removing it would hide a run-time error)",
        run_ssa(&after, "guarded", &[1, 0], &mut 0));
}
