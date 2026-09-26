// verify: debug ok
// verify: debug test
// Listing 17.3-1: the Ore parser, by recursive descent. One function per grammar rule and one per
// precedence level. Input: tokens from the Ore lexer (listing 17.2-1, condensed here: no strings).
// Output: an AST with a span on every node, printed as S-expressions.
#![allow(dead_code)] // some AST fields (spans of params and lets) are only used by later stages

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

fn main() {
    let src = "
fn fib(n: int) -> int {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
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

fn main() -> int {
    sum_to(10) + fib(10)
}";
    let items = parse_program(src).expect("parses");
    for f in &items {
        let params: Vec<String> = f.params.iter()
            .map(|p| format!("{}:{}", p.name, p.ty.as_deref().unwrap_or("_"))).collect();
        println!("(fn {} ({}) -> {} {})", f.name, params.join(" "),
            f.ret.as_deref().unwrap_or("()"), block_str(&f.body));
        println!("   span {}..{}", f.span.lo, f.span.hi);
    }

    for bad in ["fn f() { a < b < c }", "fn g() { 1 + }", "fn h() { 3 = x; }", "fn k() { let x = 1 x }"] {
        let e = parse_program(bad).unwrap_err();
        println!("{bad:<26} error at {}..{}: {}", e.span.lo, e.span.hi, e.msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(src: &str) -> String {
        let mut parser = Parser::new(lex(src).unwrap());
        let e = parser.expr().unwrap();
        assert_eq!(*parser.peek(), Tok::Eof, "input not fully consumed: {src}");
        sexpr(&e)
    }

    #[test]
    fn precedence() {
        assert_eq!(p("1 + 2 * 3"), "(+ 1 (* 2 3))");
        assert_eq!(p("(1 + 2) * 3"), "(* (+ 1 2) 3)");
        assert_eq!(p("a || b && c"), "(|| a (&& b c))");
        assert_eq!(p("a + 1 < b * 2 && c"), "(&& (< (+ a 1) (* b 2)) c)");
        assert_eq!(p("-x * y"), "(* (- x) y)");
        assert_eq!(p("!a == b"), "(== (! a) b)");
    }

    #[test]
    fn associativity() {
        assert_eq!(p("1 - 2 - 3"), "(- (- 1 2) 3)");
        assert_eq!(p("a / b / c"), "(/ (/ a b) c)");
        assert_eq!(p("x = y = 3"), "(= x (= y 3))");
        assert_eq!(p("f(1)(2)"), "(call (call f 1) 2)");
    }

    #[test]
    fn errors() {
        assert!(parse_program("fn f() { a < b < c }").unwrap_err().msg.contains("cannot be chained"));
        assert!(parse_program("fn f() { (1 + 2 }").is_err());
        assert!(parse_program("fn f() { x + 1 = 2; }").unwrap_err().msg.contains("left-hand side"));
    }
}
