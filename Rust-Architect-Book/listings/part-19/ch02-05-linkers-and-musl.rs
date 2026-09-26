// verify: debug ok
// Which linker does rustc 1.98 use for x86_64-unknown-linux-gnu, what does it write into the binary, how long does
// linking take with rust-lld vs GNU ld, and what building for the musl target needs.
use std::process::Command;
use std::time::Instant;

const SRC: &str = r#"
use std::collections::HashMap;
fn main() {
    let mut m: HashMap<String, u64> = HashMap::new();
    for w in "a b c a b a".split(' ') { *m.entry(w.to_string()).or_default() += 1; }
    let mut v: Vec<_> = m.into_iter().collect();
    v.sort();
    println!("{v:?}");
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
    std::fs::write("/tmp/words.rs", SRC).unwrap();
    // Compile once to an object-free rlib-less build: we time the whole rustc invocation, best of 3.
    let variants = [("default", ""), ("GNU ld (-C linker-features=-lld)", "-C linker-features=-lld")];
    for (label, flag) in variants {
        let mut best = f64::MAX;
        for _ in 0..3 {
            let t = Instant::now();
            must(&format!("cd /tmp && rustc --edition 2024 -C opt-level=1 {flag} words.rs -o words-{}", label.len()));
            best = best.min(t.elapsed().as_secs_f64() * 1000.0);
        }
        let exe = format!("/tmp/words-{}", label.len());
        println!("=== {label}: best of 3 rustc runs {best:.0} ms, {} bytes ===", std::fs::metadata(&exe).unwrap().len());
        print!(".comment: {}", sh(&format!("readelf -p .comment {exe} | grep -oE '(Linker|GCC): .*' | sort -u | tr '\\n' ';'; echo")));
        print!("dynamic FLAGS: {}", sh(&format!("readelf -d {exe} | grep -E 'FLAGS' | awk '{{$1=\"\"; print}}' | tr '\\n' ';'; echo")));
        print!("RELRO segment: {}", sh(&format!("readelf -lW {exe} | grep -c GNU_RELRO")));
        print!("runs: {}", sh(&exe));
    }
    // std's weak references (pidfd_spawnp, pidfd_getpid, used by Command): does either linker mark the version need WEAK?
    std::fs::write("/tmp/spawner.rs", "fn main() { let s = std::process::Command::new(\"/bin/true\").status().unwrap(); println!(\"{s}\"); }\n").unwrap();
    for (label, flag) in variants {
        must(&format!("cd /tmp && rustc --edition 2024 -C opt-level=1 {flag} spawner.rs -o spawner-{}", label.len()));
        print!("{label}: {}", sh(&format!("readelf -V -W /tmp/spawner-{} | grep 'Name: GLIBC_2[.]39' | sed 's/^ *0x[0-9a-f]*: *//'", label.len())));
    }
    println!("=== musl ===");
    print!("musl targets rustc knows: {}", sh("rustc --print target-list | grep -c musl"));
    print!("{}", sh("cd /tmp && rustc --edition 2024 --target x86_64-unknown-linux-musl words.rs -o words-musl 2>&1 | head -4"));
    print!("installed std targets: {}", sh("ls $(rustc --print sysroot)/lib/rustlib | grep -E 'unknown|linux' | tr '\\n' ' '; echo"));
}
