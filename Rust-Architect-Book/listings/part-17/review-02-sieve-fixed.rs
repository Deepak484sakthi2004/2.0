// verify: debug ok
// verify: debug test
// Part XVII review, listing R-2: sieve-compiler v0.2, after review. Every stage reports errors with
// spans instead of panicking or guessing; names and types are checked against the feature schema at
// compile time; rules are compiled once; `&&`/`||` short-circuit, so remote features are fetched lazily.

use std::cell::Cell;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span { pub lo: usize, pub hi: usize }

#[derive(Debug)]
pub struct Diag { pub span: Span, pub msg: String }

fn diag<T>(lo: usize, hi: usize, msg: impl Into<String>) -> Result<T, Diag> {
    Err(Diag { span: Span { lo, hi }, msg: msg.into() })
}

// ---------------- lexer: ASCII identifiers and literals, checked numbers, every error kept ----------------
#[derive(Debug, Clone, PartialEq)]
enum Tok { Ident(String), Int(i64), Str(String), Op(&'static str), LParen, RParen, Eof }

#[derive(Debug, Clone)]
struct Token { tok: Tok, span: Span }

const OPS: [&str; 10] = ["==", "!=", "<=", ">=", "&&", "||", "<", ">", "*", "+"];

fn lex(src: &str) -> Result<Vec<Token>, Vec<Diag>> {
    let b = src.as_bytes();
    let (mut i, mut out, mut errs) = (0, Vec::new(), Vec::new());
    while i < b.len() {
        let (c, lo) = (b[i], i);
        let tok = if c.is_ascii_whitespace() {
            i += 1;
            continue;
        } else if c.is_ascii_digit() {
            while i < b.len() && b[i].is_ascii_digit() { i += 1; }
            match src[lo..i].parse() {
                Ok(n) => Tok::Int(n),
                Err(_) => { errs.push(Diag { span: Span { lo, hi: i }, msg: "integer literal is too large".into() }); continue; }
            }
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') { i += 1; }
            Tok::Ident(src[lo..i].to_string())
        } else if c == b'"' {
            i += 1;
            while i < b.len() && b[i] != b'"' { i += 1; }
            if i == b.len() {
                errs.push(Diag { span: Span { lo, hi: i }, msg: "unterminated string literal".into() });
                continue;
            }
            i += 1;
            let text = &src[lo + 1..i - 1];
            if let Some((off, ch)) = text.char_indices().find(|(_, ch)| !ch.is_ascii()) {
                let at = lo + 1 + off;
                errs.push(Diag { span: Span { lo: at, hi: at + ch.len_utf8() },
                    msg: format!("non-ASCII character U+{:04X} in a string literal (Sieve literals are ASCII)", ch as u32) });
                continue;
            }
            Tok::Str(text.to_string())
        } else if c == b'(' || c == b')' {
            i += 1;
            if c == b'(' { Tok::LParen } else { Tok::RParen }
        } else if let Some(op) = OPS.iter().find(|op| src[i..].starts_with(**op)) {
            i += op.len();
            Tok::Op(*op)
        } else {
            let ch = src[i..].chars().next().unwrap();
            i += ch.len_utf8();
            errs.push(Diag { span: Span { lo, hi: i }, msg: format!("unexpected character {ch:?}") });
            continue;
        };
        out.push(Token { tok, span: Span { lo, hi: i } });
    }
    out.push(Token { tok: Tok::Eof, span: Span { lo: b.len(), hi: b.len() } });
    if errs.is_empty() { Ok(out) } else { Err(errs) }
}

// ---------------- parser: non-associative comparisons, && tighter than ||, a depth limit ----------------
#[derive(Debug, Clone)]
enum Expr { Int(i64), Str(String), Feature(String), Bin(&'static str, Box<Node>, Box<Node>) }

/// Every node keeps its span, so later stages can point at the exact sub-expression.
#[derive(Debug, Clone)]
struct Node { e: Expr, span: Span }

const MAX_DEPTH: usize = 64;

fn prec(op: &str) -> u8 {
    match op { "||" => 1, "&&" => 2, "==" | "!=" | "<" | "<=" | ">" | ">=" => 3, "+" => 4, "*" => 5, _ => 0 }
}

struct Parser { toks: Vec<Token>, at: usize, depth: usize }

impl Parser {
    fn expr(&mut self, min_prec: u8) -> Result<Node, Diag> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            let s = self.toks[self.at].span;
            return diag(s.lo, s.hi, format!("rule nested more than {MAX_DEPTH} levels deep"));
        }
        let t = self.toks[self.at].clone();
        self.at += 1;
        let mut lhs = match t.tok {
            Tok::Int(n) => Node { e: Expr::Int(n), span: t.span },
            Tok::Str(s) => Node { e: Expr::Str(s), span: t.span },
            Tok::Ident(f) => Node { e: Expr::Feature(f), span: t.span },
            Tok::LParen => {
                let inner = self.expr(0)?;
                let close = self.toks[self.at].clone();
                if close.tok != Tok::RParen { return diag(close.span.lo, close.span.hi, "expected `)`"); }
                self.at += 1;
                inner
            }
            _ => return diag(t.span.lo, t.span.hi.max(t.span.lo + 1), "expected a value or a feature name"),
        };
        while let Tok::Op(op) = self.toks[self.at].tok {
            let p = prec(op);
            if p < min_prec { break; }
            self.at += 1;
            let rhs = self.expr(p + 1)?;
            let span = Span { lo: lhs.span.lo, hi: rhs.span.hi };
            lhs = Node { e: Expr::Bin(op, Box::new(lhs), Box::new(rhs)), span };
            if p == 3 && matches!(self.toks[self.at].tok, Tok::Op(o) if prec(o) == 3) {
                let s = self.toks[self.at].span;
                return diag(s.lo, s.hi, "comparison operators cannot be chained; use `&&`");
            }
        }
        self.depth -= 1;
        Ok(lhs)
    }
}

// ---------------- names and types, against the feature schema ----------------
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ty { Int, Str, Bool }

const SCHEMA: [(&str, Ty); 4] = [("amount", Ty::Int), ("country", Ty::Str), ("velocity_1h", Ty::Int), ("graph_score", Ty::Int)];

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

/// Type-checks and constant-folds (with overflow checking) in one bottom-up pass.
fn check(n: Node) -> Result<(Node, Ty), Diag> {
    let span = n.span;
    match n.e {
        Expr::Int(_) => Ok((n, Ty::Int)),
        Expr::Str(_) => Ok((n, Ty::Str)),
        Expr::Feature(ref f) => match SCHEMA.iter().find(|(name, _)| *name == f.as_str()) {
            Some(&(_, t)) => Ok((n, t)),
            None => {
                let best = SCHEMA.iter().map(|(name, _)| (edit_distance(f, name), *name)).min().filter(|(d, _)| *d <= 2);
                let help = best.map(|(_, name)| format!("; did you mean `{name}`?")).unwrap_or_default();
                diag(span.lo, span.hi, format!("unknown feature `{f}`{help}"))
            }
        },
        Expr::Bin(op, a, b) => {
            let ((a, ta), (b, tb)) = (check(*a)?, check(*b)?);
            let ty = match op {
                "&&" | "||" if ta == Ty::Bool && tb == Ty::Bool => Ty::Bool,
                "==" | "!=" if ta == tb => Ty::Bool,
                "<" | "<=" | ">" | ">=" if ta == Ty::Int && tb == Ty::Int => Ty::Bool,
                "*" | "+" if ta == Ty::Int && tb == Ty::Int => Ty::Int,
                _ => return diag(span.lo, span.hi, format!("mismatched types: {ta:?} {op} {tb:?}")),
            };
            if let (Expr::Int(x), Expr::Int(y)) = (&a.e, &b.e) {
                let folded = if op == "*" { x.checked_mul(*y) } else if op == "+" { x.checked_add(*y) } else { None };
                match folded {
                    Some(v) => return Ok((Node { e: Expr::Int(v), span }, Ty::Int)),
                    None if op == "*" || op == "+" => return diag(span.lo, span.hi, "constant expression overflows i64"),
                    None => {}
                }
            }
            Ok((Node { e: Expr::Bin(op, Box::new(a), Box::new(b)), span }, ty))
        }
    }
}

// ---------------- the compiled rule ----------------
pub struct Record<'a> { pub amount: i64, pub country: &'a str, pub velocity_1h: i64, pub graph_score: &'a dyn Fn() -> i64 }

pub struct Rule { expr: Node }

#[derive(Debug, PartialEq)]
enum V { Int(i64), Str(String), Bool(bool) }

impl Rule {
    pub fn compile(src: &str) -> Result<Rule, Vec<Diag>> {
        let toks = lex(src)?;
        let mut p = Parser { toks, at: 0, depth: 0 };
        let node = p.expr(0).map_err(|d| vec![d])?;
        if p.toks[p.at].tok != Tok::Eof {
            let s = p.toks[p.at].span;
            return Err(vec![Diag { span: s, msg: "unexpected input after the rule".into() }]);
        }
        let span = node.span;
        let (expr, ty) = check(node).map_err(|d| vec![d])?;
        if ty != Ty::Bool {
            return Err(vec![Diag { span, msg: format!("a rule must be a condition, found {ty:?}") }]);
        }
        Ok(Rule { expr })
    }

    pub fn eval(&self, r: &Record) -> bool {
        fn go(n: &Node, r: &Record) -> V {
            match &n.e {
                Expr::Int(n) => V::Int(*n),
                Expr::Str(s) => V::Str(s.clone()),
                Expr::Feature(f) => match f.as_str() {
                    "amount" => V::Int(r.amount),
                    "country" => V::Str(r.country.to_string()),
                    "velocity_1h" => V::Int(r.velocity_1h),
                    _ => V::Int((r.graph_score)()), // only reached when the rule actually needs it
                },
                Expr::Bin("&&", a, b) => V::Bool(go(a, r) == V::Bool(true) && go(b, r) == V::Bool(true)),
                Expr::Bin("||", a, b) => V::Bool(go(a, r) == V::Bool(true) || go(b, r) == V::Bool(true)),
                Expr::Bin(op, a, b) => match (go(a, r), go(b, r)) {
                    (V::Int(x), V::Int(y)) => match *op {
                        "<" => V::Bool(x < y), "<=" => V::Bool(x <= y), ">" => V::Bool(x > y), ">=" => V::Bool(x >= y),
                        "==" => V::Bool(x == y), "!=" => V::Bool(x != y), "*" => V::Int(x.wrapping_mul(y)), _ => V::Int(x.wrapping_add(y)),
                    },
                    (x, y) => V::Bool(if *op == "==" { x == y } else { x != y }), // types were checked
                },
            }
        }
        go(&self.expr, r) == V::Bool(true)
    }
}

pub fn render(src: &str, d: &Diag) -> String {
    let hi = d.span.hi.min(src.len());
    let width = src[d.span.lo.min(hi)..hi].chars().count().max(1);
    let col = src[..d.span.lo].chars().count();
    format!("error: {}\n      | {src}\n      | {}{}", d.msg, " ".repeat(col), "^".repeat(width))
}

fn main() {
    let fetches = Cell::new(0);
    let fetch = || { fetches.set(fetches.get() + 1); 50 };
    let rules = [
        "country == \"D\u{0415}\"",
        "amount > 99999999999999999999",
        "1 < amount < 1000",
        "velocty_1h > 20",
        "amount > \"1000\"",
        "amount > 1000 * 1000 * 1000 * 1000 * 1000 * 1000 * 1000",
        "amount >",
    ];
    for (i, src) in rules.iter().enumerate() {
        match Rule::compile(src) {
            Ok(_) => println!("[{}] {src}: compiled", i + 1),
            Err(ds) => ds.iter().for_each(|d| println!("[{}] {}", i + 1, render(src, d))),
        }
    }
    let rule = Rule::compile("amount > 100 && graph_score > 80").unwrap(); // compiled ONCE
    let matches = (0..10_000)
        .filter(|i| rule.eval(&Record { amount: i % 200, country: "DE", velocity_1h: 0, graph_score: &fetch }))
        .count();
    println!("[8] {matches} matches, graph_score fetched {} times for 10,000 records", fetches.get());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec<'a>(amount: i64, country: &'a str, g: &'a dyn Fn() -> i64) -> Record<'a> {
        Record { amount, country, velocity_1h: 7, graph_score: g }
    }
    fn err(src: &str) -> String {
        Rule::compile(src).err().expect("should not compile")[0].msg.clone()
    }

    #[test]
    fn rules_evaluate() {
        let g = || 90;
        let r = Rule::compile("amount > 100 && country == \"DE\" || graph_score > 80").unwrap();
        assert!(r.eval(&rec(150, "DE", &g)));
        assert!(r.eval(&rec(5, "FR", &g))); // || binds loosest: the graph score alone suffices
        let r = Rule::compile("(amount > 100 || velocity_1h > 5) && country != \"DE\"").unwrap();
        assert!(r.eval(&rec(1, "FR", &g)));
        assert!(!r.eval(&rec(500, "DE", &g)));
    }

    #[test]
    fn review_findings_are_compile_errors() {
        assert!(err("country == \"D\u{0415}\"").contains("non-ASCII"));
        assert!(err("amount > 99999999999999999999").contains("too large"));
        assert!(err("1 < amount < 1000").contains("chained"));
        assert!(err("velocty_1h > 20").contains("did you mean `velocity_1h`"));
        assert!(err("amount > \"1000\"").contains("mismatched types"));
        assert!(err("amount > 1000 * 1000 * 1000 * 1000 * 1000 * 1000 * 1000").contains("overflows"));
        assert!(err("amount >").contains("expected a value"));
        assert!(err("amount + 1").contains("must be a condition"));
        assert!(err(&format!("{}amount > 1{}", "(".repeat(100), ")".repeat(100))).contains("nested"));
        assert!(err("country == \"DE").contains("unterminated"));
    }

    #[test]
    fn remote_features_are_fetched_lazily() {
        let n = Cell::new(0);
        let g = || { n.set(n.get() + 1); 99 };
        let r = Rule::compile("amount > 100 && graph_score > 80").unwrap();
        assert!(!r.eval(&rec(5, "DE", &g)));
        assert_eq!(n.get(), 0);
        assert!(r.eval(&rec(500, "DE", &g)));
        assert_eq!(n.get(), 1);
    }
}
