// verify: debug ok
// Part XVII review, listing R-1: "sieve-compiler v0.1", a pull request as submitted.
// Sieve is Meridian's rule language for risk checks, e.g.  amount > 1000 && country == "DE".
// The author's demo harness is in main(). Review the compiler, not the harness.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
enum Tok { Ident(String), Int(i64), Str(String), Op(&'static str) }

const OPS: [&str; 10] = ["==", "!=", "<=", ">=", "&&", "||", "<", ">", "*", "+"];

fn lex(src: &str) -> Vec<Tok> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() { i += 1; }
            let s: String = chars[start..i].iter().collect();
            out.push(Tok::Int(s.parse().unwrap()));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') { i += 1; }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
        } else if c == '"' {
            let start = i + 1;
            i += 1;
            while chars[i] != '"' { i += 1; }
            out.push(Tok::Str(chars[start..i].iter().collect()));
            i += 1;
        } else {
            let rest: String = chars[i..].iter().take(2).collect();
            let op = OPS.iter().find(|op| rest.starts_with(**op)).expect("unknown operator");
            out.push(Tok::Op(*op));
            i += op.len();
        }
    }
    out
}

#[derive(Debug, Clone)]
enum Expr { Int(i64), Str(String), Feature(String), Bin(&'static str, Box<Expr>, Box<Expr>) }

fn prec(op: &str) -> u8 {
    match op {
        "||" => 1,
        "&&" => 2,
        "==" | "!=" | "<" | "<=" | ">" | ">=" => 3,
        "+" => 4,
        "*" => 5,
        _ => 0,
    }
}

fn parse(toks: &[Tok], at: &mut usize, min_prec: u8) -> Result<Expr, String> {
    let mut lhs = match toks.get(*at) {
        Some(Tok::Int(n)) => Expr::Int(*n),
        Some(Tok::Str(s)) => Expr::Str(s.clone()),
        Some(Tok::Ident(f)) => Expr::Feature(f.clone()),
        _ => return Err("parse error".into()),
    };
    *at += 1;
    while let Some(Tok::Op(op)) = toks.get(*at) {
        let p = prec(op);
        if p < min_prec { break; }
        *at += 1;
        let rhs = parse(toks, at, p + 1)?;
        lhs = Expr::Bin(*op, Box::new(lhs), Box::new(rhs));
    }
    Ok(lhs)
}

/// Constant folding, so rules like `amount > 1000 * 1000` cost nothing at run time.
fn fold(e: Expr) -> Expr {
    match e {
        Expr::Bin(op, a, b) => match (op, fold(*a), fold(*b)) {
            ("*", Expr::Int(x), Expr::Int(y)) => Expr::Int(x * y),
            ("+", Expr::Int(x), Expr::Int(y)) => Expr::Int(x + y),
            (op, a, b) => Expr::Bin(op, Box::new(a), Box::new(b)),
        },
        other => other,
    }
}

#[derive(Debug, Clone, PartialEq)]
enum V { Int(i64), Str(String), Bool(bool) }

struct Record<'a> {
    fields: HashMap<&'static str, V>,
    fetch_graph_score: &'a dyn Fn() -> i64, // remote call to the graph service
}

fn feature(r: &Record, name: &str) -> V {
    if name == "graph_score" {
        return V::Int((r.fetch_graph_score)());
    }
    r.fields.get(name).cloned().unwrap_or(V::Int(0))
}

fn eval(e: &Expr, r: &Record) -> V {
    match e {
        Expr::Int(n) => V::Int(*n),
        Expr::Str(s) => V::Str(s.clone()),
        Expr::Feature(f) => feature(r, f),
        Expr::Bin(op, a, b) => {
            let (x, y) = (eval(a, r), eval(b, r));
            let as_int = |v: &V| match v { V::Int(n) => Some(*n), V::Bool(b) => Some(*b as i64), V::Str(_) => None };
            V::Bool(match *op {
                "&&" => x == V::Bool(true) && y == V::Bool(true),
                "||" => x == V::Bool(true) || y == V::Bool(true),
                "==" => x == y,
                "!=" => x != y,
                cmp => match (as_int(&x), as_int(&y)) {
                    (Some(p), Some(q)) => match cmp { "<" => p < q, "<=" => p <= q, ">" => p > q, _ => p >= q },
                    _ => false,
                },
            })
        }
    }
}

/// The entry point: evaluate a rule against a record.
fn check(rule: &str, r: &Record) -> Result<bool, String> {
    let toks = lex(rule);
    let e = fold(parse(&toks, &mut 0, 0)?);
    Ok(eval(&e, r) == V::Bool(true))
}

fn main() {
    std::panic::set_hook(Box::new(|_| {})); // the harness reports panics itself
    let fetches = std::cell::Cell::new(0);
    let fetch = || { fetches.set(fetches.get() + 1); 50 };
    let rec = |amount: i64, country: &str, velocity: i64| Record {
        fields: HashMap::from([("amount", V::Int(amount)), ("country", V::Str(country.into())), ("velocity_1h", V::Int(velocity))]),
        fetch_graph_score: &fetch,
    };
    let demo = [
        ("country == \"D\u{0415}\"", rec(10, "DE", 0)),
        ("amount > 99999999999999999999", rec(10, "DE", 0)),
        ("1 < amount < 1000", rec(5000, "DE", 0)),
        ("velocty_1h > 20", rec(10, "DE", 50)),
        ("amount > \"1000\"", rec(5000, "DE", 0)),
        ("amount > 1000 * 1000 * 1000 * 1000 * 1000 * 1000 * 1000", rec(10, "DE", 0)),
        ("amount >", rec(10, "DE", 0)),
    ];
    for (i, (rule, r)) in demo.iter().enumerate() {
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(rule, r)));
        let shown = match out {
            Ok(res) => format!("{res:?}"),
            Err(p) => format!("COMPILER PANICKED: {}", p.downcast_ref::<String>().cloned()
                .or(p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()),
        };
        println!("[{}] {rule:<58} -> {shown}", i + 1);
    }
    let rule = "amount > 100 && graph_score > 80";
    let decisions: usize = (0..10_000).map(|i| check(rule, &rec(i % 200, "DE", 0)).unwrap() as usize).sum();
    println!("[8] {rule}: {decisions} matches, graph_score fetched {} times for 10,000 records", fetches.get());
}
