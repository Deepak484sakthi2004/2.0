// verify: debug ok
// Shared libraries end to end: a C library, a Rust program linked against it, the loader's search, rpath,
// what a Rust cdylib exports, and loading a library at run time with dlopen.
use std::process::Command;

fn sh(script: &str) -> String {
    let o = Command::new("sh").args(["-c", script]).output().expect("sh");
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

const USE_FEE: &str = r#"
unsafe extern "C" { fn fee_bps(amount_cents: i64) -> i64; }
fn main() { println!("fee = {}", unsafe { fee_bps(10_000) }); }
"#;

const CDYLIB: &str = r#"
#[unsafe(no_mangle)]
pub extern "C" fn risk_score(amount_cents: i64) -> i32 { internal_weight(amount_cents) }
#[inline(never)]
pub fn internal_weight(x: i64) -> i32 { (x / 1000) as i32 }   // pub, but not exported from the cdylib
"#;

fn main() {
    std::fs::create_dir_all("/tmp/app/lib").unwrap();
    std::fs::write("/tmp/fee.c", "long fee_bps(long cents) { return cents * 29 / 10000; }\n").unwrap();
    std::fs::write("/tmp/use_fee.rs", USE_FEE).unwrap();
    must("cd /tmp && gcc -shared -fPIC -O1 fee.c -o app/lib/libfee.so");

    println!("--- 1. linked against libfee.so, no rpath ---");
    must("cd /tmp && rustc --edition 2024 use_fee.rs -L app/lib -l fee -o app/use_fee");
    print!("{}", sh("readelf -d /tmp/app/use_fee | grep -E 'NEEDED|RUNPATH'"));
    print!("{}", sh("cd /tmp && env -u LD_LIBRARY_PATH ./app/use_fee; echo \"exit status $?\""));

    println!("--- 2. same, with RUNPATH=$ORIGIN/lib (resolved relative to the executable) ---");
    must("cd /tmp && rustc --edition 2024 use_fee.rs -L app/lib -l fee -C link-arg=-Wl,-rpath,'$ORIGIN/lib' -o app/use_fee");
    print!("{}", sh("readelf -d /tmp/app/use_fee | grep -E 'RUNPATH'"));
    print!("{}", sh("cd / && env -u LD_LIBRARY_PATH /tmp/app/use_fee"));
    println!("--- the loader's search, as it reports it (LD_DEBUG=libs) ---");
    print!("{}", sh("cd / && env -u LD_LIBRARY_PATH LD_DEBUG=libs /tmp/app/use_fee 2>&1 | grep -E 'find library|search path|trying file' | sed 's/^ *[0-9]*: *//' | head -8"));

    println!("--- 3. a Rust cdylib: what it exports ---");
    std::fs::write("/tmp/risk.rs", CDYLIB).unwrap();
    must("cd /tmp && rustc --edition 2024 -C opt-level=2 --crate-type=cdylib risk.rs -o librisk.so");
    print!("{}", sh("stat -c '%s bytes' /tmp/librisk.so"));
    print!("{}", sh("cd /tmp && echo \"defined dynamic symbols: $(nm -D --defined-only librisk.so | wc -l)\"; nm -D --defined-only librisk.so"));
    print!("{}", sh("cd /tmp && echo \"internal_weight in .symtab: $(nm librisk.so | grep -c internal_weight)\""));

    println!("--- 4. dlopen + dlsym at run time (how plugins and proc macros are loaded) ---");
    // SAFETY: dlopen/dlsym with NUL-terminated strings; the symbol's type matches its definition in risk.rs.
    unsafe {
        let handle = libc::dlopen(c"/tmp/librisk.so".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        assert!(!handle.is_null());
        let sym = libc::dlsym(handle, c"risk_score".as_ptr());
        let f: extern "C" fn(i64) -> i32 = std::mem::transmute(sym);
        println!("risk_score(250_000) = {}", f(250_000));
        let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
        println!("librisk.so mappings in this process: {}", maps.lines().filter(|l| l.contains("librisk")).count());
        libc::dlclose(handle);
    }
}

/// Run a build step; the listing fails if the step fails.
fn must(script: &str) {
    let o = Command::new("sh").args(["-c", script]).output().expect("sh");
    assert!(o.status.success(), "step failed: {script}\n{}", String::from_utf8_lossy(&o.stderr));
}
