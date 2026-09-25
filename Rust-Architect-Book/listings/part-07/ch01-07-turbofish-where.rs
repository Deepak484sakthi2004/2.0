// verify: debug ok
use std::collections::BTreeMap;
use std::fmt::Display;
use std::str::FromStr;

/// Parses "key=value" pairs into any key/value types that know how to parse themselves.
/// The `where` clause keeps a long list of bounds readable.
fn parse_pairs<K, V>(input: &str) -> Result<BTreeMap<K, V>, String>
where
    K: FromStr + Ord,
    V: FromStr,
    K::Err: Display,
    V::Err: Display,
{
    let mut out = BTreeMap::new();
    for pair in input.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').ok_or_else(|| format!("missing '=' in {pair:?}"))?;
        let key = k.trim().parse::<K>().map_err(|e| format!("bad key {k:?}: {e}"))?;
        let value = v.trim().parse::<V>().map_err(|e| format!("bad value {v:?}: {e}"))?;
        out.insert(key, value);
    }
    Ok(out)
}

fn main() {
    // Three ways to tell the compiler which types to instantiate:
    let port: u16 = "8080".parse().unwrap(); // 1. annotate the binding
    let retries = "3".parse::<u8>().unwrap(); // 2. turbofish on the call
    let limits = parse_pairs::<String, u32>("gold=1000, free=10").unwrap(); // 3. turbofish on a generic fn
    println!("port={port} retries={retries} limits={limits:?}");

    let weights: BTreeMap<u8, f64> = parse_pairs("1=0.5, 2=0.25").unwrap(); // inferred from the binding
    println!("weights={weights:?}");
    println!("{:?}", parse_pairs::<u8, u8>("1=300"));
}
