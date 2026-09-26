// verify: debug ok
// Listing 17.3-5: parse-error recovery. On an error the parser records a diagnostic, puts an
// `<error>` node in the AST (rustc has `ExprKind::Err` for this), and synchronizes: it skips to a
// token where a statement can safely restart. Compared with naive "skip one token" recovery.

#[derive(Debug, Clone, PartialEq)]
enum T { Let, Name(String), Num(i64), Op(char), Eof }

#[derive(Debug, Clone)]
struct Tok { t: T, lo: usize, hi: usize }

fn lex(src: &str) -> Vec<Tok> {
    let (b, mut i, mut out) = (src.as_bytes(), 0, Vec::new());
    while i < b.len() {
        let s = i;
        let t = match b[i] {
            c if c.is_ascii_whitespace() => { i += 1; continue; }
            c if c.is_ascii_digit() => {
                while i < b.len() && b[i].is_ascii_digit() { i += 1; }
                T::Num(src[s..i].parse().unwrap())
            }
            c if c.is_ascii_alphabetic() => {
                while i < b.len() && b[i].is_ascii_alphanumeric() { i += 1; }
                if &src[s..i] == "let" { T::Let } else { T::Name(src[s..i].to_string()) }
            }
            c => { i += 1; T::Op(c as char) }
        };
        out.push(Tok { t, lo: s, hi: i });
    }
    out.push(Tok { t: T::Eof, lo: src.len(), hi: src.len() });
    out
}

#[derive(Debug)]
enum Stmt { Let(String, String), Expr(String), Error }

struct Diag { msg: String, lo: usize, hi: usize }

#[derive(Clone, Copy, PartialEq)]
enum Recovery { Synchronize, SkipOneToken }

struct Parser { toks: Vec<Tok>, at: usize, diags: Vec<Diag>, mode: Recovery }

impl Parser {
    fn peek(&self) -> &Tok { &self.toks[self.at] }
    fn bump(&mut self) -> Tok {
        let t = self.toks[self.at].clone();
        if t.t != T::Eof { self.at += 1; }
        t
    }
    fn fail<X>(&self, what: &str) -> Result<X, Diag> {
        let t = self.peek();
        let found = match &t.t {
            T::Let => "`let`".to_string(), T::Name(n) => format!("`{n}`"), T::Num(n) => format!("`{n}`"),
            T::Op(c) => format!("`{c}`"), T::Eof => "end of input".to_string(),
        };
        Err(Diag { msg: format!("expected {what}, found {found}"), lo: t.lo, hi: t.hi.max(t.lo + 1) })
    }
    fn expect_op(&mut self, c: char) -> Result<(), Diag> {
        if self.peek().t == T::Op(c) { self.bump(); Ok(()) } else { self.fail(&format!("`{c}`")) }
    }

    // expr := term (('+'|'-'|'*') term)*      term := NUM | NAME | '(' expr ')'
    fn expr(&mut self) -> Result<String, Diag> {
        let mut lhs = self.term()?;
        while let T::Op(op @ ('+' | '-' | '*')) = self.peek().t {
            self.bump();
            let rhs = self.term()?;
            lhs = format!("({op} {lhs} {rhs})");
        }
        Ok(lhs)
    }
    fn term(&mut self) -> Result<String, Diag> {
        match self.peek().t.clone() {
            T::Num(n) => { self.bump(); Ok(n.to_string()) }
            T::Name(n) => { self.bump(); Ok(n) }
            T::Op('(') => {
                self.bump();
                let e = self.expr()?;
                self.expect_op(')')?;
                Ok(e)
            }
            _ => self.fail("an expression"),
        }
    }
    fn stmt(&mut self) -> Result<Stmt, Diag> {
        if self.peek().t == T::Let {
            self.bump();
            let name = match self.peek().t.clone() {
                T::Name(n) => { self.bump(); n }
                _ => return self.fail("a name after `let`"),
            };
            self.expect_op('=')?;
            let e = self.expr()?;
            self.expect_op(';')?;
            Ok(Stmt::Let(name, e))
        } else {
            let e = self.expr()?;
            self.expect_op(';')?;
            Ok(Stmt::Expr(e))
        }
    }

    fn recover(&mut self) {
        match self.mode {
            Recovery::SkipOneToken => { self.bump(); }
            Recovery::Synchronize => loop {
                match self.peek().t {
                    T::Op(';') => { self.bump(); return; } // end of the broken statement
                    T::Let | T::Eof => return,              // a new statement starts here
                    _ => { self.bump(); }
                }
            },
        }
    }

    fn program(&mut self) -> Vec<Stmt> {
        let mut out = Vec::new();
        while self.peek().t != T::Eof {
            match self.stmt() {
                Ok(s) => out.push(s),
                Err(d) => {
                    self.diags.push(d);
                    out.push(Stmt::Error);
                    self.recover();
                }
            }
        }
        out
    }
}

/// rustc-style diagnostic: message, location, the source line, and a caret under the span.
fn render(src: &str, d: &Diag) -> String {
    let line_start = src[..d.lo].rfind('\n').map_or(0, |i| i + 1);
    let line_end = src[d.lo..].find('\n').map_or(src.len(), |i| d.lo + i);
    let line_no = src[..d.lo].matches('\n').count() + 1;
    let col = d.lo - line_start + 1;
    format!(
        "error: {}\n --> line {line_no}, col {col}\n  |\n{line_no} | {}\n  | {}{}",
        d.msg,
        &src[line_start..line_end],
        " ".repeat(d.lo - line_start),
        "^".repeat(d.hi - d.lo)
    )
}

fn main() {
    let src = "let a = 1 + ;\nlet 5 = x * 2;\nlet c = (a + 3;\nlet e = 2 3 4 5;\nlet d = a + c;";
    for mode in [Recovery::Synchronize, Recovery::SkipOneToken] {
        let mut p = Parser { toks: lex(src), at: 0, diags: Vec::new(), mode };
        let stmts = p.program();
        let name = if mode == Recovery::Synchronize { "synchronize at `;` / `let`" } else { "skip one token" };
        println!("=== recovery: {name}: {} errors, {} statements", p.diags.len(), stmts.len());
        for s in &stmts {
            match s {
                Stmt::Let(n, e) => println!("    (let {n} {e})"),
                Stmt::Expr(e) => println!("    (expr {e})    <- phantom statement"),
                Stmt::Error => println!("    <error>"),
            }
        }
        if mode == Recovery::Synchronize {
            for d in &p.diags {
                println!("{}\n", render(src, d));
            }
        } else {
            for d in &p.diags {
                println!("    {}", d.msg);
            }
        }
    }
}
