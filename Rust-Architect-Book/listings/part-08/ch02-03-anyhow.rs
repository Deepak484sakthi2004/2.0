// verify: debug ok
use anyhow::{Context, Result, bail};
use std::io;
use std::path::Path;

fn parse_limit(text: &str) -> Result<u64> {
    let n: u64 = text.trim().parse().context("limit is not a whole number")?;
    if n == 0 {
        bail!("limit must be positive");
    }
    Ok(n)
}

/// Application code: every failure gets context, nobody matches on variants.
fn load_limit(path: &Path) -> Result<u64> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {name}"))?;
    parse_limit(&text).with_context(|| format!("parsing {name}"))
}

fn main() -> Result<()> {
    let dir = tempfile::tempdir()?;
    for (name, body) in [("good.conf", "500\n"), ("bad.conf", "5OO\n"), ("zero.conf", "0\n")] {
        std::fs::write(dir.path().join(name), body)?;
    }
    for name in ["good.conf", "bad.conf", "zero.conf", "missing.conf"] {
        match load_limit(&dir.path().join(name)) {
            Ok(n) => println!("{name}: limit {n}"),
            Err(e) => {
                println!("{name}:");
                println!("  {{}}   {e}");
                println!("  {{:#}}  {e:#}");
                println!("  chain length {}; root cause is io::Error? {}", e.chain().count(), e.root_cause().is::<io::Error>());
                if let Some(io) = e.downcast_ref::<io::Error>() {
                    println!("  downcast_ref::<io::Error>() -> kind {:?}", io.kind());
                }
            }
        }
    }
    let e = load_limit(&dir.path().join("bad.conf")).unwrap_err();
    println!("--- {{:?}} ---\n{e:?}");
    Ok(())
}
