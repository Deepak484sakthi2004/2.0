// verify: debug ok
use std::collections::HashMap;

/// Absence is not an error: Option.
fn find_timeout<'a>(config: &HashMap<&str, &'a str>) -> Option<&'a str> {
    config.get("timeout_ms").copied()
}

/// Failure with a reason: Result.
fn parse_timeout(raw: &str) -> Result<u64, std::num::ParseIntError> {
    raw.trim().parse::<u64>()
}

/// Combining them: missing -> a default; present but malformed -> an error.
fn timeout_ms(config: &HashMap<&str, &str>) -> Result<u64, String> {
    match find_timeout(config) {
        None => Ok(1_000),
        Some(raw) => parse_timeout(raw).map_err(|e| format!("timeout_ms={raw:?}: {e}")),
    }
}

fn main() {
    let ok = HashMap::from([("timeout_ms", " 250 ")]);
    let missing: HashMap<&str, &str> = HashMap::new();
    let bad = HashMap::from([("timeout_ms", "25O")]); // a letter O, not a zero
    for (name, cfg) in [("ok", &ok), ("missing", &missing), ("bad", &bad)] {
        println!("{name:>7}: {:?}", timeout_ms(cfg));
    }

    // Combinators express the same kind of logic without an explicit match:
    let retries: Option<u32> = Some("3").and_then(|s| s.parse().ok()).filter(|&n| n <= 10);
    let port: Result<u16, String> = "70000".parse::<u16>().map_err(|e| e.to_string());
    println!("retries={retries:?} port={port:?}");
    println!("ok_or: {:?}", None::<u32>.ok_or("missing port"));
    println!("unwrap_or: {}", "x".parse::<u32>().unwrap_or(8080));
}
