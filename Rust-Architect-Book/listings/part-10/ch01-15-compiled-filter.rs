// verify: release ok
// verify: debug ok
// Log-sampling rules from config ("status>=500 || path^=/payments && latency>250"), evaluated three ways:
// walking an AST per record, a tree of closures built once ("closure compilation"), and hand-written code.
// Best of 7 runs per variant; one Playground run, noisy.
use std::hint::black_box;
use std::time::Instant;

struct Record {
    status: u16,
    latency_ms: u32,
    path: String,
}

enum Expr {
    StatusGe(u16),
    LatencyGt(u32),
    PathPrefix(String),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

/// Parses `a || b && c` with && binding tighter than ||. No parentheses: enough for the sampler's config.
fn parse(src: &str) -> Result<Expr, String> {
    let mut ors = src.split("||").map(|conj| {
        let mut ands = conj.split("&&").map(|atom| {
            let atom = atom.trim();
            if let Some(v) = atom.strip_prefix("status>=") {
                v.parse().map(Expr::StatusGe).map_err(|e| format!("{atom}: {e}"))
            } else if let Some(v) = atom.strip_prefix("latency>") {
                v.parse().map(Expr::LatencyGt).map_err(|e| format!("{atom}: {e}"))
            } else if let Some(v) = atom.strip_prefix("path^=") {
                Ok(Expr::PathPrefix(v.to_string()))
            } else {
                Err(format!("unknown condition {atom:?}"))
            }
        });
        let first = ands.next().ok_or("empty rule")??;
        ands.try_fold(first, |acc, e| Ok::<_, String>(Expr::And(Box::new(acc), Box::new(e?))))
    });
    let first = ors.next().ok_or("empty rule")??;
    ors.try_fold(first, |acc, e| Ok(Expr::Or(Box::new(acc), Box::new(e?))))
}

/// Interpretation: a match per node, per record.
fn eval(e: &Expr, r: &Record) -> bool {
    match e {
        Expr::StatusGe(s) => r.status >= *s,
        Expr::LatencyGt(l) => r.latency_ms > *l,
        Expr::PathPrefix(p) => r.path.starts_with(p.as_str()),
        Expr::And(a, b) => eval(a, r) && eval(b, r),
        Expr::Or(a, b) => eval(a, r) || eval(b, r),
    }
}

type Pred = Box<dyn Fn(&Record) -> bool + Send + Sync>;

/// Closure compilation: walk the AST ONCE, producing nested closures that capture their operands.
fn compile(e: Expr) -> Pred {
    match e {
        Expr::StatusGe(s) => Box::new(move |r| r.status >= s),
        Expr::LatencyGt(l) => Box::new(move |r| r.latency_ms > l),
        Expr::PathPrefix(p) => Box::new(move |r| r.path.starts_with(p.as_str())),
        Expr::And(a, b) => {
            let (a, b) = (compile(*a), compile(*b));
            Box::new(move |r| a(r) && b(r))
        }
        Expr::Or(a, b) => {
            let (a, b) = (compile(*a), compile(*b));
            Box::new(move |r| a(r) || b(r))
        }
    }
}

fn time(label: &str, records: &[Record], f: impl Fn(&Record) -> bool) {
    let mut best = f64::MAX;
    let mut kept = 0;
    for _ in 0..7 {
        let t = Instant::now();
        kept = black_box(records).iter().filter(|r| f(r)).count();
        best = best.min(t.elapsed().as_secs_f64());
    }
    println!("{label:<24} {:>6.2} ns/record   kept {kept}", best * 1e9 / records.len() as f64);
}

fn main() {
    let rule = "status>=500 || path^=/payments && latency>250";
    let paths = ["/payments/charge", "/catalog/items", "/payments/refund", "/health"];
    let records: Vec<Record> = (0..200_000u32)
        .map(|i| Record {
            status: if i % 50 == 0 { 503 } else { 200 },
            latency_ms: (i * 7919) % 400,
            path: paths[(i % 4) as usize].to_string(),
        })
        .collect();

    let ast = parse(rule).expect("valid rule");
    time("AST interpreter", &records, |r| eval(&ast, r));
    let compiled = compile(parse(rule).expect("valid rule"));
    time("compiled closures", &records, |r| compiled(r));
    time("hand-written", &records, |r| {
        r.status >= 500 || (r.path.starts_with("/payments") && r.latency_ms > 250)
    });
    println!("bad rule: {:?}", parse("status>=5xx").err());
}
