// verify: debug ok
#![allow(dead_code)] // the structs are read only through Debug in this demo
use std::collections::HashMap;
use std::num::ParseIntError;

#[derive(Debug)]
struct PoolConfig {
    max_conns: u32,
    idle_timeout_s: Option<u32>, // optional in the file; None means "never time out"
}

#[derive(Debug)]
enum ConfigError {
    Missing(&'static str),
    NotANumber(&'static str, ParseIntError),
}

fn parse_pool(raw: &HashMap<&str, &str>) -> Result<PoolConfig, ConfigError> {
    let max_conns = raw
        .get("max_conns")
        .ok_or(ConfigError::Missing("max_conns"))? // Option -> Result, then `?`
        .parse::<u32>()
        .map_err(|e| ConfigError::NotANumber("max_conns", e))?;

    let idle_timeout_s = raw
        .get("idle_timeout_s")
        .map(|v| v.parse::<u32>()) // Option<Result<u32, _>>
        .transpose() // Result<Option<u32>, _>
        .map_err(|e| ConfigError::NotANumber("idle_timeout_s", e))?;

    Ok(PoolConfig { max_conns, idle_timeout_s })
}

fn main() {
    let cases = [
        HashMap::from([("max_conns", "64"), ("idle_timeout_s", "30")]),
        HashMap::from([("max_conns", "64")]),
        HashMap::from([("idle_timeout_s", "30")]),
        HashMap::from([("max_conns", "64"), ("idle_timeout_s", "soon")]),
    ];
    for raw in &cases {
        println!("{:?}", parse_pool(raw));
    }
}
