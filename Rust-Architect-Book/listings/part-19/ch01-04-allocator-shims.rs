// verify: debug ok
// Every Rust allocation calls `__rust_alloc`, a symbol rustc generates once, in the final binary. Disassemble the shim
// in this program (default allocator) and in a program with a #[global_allocator], and list the related symbols.
use std::process::Command;

const CUSTOM: &str = r#"
use std::alloc::{GlobalAlloc, Layout, System};
struct Forwarding;
// SAFETY: every method forwards to System with the caller's layout, so System's guarantees carry over unchanged.
unsafe impl GlobalAlloc for Forwarding {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 { unsafe { System.alloc(l) } }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) { unsafe { System.dealloc(p, l) } }
}
#[global_allocator]
static A: Forwarding = Forwarding;
fn main() { let v = vec![1u8; 100]; println!("{}", v.len()); }
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

/// Disassemble the `__rust_alloc` shim of an executable (found by its v0-mangled name).
fn shim(exe: &str) -> String {
    let sym = sh(&format!("nm {exe} | grep -E '12___rust_alloc$' | awk '{{print $3}}'")).trim().to_string();
    sh(&format!("objdump -d --no-show-raw-insn --disassemble={sym} {exe} | c++filt | sed -n '/>:$/,/^$/p'"))
}

fn main() {
    let v = vec![7u8; 100]; // one allocation through the shim
    assert_eq!(v.len(), 100);
    let exe = std::env::current_exe().unwrap().display().to_string();
    println!("--- allocator symbols in this binary (nm -C) ---");
    print!("{}", sh(&format!("nm -C {exe} | grep -E '__rust_(alloc|dealloc|realloc|alloc_zeroed|no_alloc_shim_is_unstable_v2)$|__rdl_alloc$|__rg_alloc$' | sort -k3")));
    println!("--- __rust_alloc, default allocator ---");
    print!("{}", shim(&exe));

    std::fs::write("/tmp/custom_alloc.rs", CUSTOM).unwrap();
    must("cd /tmp && rustc --edition 2024 custom_alloc.rs -o custom_alloc");
    println!("--- __rust_alloc, with #[global_allocator] ---");
    print!("{}", shim("/tmp/custom_alloc"));
    print!("{}", sh("nm -C /tmp/custom_alloc | grep -E '__rg_alloc$|__rdl_alloc$' | sort -k3"));
    print!("custom_alloc prints: {}", sh("/tmp/custom_alloc"));
}
