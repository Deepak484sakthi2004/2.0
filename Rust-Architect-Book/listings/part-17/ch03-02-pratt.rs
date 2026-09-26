// verify: debug ok
// verify: debug test
// Listing 17.3-2: Ore expressions by Pratt parsing (top-down operator precedence, Pratt 1973).
// All precedence and associativity lives in three small tables; one loop does the parsing.
// `**` is NOT part of Ore: it is added here to show that a new operator is one table row.

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(i64),
    Name(String),
    Op(&'static str),
    Eof,
}

const OPS: [&str; 22] = [
    "**", "==", "!=", "<=", ">=", "&&", "||", "+", "-", "*", "/", "%", "<", ">", "=", "!", "(", ")", ",", "{", "}", ";",
];

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let (b, mut i, mut out) = (src.as_bytes(), 0, Vec::new());
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let s = i;
            while i < b.len() && b[i].is_ascii_digit() { i += 1; }
            out.push(Tok::Num(src[s..i].parse().map_err(|e| format!("{e}"))?));
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') { i += 1; }
            out.push(Tok::Name(src[s..i].to_string()));
        } else {
            // OPS lists two-byte operators first: maximal munch.
            let op = OPS.iter().find(|op| src[i..].starts_with(**op)).ok_or(format!("bad char at {i}"))?;
            out.push(Tok::Op(*op));
            i += op.len();
        }
    }
    out.push(Tok::Eof);
    Ok(out)
}

// ---------------- the three tables ----------------
/// Prefix operators: right binding power.
fn prefix_bp(op: &str) -> Option<u8> {
    match op {
        "-" | "!" => Some(13),
        _ => None,
    }
}

/// Infix operators: (left bp, right bp). left < right: left-associative; left > right: right-associative.
fn infix_bp(op: &str) -> Option<(u8, u8)> {
    Some(match op {
        "=" => (2, 1), // right-assoc: a = b = c  is  a = (b = c)
        "||" => (3, 4),
        "&&" => (5, 6),
        "==" | "!=" | "<" | "<=" | ">" | ">=" => (7, 8), // non-associative: checked below
        "+" | "-" => (9, 10),
        "*" | "/" | "%" => (11, 12),
        "**" => (16, 15), // right-assoc, and tighter than prefix minus: -2 ** 2 is -(2 ** 2)
        _ => return None,
    })
}

/// Postfix operators: left binding power. A call `f(...)` is a postfix operator.
fn postfix_bp(op: &str) -> Option<u8> {
    match op {
        "(" => Some(17),
        _ => None,
    }
}

fn is_cmp(t: &Tok) -> bool {
    matches!(t, Tok::Op("==" | "!=" | "<" | "<=" | ">" | ">="))
}

// ---------------- the parser: one loop ----------------
#[derive(Debug)]
enum S {
    Atom(String),
    Cons(String, Vec<S>),
}

impl std::fmt::Display for S {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            S::Atom(a) => write!(f, "{a}"),
            S::Cons(head, rest) => {
                write!(f, "({head}")?;
                for s in rest {
                    write!(f, " {s}")?;
                }
                write!(f, ")")
            }
        }
    }
}

struct Pratt {
    toks: Vec<Tok>,
    at: usize,
}

impl Pratt {
    fn peek(&self) -> &Tok {
        &self.toks[self.at]
    }
    fn next(&mut self) -> Tok {
        let t = self.toks[self.at].clone();
        if t != Tok::Eof { self.at += 1; }
        t
    }

    /// Parses an expression whose operators all bind at least as tightly as `min_bp`.
    fn expr_bp(&mut self, min_bp: u8) -> Result<S, String> {
        let mut lhs = match self.next() {
            Tok::Num(n) => S::Atom(n.to_string()),
            Tok::Name(x) => S::Atom(x),
            Tok::Op("(") => {
                let e = self.expr_bp(0)?;
                if self.next() != Tok::Op(")") { return Err("expected `)`".into()); }
                e
            }
            Tok::Op(op) => match prefix_bp(op) {
                Some(r_bp) => S::Cons(op.to_string(), vec![self.expr_bp(r_bp)?]),
                None => return Err(format!("unexpected `{op}`")),
            },
            Tok::Eof => return Err("unexpected end of input".into()),
        };
        loop {
            let op = match self.peek() {
                Tok::Op(op) => *op,
                Tok::Eof => break,
                t => return Err(format!("expected an operator, found {t:?}")),
            };
            if let Some(l_bp) = postfix_bp(op) {
                if l_bp < min_bp { break; }
                self.next();
                let mut args = vec![lhs];
                while *self.peek() != Tok::Op(")") {
                    args.push(self.expr_bp(0)?);
                    if *self.peek() == Tok::Op(",") { self.next(); } else { break; }
                }
                if self.next() != Tok::Op(")") { return Err("expected `)` after arguments".into()); }
                lhs = S::Cons("call".into(), args);
                continue;
            }
            if let Some((l_bp, r_bp)) = infix_bp(op) {
                if l_bp < min_bp { break; }
                self.next();
                let rhs = self.expr_bp(r_bp)?;
                lhs = S::Cons(op.to_string(), vec![lhs, rhs]);
                if is_cmp(&Tok::Op(op)) && is_cmp(self.peek()) {
                    return Err("comparison operators cannot be chained".into());
                }
                continue;
            }
            break;
        }
        Ok(lhs)
    }
}

fn parse(src: &str) -> Result<String, String> {
    let mut p = Pratt { toks: lex(src)?, at: 0 };
    let e = p.expr_bp(0)?;
    match p.peek() {
        Tok::Eof => Ok(e.to_string()),
        t => Err(format!("trailing input: {t:?}")),
    }
}

fn main() {
    println!("binding powers (higher binds tighter):");
    for op in ["=", "||", "&&", "<", "+", "*", "**"] {
        let (l, r) = infix_bp(op).unwrap();
        let assoc = if l < r { "left" } else { "right" };
        println!("  {op:<3} l={l:<2} r={r:<2} {assoc}{}", if op == "<" { " (non-assoc, checked)" } else { "" });
    }
    println!("  prefix - !  r={}   postfix call  l={}", prefix_bp("-").unwrap(), postfix_bp("(").unwrap());
    println!();
    for src in ["1 + 2 * 3", "a || b && c", "x = y = 3", "-2 ** 2", "2 ** 3 ** 2", "f(a, b + 1)(c) * -d", "a < b < c", "(1 + 2"] {
        match parse(src) {
            Ok(s) => println!("  {src:<22} => {s}"),
            Err(e) => println!("  {src:<22} => error: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    /// The same cases as listing 17.3-1's tests: Pratt and recursive descent must agree.
    #[test]
    fn agrees_with_recursive_descent() {
        for (src, want) in [
            ("1 + 2 * 3", "(+ 1 (* 2 3))"),
            ("(1 + 2) * 3", "(* (+ 1 2) 3)"),
            ("a || b && c", "(|| a (&& b c))"),
            ("a + 1 < b * 2 && c", "(&& (< (+ a 1) (* b 2)) c)"),
            ("-x * y", "(* (- x) y)"),
            ("!a == b", "(== (! a) b)"),
            ("1 - 2 - 3", "(- (- 1 2) 3)"),
            ("a / b / c", "(/ (/ a b) c)"),
            ("x = y = 3", "(= x (= y 3))"),
            ("f(1)(2)", "(call (call f 1) 2)"),
        ] {
            assert_eq!(parse(src).unwrap(), want, "input: {src}");
        }
    }

    #[test]
    fn the_extension() {
        assert_eq!(parse("-2 ** 2").unwrap(), "(- (** 2 2))");
        assert_eq!(parse("2 ** 3 ** 2").unwrap(), "(** 2 (** 3 2))");
        assert!(parse("a < b < c").is_err());
    }
}
