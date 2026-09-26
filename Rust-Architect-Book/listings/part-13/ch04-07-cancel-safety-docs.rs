// verify: debug ok
//! Tokio documents cancel safety per method. This program prints those "# Cancel safety" sections from
//! tokio 1.53.1's own source (the Playground keeps registry sources next to the build).
use std::fs;
fn section(root: &str, file: &str, fn_marker: &str) {
    let s = match fs::read_to_string(format!("{root}/{file}")) { Ok(s) => s, Err(e) => { println!("== {file}: {e}"); return; } };
    let lines: Vec<&str> = s.lines().collect();
    // find the fn, then walk back to the nearest "# Cancel safety" heading in its doc comment
    let Some(fn_at) = lines.iter().position(|l| l.contains(fn_marker)) else { println!("== {file}: no {fn_marker}"); return; };
    let start = (0..fn_at).rev().take(120).find(|&i| lines[i].contains("# Cancel safety"));
    match start {
        Some(i) => {
            println!("== {file} [{fn_marker}]");
            for l in lines.iter().skip(i + 1).take(8) {
                let t = l.trim().trim_start_matches("///").trim();
                if t.starts_with('#') || t.starts_with("```") { break; }
                if !t.is_empty() { println!("   {t}"); }
            }
        }
        None => println!("== {file} [{fn_marker}]: no cancel-safety section"),
    }
}
fn main() {
    let r = "/playground/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.53.1/src";
    section(r, "sync/mpsc/bounded.rs", "pub async fn recv(&mut self)");
    section(r, "sync/mpsc/bounded.rs", "pub async fn send(&self, value: T)");
    section(r, "sync/mpsc/bounded.rs", "pub async fn reserve(&self)");
    section(r, "io/util/async_read_ext.rs", "fn read_exact<'a>");
    section(r, "io/util/async_read_ext.rs", "fn read<'a>(&'a mut self, buf: &'a mut [u8])");
    section(r, "io/util/async_write_ext.rs", "fn write_all<'a>");
    section(r, "sync/mutex.rs", "pub async fn lock(&self)");
    section(r, "sync/broadcast.rs", "pub async fn recv(&mut self)");
    section(r, "io/util/async_buf_read_ext.rs", "fn read_line<'a>");
}
