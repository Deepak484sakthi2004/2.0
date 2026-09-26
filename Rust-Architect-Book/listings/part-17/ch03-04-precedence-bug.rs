// verify: debug ok
// Listing 17.3-4: the Sieve precedence incident. One wrong row in a binding-power table made
// `&&` and `||` equally strong, so a review rule silently changed meaning. Shadow mode (running the
// new engine next to the old one and diffing decisions) caught it.

use std::collections::HashMap;

#[derive(Debug)]
enum E {
    Num(i64),
    Var(String),
    Bin(&'static str, Box<E>, Box<E>),
}

type Table = fn(&str) -> Option<(u8, u8)>;

fn correct_bp(op: &str) -> Option<(u8, u8)> {
    Some(match op {
        "||" => (1, 2),
        "&&" => (3, 4),
        "<" | ">" => (5, 6),
        _ => return None,
    })
}

fn buggy_bp(op: &str) -> Option<(u8, u8)> {
    Some(match op {
        "||" | "&&" => (1, 2), // the bug: && no longer binds tighter than ||
        "<" | ">" => (5, 6),
        _ => return None,
    })
}

fn tokens(src: &str) -> Vec<String> {
    src.split_whitespace().map(str::to_string).collect()
}

fn parse(toks: &[String], at: &mut usize, min_bp: u8, table: Table) -> E {
    let t = &toks[*at];
    *at += 1;
    let mut lhs = match t.parse::<i64>() {
        Ok(n) => E::Num(n),
        Err(_) => E::Var(t.clone()),
    };
    while *at < toks.len() {
        let op: &'static str = match toks[*at].as_str() {
            "||" => "||", "&&" => "&&", "<" => "<", ">" => ">",
            other => panic!("unexpected token {other}"),
        };
        let Some((l, r)) = table(op) else { break };
        if l < min_bp {
            break;
        }
        *at += 1;
        let rhs = parse(toks, at, r, table);
        lhs = E::Bin(op, Box::new(lhs), Box::new(rhs));
    }
    lhs
}

fn show(e: &E) -> String {
    match e {
        E::Num(n) => n.to_string(),
        E::Var(v) => v.clone(),
        E::Bin(op, a, b) => format!("({op} {} {})", show(a), show(b)),
    }
}

fn eval(e: &E, rec: &HashMap<&str, i64>) -> i64 {
    match e {
        E::Num(n) => *n,
        E::Var(v) => rec[v.as_str()],
        E::Bin("||", a, b) => (eval(a, rec) != 0 || eval(b, rec) != 0) as i64,
        E::Bin("&&", a, b) => (eval(a, rec) != 0 && eval(b, rec) != 0) as i64,
        E::Bin("<", a, b) => (eval(a, rec) < eval(b, rec)) as i64,
        E::Bin(">", a, b) => (eval(a, rec) > eval(b, rec)) as i64,
        E::Bin(op, ..) => unreachable!("{op}"),
    }
}

fn main() {
    // Intent: review if the country is high-risk, OR the merchant is new AND high-volume.
    let rule = "high_risk_country || volume_30d > 50000 && age_days < 30";
    let toks = tokens(rule);
    let good = parse(&toks, &mut 0, 0, correct_bp);
    let bad = parse(&toks, &mut 0, 0, buggy_bp);
    println!("rule:    {rule}");
    println!("correct: {}", show(&good));
    println!("buggy:   {}", show(&bad));
    println!();

    let merchants = [
        ("old merchant, high-risk country", 1, 1_000, 400),
        ("new merchant, high volume", 0, 90_000, 10),
        ("new merchant, high-risk country", 1, 500, 5),
        ("old merchant, high volume", 0, 90_000, 900),
    ];
    for (name, hr, vol, age) in merchants {
        let rec = HashMap::from([("high_risk_country", hr), ("volume_30d", vol), ("age_days", age)]);
        println!("{name:<33} correct={} buggy={}", eval(&good, &rec), eval(&bad, &rec));
    }

    // Shadow mode: the same 10,000 synthetic merchants through both engines.
    let mut seed = 42u64;
    let mut next = |m: u64| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) % m) as i64
    };
    let (mut flagged_good, mut flagged_bad, mut diffs) = (0, 0, 0);
    for _ in 0..10_000 {
        let rec = HashMap::from([
            ("high_risk_country", (next(100) < 8) as i64),
            ("volume_30d", next(200_000)),
            ("age_days", next(2_000)),
        ]);
        let (g, b) = (eval(&good, &rec), eval(&bad, &rec));
        flagged_good += g;
        flagged_bad += b;
        diffs += (g != b) as i64;
    }
    println!("\nshadow mode, 10,000 merchants: correct flags {flagged_good}, buggy flags {flagged_bad}, decisions differ {diffs}");
}
