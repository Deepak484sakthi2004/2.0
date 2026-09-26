// verify: debug ok
// Debug information as a deployment decision: the same program built with different debuginfo, split-debuginfo and
// strip settings. For each: executable size, extra files, .debug_* sections, and whether a panic backtrace still shows
// file:line. Then the classic split: objcopy --only-keep-debug + --add-gnu-debuglink.
use std::process::Command;

const SRC: &str = r#"
#[inline(never)]
fn settle(cents: i64) -> i64 {
    if cents < 0 { panic!("negative settlement: {cents}"); }
    cents
}
fn main() {
    let n = std::env::args().count() as i64;
    println!("{}", settle(std::hint::black_box(1 - n * 2)));
}
"#;

fn sh(script: &str) -> String {
    let o = Command::new("sh").args(["-c", script]).output().expect("sh");
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

/// Run a build step; the listing fails if the step fails.
fn must(script: &str) {
    let o = Command::new("sh").args(["-c", script]).output().expect("sh");
    assert!(o.status.success(), "step failed: {script}\n{}", String::from_utf8_lossy(&o.stderr));
}

fn main() {
    std::fs::write("/tmp/settle.rs", SRC).unwrap();
    let variants = [
        ("d0", "-C debuginfo=0"),
        ("lines", "-C debuginfo=line-tables-only"),
        ("full", "-C debuginfo=2"),
        ("packed", "-C debuginfo=2 -C split-debuginfo=packed"),
        ("unpacked", "-C debuginfo=2 -C split-debuginfo=unpacked"),
        ("strip-dbg", "-C debuginfo=2 -C strip=debuginfo"),
        ("strip-sym", "-C debuginfo=2 -C strip=symbols"),
    ];
    println!("{:<10} {:>9} {:>6} {:<28} {}", "variant", "exe bytes", ".debug", "other files", "backtrace frame for settle()");
    for (name, flags) in variants {
        let dir = format!("/tmp/dbg-{name}");
        must(&format!("rm -rf {dir} && mkdir -p {dir} && cd {dir} && rustc --edition 2024 -C opt-level=2 {flags} /tmp/settle.rs -o settle"));
        let size = std::fs::metadata(format!("{dir}/settle")).unwrap().len();
        let debug_secs = sh(&format!("readelf -S -W {dir}/settle | grep -c ' .debug_'")).trim().to_string();
        let others = sh(&format!("cd {dir} && ls | grep -v '^settle$' | while read f; do echo \"$f:$(du -b $f | cut -f1)\"; done | tr '\\n' ' '"));
        let frame = sh(&format!("cd {dir} && RUST_BACKTRACE=1 ./settle x 2>&1 | grep -A1 'settle::settle' | tr -s ' ' | tr '\\n' ' '"));
        let frame = if frame.trim().is_empty() { "(no frame named settle)".to_string() } else { frame.trim().to_string() };
        println!("{name:<10} {size:>9} {debug_secs:>6} {:<28} {frame}", if others.trim().is_empty() { "-" } else { others.trim() });
    }
    println!("=== separate debug file with a debuglink (the distro approach) ===");
    must("cd /tmp/dbg-full && objcopy --only-keep-debug settle settle.debug && objcopy --strip-debug --add-gnu-debuglink=settle.debug settle settle.stripped");
    print!("{}", sh("cd /tmp/dbg-full && stat -c '%n %s' settle settle.stripped settle.debug"));
    print!("{}", sh("readelf --string-dump=.gnu_debuglink /tmp/dbg-full/settle.stripped | grep settle"));
    print!("build-ids: {}", sh("for f in settle.stripped settle.debug; do readelf -n /tmp/dbg-full/$f 2>/dev/null | awk '/Build ID/ {print $3}'; done | uniq -c | tr -s ' ' | tr '\\n' ' '; echo"));
    let addr = sh("nm /tmp/dbg-full/settle | grep '6settle6settle$' | head -1 | cut -d' ' -f1");
    let addr = u64::from_str_radix(addr.trim(), 16).unwrap();
    print!("addr2line via the .debug file: {}", sh(&format!("addr2line -e /tmp/dbg-full/settle.debug -f -C {addr:#x} | tr '\\n' ' '; echo")));
}
