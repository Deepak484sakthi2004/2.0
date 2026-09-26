// verify: debug ok
// Listing 17.1-1: a whole compiler pipeline for a tiny subset of Ore, in one file.
// Stages: lex -> parse -> resolve -> type-check -> lower to stack bytecode -> run on a VM.
// main() compiles one good program, then shows which stage rejects each of five bad ones.

use std::collections::HashMap;

#[derive(Debug)]
struct CompileError {
    stage: &'static str,
    pos: usize,
    msg: String,
}

fn fail<T>(stage: &'static str, pos: usize, msg: impl Into<String>) -> Result<T, CompileError> {
    Err(CompileError { stage, pos, msg: msg.into() })
}

// ---------------- Stage 1: lexer (bytes -> tokens) ----------------
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Int(i64),
    Ident(String),
    Let, Mut, While, If, Else, True, False,
    Plus, Minus, Star, Slash, Percent,
    Lt, Le, Gt, Ge, EqEq, Ne, Assign, AndAnd, OrOr, Bang,
    Semi, LBrace, RBrace, LParen, RParen,
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    pos: usize, // byte offset of the token's first byte
}

fn lex(src: &str) -> Result<Vec<Token>, CompileError> {
    let b = src.as_bytes();
    let (mut i, mut out) = (0, Vec::new());
    while i < b.len() {
        let c = b[i];
        let start = i;
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() {
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let Ok(n) = src[start..i].parse::<i64>() else {
                return fail("lexer", start, "integer literal too large");
            };
            out.push(Token { tok: Tok::Int(n), pos: start });
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let tok = match &src[start..i] {
                "let" => Tok::Let,
                "mut" => Tok::Mut,
                "while" => Tok::While,
                "if" => Tok::If,
                "else" => Tok::Else,
                "true" => Tok::True,
                "false" => Tok::False,
                word => Tok::Ident(word.to_string()),
            };
            out.push(Token { tok, pos: start });
            continue;
        }
        // Maximal munch: a two-byte operator wins over its one-byte prefix.
        let (tok, len) = match (c, b.get(i + 1).copied()) {
            (b'<', Some(b'=')) => (Tok::Le, 2),
            (b'>', Some(b'=')) => (Tok::Ge, 2),
            (b'=', Some(b'=')) => (Tok::EqEq, 2),
            (b'!', Some(b'=')) => (Tok::Ne, 2),
            (b'&', Some(b'&')) => (Tok::AndAnd, 2),
            (b'|', Some(b'|')) => (Tok::OrOr, 2),
            (b'+', _) => (Tok::Plus, 1),
            (b'-', _) => (Tok::Minus, 1),
            (b'*', _) => (Tok::Star, 1),
            (b'/', _) => (Tok::Slash, 1),
            (b'%', _) => (Tok::Percent, 1),
            (b'<', _) => (Tok::Lt, 1),
            (b'>', _) => (Tok::Gt, 1),
            (b'=', _) => (Tok::Assign, 1),
            (b'!', _) => (Tok::Bang, 1),
            (b';', _) => (Tok::Semi, 1),
            (b'{', _) => (Tok::LBrace, 1),
            (b'}', _) => (Tok::RBrace, 1),
            (b'(', _) => (Tok::LParen, 1),
            (b')', _) => (Tok::RParen, 1),
            _ => {
                let ch = src[start..].chars().next().unwrap();
                return fail("lexer", start, format!("unexpected character {ch:?}"));
            }
        };
        out.push(Token { tok, pos: start });
        i += len;
    }
    out.push(Token { tok: Tok::Eof, pos: b.len() });
    Ok(out)
}

// ---------------- Stage 2: parser (tokens -> AST) ----------------
#[derive(Debug, Clone, Copy, PartialEq)]
enum BinOp { Add, Sub, Mul, Div, Rem, Lt, Le, Gt, Ge, Eq, Ne, And, Or }

#[derive(Debug, Clone, Copy)]
enum UnOp { Neg, Not }

#[derive(Debug)]
enum Expr {
    Int(i64),
    Bool(bool),
    Var(String, usize), // name, position (the key for the resolver's side table)
    Unary(UnOp, Box<Expr>, usize),
    Binary(BinOp, Box<Expr>, Box<Expr>, usize),
}

#[derive(Debug)]
enum Stmt {
    Let { name: String, mutable: bool, init: Expr, pos: usize },
    Assign { name: String, value: Expr, pos: usize },
    While { cond: Expr, body: Vec<Stmt> },
    If { cond: Expr, then_body: Vec<Stmt>, else_body: Vec<Stmt> },
}

struct Program {
    stmts: Vec<Stmt>,
    result: Expr,
}

struct Parser {
    toks: Vec<Token>,
    at: usize,
}

/// Binary operators by precedence level: 0 = `||` (loosest) ... 4 = `* / %` (tightest).
fn binop_of(t: &Tok) -> Option<(u8, BinOp)> {
    Some(match t {
        Tok::OrOr => (0, BinOp::Or),
        Tok::AndAnd => (1, BinOp::And),
        Tok::EqEq => (2, BinOp::Eq),
        Tok::Ne => (2, BinOp::Ne),
        Tok::Lt => (2, BinOp::Lt),
        Tok::Le => (2, BinOp::Le),
        Tok::Gt => (2, BinOp::Gt),
        Tok::Ge => (2, BinOp::Ge),
        Tok::Plus => (3, BinOp::Add),
        Tok::Minus => (3, BinOp::Sub),
        Tok::Star => (4, BinOp::Mul),
        Tok::Slash => (4, BinOp::Div),
        Tok::Percent => (4, BinOp::Rem),
        _ => return None,
    })
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.toks[self.at]
    }
    fn bump(&mut self) -> Token {
        let t = self.toks[self.at].clone();
        if t.tok != Tok::Eof {
            self.at += 1;
        }
        t
    }
    fn expect(&mut self, want: Tok, what: &str) -> Result<Token, CompileError> {
        if self.peek().tok == want {
            Ok(self.bump())
        } else {
            fail("parser", self.peek().pos, format!("expected {what}, found {:?}", self.peek().tok))
        }
    }
    fn ident(&mut self) -> Result<(String, usize), CompileError> {
        let t = self.bump();
        match t.tok {
            Tok::Ident(name) => Ok((name, t.pos)),
            other => fail("parser", t.pos, format!("expected a name, found {other:?}")),
        }
    }

    fn program(&mut self) -> Result<Program, CompileError> {
        let mut stmts = Vec::new();
        loop {
            match self.peek().tok {
                Tok::Let | Tok::While | Tok::If => stmts.push(self.stmt()?),
                Tok::Ident(_) if self.toks[self.at + 1].tok == Tok::Assign => stmts.push(self.stmt()?),
                _ => {
                    let result = self.expr(0)?;
                    self.expect(Tok::Eof, "end of program")?;
                    return Ok(Program { stmts, result });
                }
            }
        }
    }

    fn block(&mut self) -> Result<Vec<Stmt>, CompileError> {
        self.expect(Tok::LBrace, "`{`")?;
        let mut body = Vec::new();
        while self.peek().tok != Tok::RBrace {
            body.push(self.stmt()?);
        }
        self.bump();
        Ok(body)
    }

    fn stmt(&mut self) -> Result<Stmt, CompileError> {
        match self.peek().tok {
            Tok::Let => {
                self.bump();
                let mutable = self.peek().tok == Tok::Mut;
                if mutable {
                    self.bump();
                }
                let (name, pos) = self.ident()?;
                self.expect(Tok::Assign, "`=`")?;
                let init = self.expr(0)?;
                self.expect(Tok::Semi, "`;`")?;
                Ok(Stmt::Let { name, mutable, init, pos })
            }
            Tok::While => {
                self.bump();
                let cond = self.expr(0)?;
                let body = self.block()?;
                Ok(Stmt::While { cond, body })
            }
            Tok::If => {
                self.bump();
                let cond = self.expr(0)?;
                let then_body = self.block()?;
                self.expect(Tok::Else, "`else`")?;
                let else_body = self.block()?;
                Ok(Stmt::If { cond, then_body, else_body })
            }
            _ => {
                let (name, pos) = self.ident()?;
                self.expect(Tok::Assign, "`=`")?;
                let value = self.expr(0)?;
                self.expect(Tok::Semi, "`;`")?;
                Ok(Stmt::Assign { name, value, pos })
            }
        }
    }

    /// Precedence climbing by levels: parse operands one level tighter, loop over operators of this level.
    fn expr(&mut self, level: u8) -> Result<Expr, CompileError> {
        if level == 5 {
            return self.unary();
        }
        let mut lhs = self.expr(level + 1)?;
        while let Some((l, op)) = binop_of(&self.peek().tok) {
            if l != level {
                break;
            }
            let pos = self.bump().pos;
            let rhs = self.expr(level + 1)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs), pos);
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, CompileError> {
        let t = self.bump();
        match t.tok {
            Tok::Minus => Ok(Expr::Unary(UnOp::Neg, Box::new(self.unary()?), t.pos)),
            Tok::Bang => Ok(Expr::Unary(UnOp::Not, Box::new(self.unary()?), t.pos)),
            Tok::Int(n) => Ok(Expr::Int(n)),
            Tok::True => Ok(Expr::Bool(true)),
            Tok::False => Ok(Expr::Bool(false)),
            Tok::Ident(name) => Ok(Expr::Var(name, t.pos)),
            Tok::LParen => {
                let e = self.expr(0)?;
                self.expect(Tok::RParen, "`)`")?;
                Ok(e)
            }
            other => fail("parser", t.pos, format!("expected an expression, found {other:?}")),
        }
    }
}

fn sexpr(e: &Expr) -> String {
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Bool(b) => b.to_string(),
        Expr::Var(name, _) => name.clone(),
        Expr::Unary(op, x, _) => format!("({} {})", if matches!(op, UnOp::Neg) { "-" } else { "!" }, sexpr(x)),
        Expr::Binary(op, a, b, _) => {
            let s = match op {
                BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*", BinOp::Div => "/",
                BinOp::Rem => "%", BinOp::Lt => "<", BinOp::Le => "<=", BinOp::Gt => ">",
                BinOp::Ge => ">=", BinOp::Eq => "==", BinOp::Ne => "!=", BinOp::And => "&&",
                BinOp::Or => "||",
            };
            format!("({s} {} {})", sexpr(a), sexpr(b))
        }
    }
}

fn print_stmts(stmts: &[Stmt], indent: usize) {
    let pad = "  ".repeat(indent);
    for s in stmts {
        match s {
            Stmt::Let { name, mutable, init, .. } => {
                println!("{pad}(let{} {name} {})", if *mutable { " mut" } else { "" }, sexpr(init))
            }
            Stmt::Assign { name, value, .. } => println!("{pad}(set {name} {})", sexpr(value)),
            Stmt::While { cond, body } => {
                println!("{pad}(while {}", sexpr(cond));
                print_stmts(body, indent + 1);
                println!("{pad})");
            }
            Stmt::If { cond, then_body, else_body } => {
                println!("{pad}(if {}", sexpr(cond));
                print_stmts(then_body, indent + 1);
                println!("{pad} else");
                print_stmts(else_body, indent + 1);
                println!("{pad})");
            }
        }
    }
}

// ---------------- Stage 3: name resolution (names -> slots, in a side table) ----------------
struct Slot {
    name: String,
    mutable: bool,
}

struct Resolver {
    scopes: Vec<HashMap<String, usize>>, // innermost scope last
    slots: Vec<Slot>,
    uses: HashMap<usize, usize>, // side table: position of a name in the source -> slot
}

impl Resolver {
    fn lookup(&self, name: &str, pos: usize) -> Result<usize, CompileError> {
        for scope in self.scopes.iter().rev() {
            if let Some(&slot) = scope.get(name) {
                return Ok(slot);
            }
        }
        fail("resolver", pos, format!("cannot find `{name}` in this scope"))
    }
    fn expr(&mut self, e: &Expr) -> Result<(), CompileError> {
        match e {
            Expr::Int(_) | Expr::Bool(_) => Ok(()),
            Expr::Var(name, pos) => {
                let slot = self.lookup(name, *pos)?;
                self.uses.insert(*pos, slot);
                Ok(())
            }
            Expr::Unary(_, x, _) => self.expr(x),
            Expr::Binary(_, a, b, _) => {
                self.expr(a)?;
                self.expr(b)
            }
        }
    }
    fn stmts(&mut self, stmts: &[Stmt]) -> Result<(), CompileError> {
        for s in stmts {
            match s {
                Stmt::Let { name, mutable, init, pos } => {
                    self.expr(init)?; // resolve the initializer BEFORE the new name is in scope
                    let slot = self.slots.len();
                    self.slots.push(Slot { name: name.clone(), mutable: *mutable });
                    self.scopes.last_mut().unwrap().insert(name.clone(), slot);
                    self.uses.insert(*pos, slot);
                }
                Stmt::Assign { name, value, pos } => {
                    self.expr(value)?;
                    let slot = self.lookup(name, *pos)?;
                    if !self.slots[slot].mutable {
                        return fail("resolver", *pos, format!("cannot assign twice to immutable `{name}`"));
                    }
                    self.uses.insert(*pos, slot);
                }
                Stmt::While { cond, body } => {
                    self.expr(cond)?;
                    self.scopes.push(HashMap::new());
                    self.stmts(body)?;
                    self.scopes.pop();
                }
                Stmt::If { cond, then_body, else_body } => {
                    self.expr(cond)?;
                    for body in [then_body, else_body] {
                        self.scopes.push(HashMap::new());
                        self.stmts(body)?;
                        self.scopes.pop();
                    }
                }
            }
        }
        Ok(())
    }
}

// ---------------- Stage 4: type checking ----------------
#[derive(Debug, Clone, Copy, PartialEq)]
enum Ty { Int, Bool }

struct Checker<'r> {
    res: &'r Resolver,
    slot_ty: Vec<Option<Ty>>,
}

impl Checker<'_> {
    fn expr(&self, e: &Expr) -> Result<Ty, CompileError> {
        match e {
            Expr::Int(_) => Ok(Ty::Int),
            Expr::Bool(_) => Ok(Ty::Bool),
            Expr::Var(_, pos) => Ok(self.slot_ty[self.res.uses[pos]].expect("defined before use")),
            Expr::Unary(op, x, pos) => {
                let (want, t) = match op {
                    UnOp::Neg => (Ty::Int, self.expr(x)?),
                    UnOp::Not => (Ty::Bool, self.expr(x)?),
                };
                if t != want {
                    return fail("types", *pos, format!("operand must be {want:?}, found {t:?}"));
                }
                Ok(want)
            }
            Expr::Binary(op, a, b, pos) => {
                let (ta, tb) = (self.expr(a)?, self.expr(b)?);
                let (operand, result) = match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => (Ty::Int, Ty::Int),
                    BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => (Ty::Int, Ty::Bool),
                    BinOp::And | BinOp::Or => (Ty::Bool, Ty::Bool),
                    BinOp::Eq | BinOp::Ne => {
                        if ta != tb {
                            return fail("types", *pos, format!("cannot compare {ta:?} with {tb:?}"));
                        }
                        (ta, Ty::Bool)
                    }
                };
                if ta != operand || tb != operand {
                    return fail("types", *pos, format!("{op:?} needs {operand:?} operands, found {ta:?} and {tb:?}"));
                }
                Ok(result)
            }
        }
    }
    fn stmts(&mut self, stmts: &[Stmt]) -> Result<(), CompileError> {
        for s in stmts {
            match s {
                Stmt::Let { init, pos, .. } => {
                    let t = self.expr(init)?;
                    self.slot_ty[self.res.uses[pos]] = Some(t);
                }
                Stmt::Assign { value, pos, name } => {
                    let (t, slot_t) = (self.expr(value)?, self.slot_ty[self.res.uses[pos]].unwrap());
                    if t != slot_t {
                        return fail("types", *pos, format!("`{name}` is {slot_t:?}, cannot assign {t:?}"));
                    }
                }
                Stmt::While { cond, body } | Stmt::If { cond, then_body: body, .. } => {
                    if self.expr(cond)? != Ty::Bool {
                        return fail("types", 0, "condition must be Bool");
                    }
                    self.stmts(body)?;
                    if let Stmt::If { else_body, .. } = s {
                        self.stmts(else_body)?;
                    }
                }
            }
        }
        Ok(())
    }
}

// ---------------- Stage 5: lowering to stack bytecode ----------------
#[derive(Debug, Clone, Copy)]
enum Op {
    Push(i64),
    Load(usize),
    Store(usize),
    Bin(BinOp), // never And/Or: those become jumps
    Neg,
    Not,
    Jump(usize),
    JumpIfFalse(usize),
    Halt,
}

struct Lowerer<'r> {
    res: &'r Resolver,
    code: Vec<Op>,
}

impl Lowerer<'_> {
    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Int(n) => self.code.push(Op::Push(*n)),
            Expr::Bool(b) => self.code.push(Op::Push(*b as i64)),
            Expr::Var(_, pos) => self.code.push(Op::Load(self.res.uses[pos])),
            Expr::Unary(op, x, _) => {
                self.expr(x);
                self.code.push(if matches!(op, UnOp::Neg) { Op::Neg } else { Op::Not });
            }
            // Short-circuit operators are control flow, not arithmetic.
            Expr::Binary(op @ (BinOp::And | BinOp::Or), a, b, _) => {
                self.expr(a);
                if *op == BinOp::Or {
                    self.code.push(Op::Not);
                }
                let jump_short = self.code.len();
                self.code.push(Op::JumpIfFalse(0)); // patched below
                self.expr(b);
                let jump_end = self.code.len();
                self.code.push(Op::Jump(0));
                let short = self.code.len();
                self.code.push(Op::Push(if *op == BinOp::Or { 1 } else { 0 }));
                let end = self.code.len();
                self.code[jump_short] = Op::JumpIfFalse(short);
                self.code[jump_end] = Op::Jump(end);
            }
            Expr::Binary(op, a, b, _) => {
                self.expr(a);
                self.expr(b);
                self.code.push(Op::Bin(*op));
            }
        }
    }
    fn stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            match s {
                Stmt::Let { init: v, pos, .. } | Stmt::Assign { value: v, pos, .. } => {
                    self.expr(v);
                    self.code.push(Op::Store(self.res.uses[pos]));
                }
                Stmt::While { cond, body } => {
                    let top = self.code.len();
                    self.expr(cond);
                    let exit = self.code.len();
                    self.code.push(Op::JumpIfFalse(0));
                    self.stmts(body);
                    self.code.push(Op::Jump(top));
                    let end = self.code.len();
                    self.code[exit] = Op::JumpIfFalse(end);
                }
                Stmt::If { cond, then_body, else_body } => {
                    self.expr(cond);
                    let to_else = self.code.len();
                    self.code.push(Op::JumpIfFalse(0));
                    self.stmts(then_body);
                    let to_end = self.code.len();
                    self.code.push(Op::Jump(0));
                    let else_start = self.code.len();
                    self.stmts(else_body);
                    let end = self.code.len();
                    self.code[to_else] = Op::JumpIfFalse(else_start);
                    self.code[to_end] = Op::Jump(end);
                }
            }
        }
    }
}

// ---------------- Stage 6: the "machine": a stack VM ----------------
fn run(code: &[Op], nslots: usize) -> Result<(i64, usize), String> {
    let (mut stack, mut slots, mut pc, mut steps) = (Vec::new(), vec![0i64; nslots], 0, 0usize);
    loop {
        steps += 1;
        match code[pc] {
            Op::Push(n) => stack.push(n),
            Op::Load(s) => stack.push(slots[s]),
            Op::Store(s) => slots[s] = stack.pop().unwrap(),
            Op::Neg => {
                let v = stack.pop().unwrap();
                stack.push(v.wrapping_neg());
            }
            Op::Not => {
                let v = stack.pop().unwrap();
                stack.push((v == 0) as i64);
            }
            Op::Bin(op) => {
                let (b, a) = (stack.pop().unwrap(), stack.pop().unwrap());
                stack.push(match op {
                    BinOp::Add => a.wrapping_add(b),
                    BinOp::Sub => a.wrapping_sub(b),
                    BinOp::Mul => a.wrapping_mul(b),
                    BinOp::Div | BinOp::Rem if b == 0 => return Err(format!("division by zero at pc {pc}")),
                    BinOp::Div => a.wrapping_div(b),
                    BinOp::Rem => a.wrapping_rem(b),
                    BinOp::Lt => (a < b) as i64,
                    BinOp::Le => (a <= b) as i64,
                    BinOp::Gt => (a > b) as i64,
                    BinOp::Ge => (a >= b) as i64,
                    BinOp::Eq => (a == b) as i64,
                    BinOp::Ne => (a != b) as i64,
                    BinOp::And | BinOp::Or => unreachable!("lowered to jumps"),
                });
            }
            Op::Jump(t) => {
                pc = t;
                continue;
            }
            Op::JumpIfFalse(t) => {
                if stack.pop().unwrap() == 0 {
                    pc = t;
                    continue;
                }
            }
            Op::Halt => return Ok((stack.pop().unwrap(), steps)),
        }
        pc += 1;
    }
}

// ---------------- The driver ----------------
struct Compiled {
    code: Vec<Op>,
    nslots: usize,
}

fn compile(src: &str, verbose: bool) -> Result<Compiled, CompileError> {
    let toks = lex(src)?;
    if verbose {
        let shown: Vec<String> = toks.iter().take(12).map(|t| format!("{:?}", t.tok)).collect();
        println!("[lexer]    {} tokens: {} ...", toks.len(), shown.join(" "));
    }
    let program = Parser { toks, at: 0 }.program()?;
    if verbose {
        println!("[parser]   AST:");
        print_stmts(&program.stmts, 2);
        println!("    result: {}", sexpr(&program.result));
    }
    let mut res = Resolver { scopes: vec![HashMap::new()], slots: Vec::new(), uses: HashMap::new() };
    res.stmts(&program.stmts)?;
    res.expr(&program.result)?;
    if verbose {
        let slots: Vec<String> = res.slots.iter().enumerate()
            .map(|(i, s)| format!("{} -> slot {i}{}", s.name, if s.mutable { " (mut)" } else { "" }))
            .collect();
        println!("[resolver] {}; {} name uses resolved", slots.join(", "), res.uses.len());
    }
    let mut checker = Checker { res: &res, slot_ty: vec![None; res.slots.len()] };
    checker.stmts(&program.stmts)?;
    let t = checker.expr(&program.result)?;
    if verbose {
        println!("[types]    ok; slot types {:?}; result {t:?}", checker.slot_ty.iter().flatten().collect::<Vec<_>>());
    }
    let mut low = Lowerer { res: &res, code: Vec::new() };
    low.stmts(&program.stmts);
    low.expr(&program.result);
    low.code.push(Op::Halt);
    if verbose {
        println!("[lowering] {} instructions:", low.code.len());
        for (i, op) in low.code.iter().enumerate() {
            println!("    {i:>2}: {op:?}");
        }
    }
    Ok(Compiled { code: low.code, nslots: res.slots.len() })
}

fn main() {
    let good = "
        let mut i = 0;
        let mut total = 0;
        while i < 10 {
            i = i + 1;
            if i % 2 == 0 { total = total + i; } else { total = total - 1; }
        }
        total";
    let c = compile(good, true).expect("the good program compiles");
    match run(&c.code, c.nslots) {
        Ok((v, steps)) => println!("[vm]       result = {v} after {steps} instructions"),
        Err(e) => println!("[vm]       runtime error: {e}"),
    }

    println!();
    let bad = [
        "let x = 5 # 3; x",
        "let x = 5 let y = 6; x",
        "let x = 5; y + 1",
        "let x = 1; x = 2; x",
        "let x = 5; x + true",
        "let z = 0; 10 / z",
    ];
    for src in bad {
        match compile(src, false) {
            Err(e) => println!("{src:<26} => [{}] error at byte {}: {}", e.stage, e.pos, e.msg),
            Ok(c) => match run(&c.code, c.nslots) {
                Ok((v, _)) => println!("{src:<26} => ran: {v}"),
                Err(e) => println!("{src:<26} => compiled fine; [vm] {e}"),
            },
        }
    }
}
