// verify: debug ok
use std::collections::HashMap;
use std::fmt;
use std::num::ParseIntError;

#[derive(Debug)]
enum ConfigError {
    Missing(&'static str),
    Invalid(ParseIntError),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Missing(key) => write!(f, "missing key `{key}`"),
            ConfigError::Invalid(_) => write!(f, "invalid number"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Invalid(e) => Some(e),
            ConfigError::Missing(_) => None,
        }
    }
}

// This impl is what lets `?` turn a ParseIntError into a ConfigError.
impl From<ParseIntError> for ConfigError {
    fn from(e: ParseIntError) -> Self {
        ConfigError::Invalid(e)
    }
}

fn get<'a>(cfg: &HashMap<&str, &'a str>, key: &'static str) -> Result<&'a str, ConfigError> {
    cfg.get(key).copied().ok_or(ConfigError::Missing(key))
}

/// What `?` does, written out by hand.
fn parse_port_by_hand(cfg: &HashMap<&str, &str>) -> Result<u16, ConfigError> {
    let raw = match get(cfg, "port") {
        Ok(v) => v,
        Err(e) => return Err(From::from(e)),
    };
    let port = match raw.parse::<u16>() {
        Ok(v) => v,
        Err(e) => return Err(From::from(e)), // uses the From<ParseIntError> impl
    };
    Ok(port)
}

/// The same function with `?`.
fn parse_port(cfg: &HashMap<&str, &str>) -> Result<u16, ConfigError> {
    let port = get(cfg, "port")?.parse::<u16>()?;
    Ok(port)
}

/// `?` also works on Option, inside a function that returns Option.
fn first_even_square(xs: &[u32]) -> Option<u32> {
    let first = xs.iter().find(|x| *x % 2 == 0)?; // None -> return None
    first.checked_mul(*first) // None on overflow
}

fn main() {
    let cases = [
        HashMap::from([("port", "8080")]),
        HashMap::new(),
        HashMap::from([("port", "80x")]),
    ];
    for cfg in &cases {
        let a = parse_port_by_hand(cfg);
        let b = parse_port(cfg);
        let cause = b.as_ref().err().and_then(|e| std::error::Error::source(e)).map(|s| s.to_string());
        println!("{:<34} same={} cause={:?}", format!("{b:?}"), format!("{a:?}") == format!("{b:?}"), cause);
    }
    println!("{:?} {:?} {:?}", first_even_square(&[3, 4, 5]), first_even_square(&[1, 3]), first_even_square(&[70_000]));
}
