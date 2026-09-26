// verify: debug ok
// What the symbol table holds, what `strip` removes, and what that does to a panic backtrace.
use std::process::Command;

#[inline(never)]
fn validate_amount(cents: i64) -> i64 {
    if cents < 0 {
        panic!("negative amount: {cents}");
    }
    cents
}

fn sh(script: &str) -> String {
    let o = Command::new("sh").args(["-c", script]).output().expect("sh");
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("panic") {
        validate_amount(std::hint::black_box(-5));
        return;
    }
    let exe = std::env::current_exe().unwrap().display().to_string();
    println!("--- symbol kinds in this binary (nm; T/t = code, D/d/B/b/R/r = data, U = undefined, w = weak) ---");
    print!("{}", sh(&format!("nm {exe} | awk '{{print $(NF-1)}}' | sort | uniq -c | sort -rn | tr '\\n' ' '; echo")));
    println!("--- a few symbols, raw (v0 mangling) and demangled (nm -C) ---");
    print!("{}", sh(&format!("nm {exe} | grep -E 'validate_amount|rust_eh_personality|__rust_alloc$|no_alloc_shim|10playground4main$| main$| _start$'")));
    print!("{}", sh(&format!("nm -C {exe} | grep -E 'validate_amount|__rust_alloc$|playground::main$| main$'")));
    println!("--- dynamic symbols: {} imports (objdump -T) ---", sh(&format!("objdump -T {exe} | grep -c UND")).trim());

    // Three copies: as built, without debug info, without anything.
    must(&format!("cd /tmp && cp {exe} full && cp {exe} nodebug && cp {exe} bare && strip --strip-debug nodebug && strip --strip-all bare"));
    print!("{}", sh(
        "cd /tmp && for f in full nodebug bare; do echo \"$f $(stat -c %s $f) bytes: $(file -b $f | grep -o 'with debug_info\\|not stripped\\|stripped' | tr '\\n' ' ')\"; done"
    ));
    for f in ["full", "nodebug", "bare"] {
        println!("--- panic backtrace, /tmp/{f} ---");
        let out = sh(&format!("cd /tmp && RUST_BACKTRACE=1 ./{f} panic 2>&1 | grep -v -E '^note|^thread' | head -9"));
        print!("{out}");
    }
    println!("--- /tmp/bare with RUST_BACKTRACE=full: addresses only ---");
    print!("{}", sh("cd /tmp && RUST_BACKTRACE=full ./bare panic 2>&1 | grep -E '^ +[0-9]+:' | head -3"));
    println!("--- offline symbolization: addr2line against the unstripped copy (same build-id) ---");
    let addr = sh("nm /tmp/full | grep 15validate_amount$ | cut -d' ' -f1");
    let addr = u64::from_str_radix(addr.trim(), 16).unwrap() + 0x20;
    print!("{}", sh(&format!("addr2line -f -C -e /tmp/full {addr:#x}")));
    println!("--- build-id: the key a symbol server uses to find the matching debug file ---");
    print!("{}", sh(&format!("readelf -n {exe} | grep 'Build ID'; readelf -n /tmp/bare | grep 'Build ID'")));
}

/// Run a build step; the listing fails if the step fails.
fn must(script: &str) {
    let o = Command::new("sh").args(["-c", script]).output().expect("sh");
    assert!(o.status.success(), "step failed: {script}\n{}", String::from_utf8_lossy(&o.stderr));
}
