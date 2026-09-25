// verify: debug ok
use std::collections::HashMap;

#[derive(Debug)]
struct PoolConfig {
    max_conns: u32,
    idle_timeout_s: Option<u32>, // None means "never time out"
}

fn parse_pool(raw: &HashMap<&str, &str>) -> Option<PoolConfig> {
    let max_conns = raw.get("max_conns")?.parse().ok()?;
    let idle_timeout_s = raw.get("idle_timeout_s").and_then(|v| v.parse().ok());
    Some(PoolConfig { max_conns, idle_timeout_s })
}

fn main() {
    let typo = HashMap::from([("max_conns", "64"), ("idle_timeout_s", "30s")]);
    let cfg = parse_pool(&typo).expect("config loads");
    println!("{cfg:?}");
    match cfg.idle_timeout_s {
        Some(s) => println!("idle connections close after {s}s (max {})", cfg.max_conns),
        None => println!("idle connections are never closed (max {})", cfg.max_conns),
    }
}
