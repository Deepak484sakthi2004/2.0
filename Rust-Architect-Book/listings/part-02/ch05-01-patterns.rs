// verify: debug ok
fn describe_status(code: u16) -> &'static str {
    match code {
        200 | 204 => "ok",
        300..=399 => "redirect",
        429 => "rate limited",
        400..=499 => "client error",
        500..=599 => "server error",
        _ => "unknown",
    }
}

fn describe_point(p: (i32, i32)) -> String {
    match p {
        (0, 0) => "origin".to_string(),
        (x, 0) | (0, x) => format!("on an axis at {x}"),
        (x, y) if x == y => format!("on the diagonal at {x}"),
        (x @ 1..=9, y @ 1..=9) => format!("small positive ({x}, {y})"),
        (x, y) => format!("elsewhere ({x}, {y})"),
    }
}

fn summarize(latencies: &[u32]) -> String {
    match latencies {
        [] => "no samples".to_string(),
        [only] => format!("one sample: {only}"),
        [first, .., last] => format!("{} samples, first {first}, last {last}", latencies.len()),
    }
}

fn parse_kv(line: &str) -> Option<(&str, u32)> {
    // let-else: bind on success, or diverge.
    let Some((key, value)) = line.split_once('=') else {
        return None;
    };
    let Ok(value) = value.trim().parse::<u32>() else {
        return None;
    };
    Some((key.trim(), value))
}

fn main() {
    for code in [200, 302, 429, 404, 503, 700] {
        println!("{code} -> {}", describe_status(code));
    }
    for p in [(0, 0), (5, 0), (3, 3), (2, 7), (-4, 12)] {
        println!("{p:?} -> {}", describe_point(p));
    }
    let cases: [&[u32]; 3] = [&[], &[42], &[10, 20, 30]];
    for s in cases {
        println!("{}", summarize(s));
    }
    for line in ["timeout = 30", "retries=x", "no equals sign"] {
        println!("{line:?} -> {:?}", parse_kv(line));
    }

    // if-let chains (edition 2024): a pattern match and a condition in one `if`.
    let config = parse_kv("max_conns = 512");
    if let Some((key, n)) = config
        && n > 256
    {
        println!("{key} is high: {n}");
    }

    // matches!: a pattern test as a bool.
    let is_retryable = |code: u16| matches!(code, 429 | 502..=504);
    println!("503 retryable: {}, 404 retryable: {}", is_retryable(503), is_retryable(404));
}
