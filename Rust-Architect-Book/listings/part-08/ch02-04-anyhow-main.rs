// verify: debug crash Caused by
use anyhow::{Context, Result};

// An application's main: Err is printed with Debug (anyhow's Debug shows the whole chain), exit status 1.
fn main() -> Result<()> {
    let path = "/etc/meridian/limits.conf";
    let text = std::fs::read_to_string(path).with_context(|| format!("loading rate limits from {path}"))?;
    println!("{text}");
    Ok(())
}
