// verify: debug ok
// Compile a tiny program to an OBJECT FILE with the rustc inside the Playground container, show its unresolved
// relocations, then link it and show what the linker wrote into the same call sites.
use std::process::Command;

const SRC: &str = r#"
#[inline(never)]
pub fn local_helper(x: u64) -> u64 { x.wrapping_mul(3) }      // defined in this crate
fn main() {
    let n = std::hint::black_box(14u64);
    let v = local_helper(n);                                    // call into this crate
    let p = unsafe { getpid() };                                // call into libc (a shared library)
    println!("{v} {}", p > 0);                                  // calls into std (another crate)
}
unsafe extern "C" { fn getpid() -> i32; }
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
    std::fs::write("/tmp/relax.rs", SRC).unwrap();
    // --emit=obj,link keeps the object file next to the linked executable
    must("cd /tmp && rustc --edition 2024 -C opt-level=1 --emit=obj,link relax.rs -o relax");
    println!("{}", sh("cd /tmp && ./relax && ls -l relax.o relax | awk '{print $5, $9}'"));

    println!("--- sections of relax.o (name, size) ---");
    for line in sh("readelf -S -W /tmp/relax.o").lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if let Some(i) = f.iter().position(|w| w.starts_with('.')) {
            if f.len() > i + 4 && line.contains(']') {
                let name = f[i];
                let size = u64::from_str_radix(f[i + 4], 16).unwrap_or(0);
                if size > 0 {
                    println!("{name:<60} {size:>6}");
                }
            }
        }
    }

    println!("--- symbols of relax.o (nm) ---");
    print!("{}", sh("nm /tmp/relax.o | grep -E 'relax|getpid|_print|main$'"));

    let sym = sh("nm /tmp/relax.o | grep '5relax4main$' | awk '{print $NF}'").trim().to_string();
    println!("--- main in relax.o (before linking) ---");
    print!("{}", sh(&format!("objdump -dr --disassemble={sym} /tmp/relax.o | sed -n '/<_R/,/^$/p'")));
    println!("--- main in relax (after linking) ---");
    print!("{}", sh(&format!("objdump -d --disassemble={sym} /tmp/relax | sed -n '/<_R/,/^$/p'")));

    println!("--- where the GOT slots point after dynamic linking (relocations left for the loader) ---");
    let print_addr = sh("nm /tmp/relax | grep 6__print$ | cut -d' ' -f1").trim().trim_start_matches('0').to_string();
    print!("{}", sh(&format!("readelf -r -W /tmp/relax | grep -E 'getpid|RELATIVE +{print_addr}$'")));
    println!("--- relocation types left in the executable ---");
    print!("{}", sh("readelf -r -W /tmp/relax | awk '/R_X86_64/ {print $3}' | sort | uniq -c"));
}
