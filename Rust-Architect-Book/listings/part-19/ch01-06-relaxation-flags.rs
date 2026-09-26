// verify: debug ok
// Why rustc's calls into std stay indirect after static linking: the relocation type rustc asks for. Build one program
// three ways and compare the relocations in the object file and the call in the executable. The two -Z flags are
// unstable [VERSION]; the Playground's stable rustc accepts them only with RUSTC_BOOTSTRAP=1 (inspection only).
use std::process::Command;

const SRC: &str = "#[inline(never)] pub fn h(x: u64) -> u64 { x * 3 }\nfn main() { let v = h(std::hint::black_box(14)); println!(\"{v}\"); }\n";

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
    std::fs::write("/tmp/r.rs", SRC).unwrap();
    for (label, flag) in [("default", ""), ("-Z relax-elf-relocations=yes", "-Z relax-elf-relocations=yes"), ("-Z plt=yes", "-Z plt=yes")] {
        must(&format!("cd /tmp && RUSTC_BOOTSTRAP=1 rustc --edition 2024 -C opt-level=1 {flag} --emit=obj -o r.o r.rs \
                       && RUSTC_BOOTSTRAP=1 rustc --edition 2024 -C opt-level=1 {flag} -o r r.rs"));
        println!("=== {label} ===");
        print!("relocations in r.o: {}", sh("readelf -rW /tmp/r.o | awk '/R_X86/ {print $3}' | sort | uniq -c | tr -s ' ' | tr '\\n' ';'; echo"));
        let main_sym = sh("nm /tmp/r | grep '1r4main$' | awk '{print $3}'").trim().to_string();
        print!("calls in r::main:\n{}", sh(&format!("objdump -d --no-show-raw-insn --disassemble={main_sym} /tmp/r | grep call | c++filt")));
    }
    println!("=== the target's defaults (target-spec-json) ===");
    print!("{}", sh("RUSTC_BOOTSTRAP=1 rustc -Z unstable-options --print target-spec-json | grep -E '\"(plt-by-default|relro-level|position-independent-executables|static-position-independent-executables|linker-flavor|default-uwtable|crt-static-respected)\"'"));
}
