// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target llvm-ir -Mode debug   (count `define`s per module)
use std::path::{Path, PathBuf};

/// A stand-in for real work: parse "key = value" lines, validate, and summarize.
fn parse_body(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (k, v) = line.split_once('=').ok_or_else(|| format!("line {}: expected key = value", n + 1))?;
        out.push((k.trim().to_string(), v.trim().to_string()));
    }
    Ok(out)
}

pub mod fat {
    use super::*;
    /// The whole body is generic over P, so it is duplicated for every P a caller uses.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<usize, String> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let pairs = parse_body(&text)?;
        let mut keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
        keys.sort_unstable();
        keys.dedup();
        if keys.len() != pairs.len() {
            return Err(format!("{}: duplicate keys", path.display()));
        }
        Ok(pairs.len())
    }
}

pub mod thin {
    use super::*;
    /// Only the one-line conversion is generic; the body is compiled exactly once.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<usize, String> {
        fn inner(path: &Path) -> Result<usize, String> {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let pairs = parse_body(&text)?;
            let mut keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
            keys.sort_unstable();
            keys.dedup();
            if keys.len() != pairs.len() {
                return Err(format!("{}: duplicate keys", path.display()));
            }
            Ok(pairs.len())
        }
        inner(path.as_ref())
    }
}

/// Four callers, four path types: &str, String, &Path, PathBuf.
pub fn callers() -> [Result<usize, String>; 8] {
    [
        fat::load("a.conf"),
        fat::load(String::from("b.conf")),
        fat::load(Path::new("c.conf")),
        fat::load(PathBuf::from("d.conf")),
        thin::load("a.conf"),
        thin::load(String::from("b.conf")),
        thin::load(Path::new("c.conf")),
        thin::load(PathBuf::from("d.conf")),
    ]
}
