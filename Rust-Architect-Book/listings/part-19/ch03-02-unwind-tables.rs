// verify: debug ok
// Unwind tables in the object file. One function holds a Drop guard across a call. Build it three ways and compare
// the sections, the machine code, and the call-frame information (CIE/FDE):
//   A: the callee is extern "C"        (can't unwind since Rust 1.81) with panic=unwind
//   B: the callee is extern "C-unwind" (may unwind)                   with panic=unwind
//   C: the callee is extern "C-unwind"                                with panic=abort
use std::process::Command;

const SRC: &str = r#"
pub struct Guard(pub u32);
impl Drop for Guard {
    fn drop(&mut self) { unsafe { release(self.0) } }
}
unsafe extern "C" { fn release(id: u32); }
unsafe extern "ABI" { fn may_fail(x: u32) -> u32; }

#[unsafe(no_mangle)]
pub fn with_guard(x: u32) -> u32 {
    let _g = Guard(x);                 // dropped on the normal path, and during unwinding if may_fail unwinds
    (unsafe { may_fail(x) }) + 1
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
    let builds = [("A", "C", "unwind"), ("B", "C-unwind", "unwind"), ("C", "C-unwind", "abort")];
    for (name, abi, strategy) in builds {
        std::fs::write(format!("/tmp/guard{name}.rs"), SRC.replace("\"ABI\"", &format!("\"{abi}\""))).unwrap();
        must(&format!(
            "cd /tmp && rustc --edition 2024 --crate-type=lib --emit=obj -C opt-level=2 -C panic={strategy} guard{name}.rs -o guard{name}.o"
        ));
        println!("=== {name}: callee extern \"{abi}\", panic={strategy} ===");
        print!("sections: {}", sh(&format!(
            "readelf -S -W /tmp/guard{name}.o | grep -oE '\\.(gcc_except_table[^ ]*|eh_frame)( |$)' | sort -u | tr -d ' ' | tr '\\n' ' '; echo"
        )));
        print!("{}", sh(&format!(
            "objdump -dr --no-addresses --no-show-raw-insn --disassemble=with_guard /tmp/guard{name}.o | sed -n '/<with_guard>/,/^$/p' | c++filt"
        )));
    }
    println!("=== B: CIE augmentation and FDE for with_guard ===");
    print!("{}", sh("readelf --debug-dump=frames /tmp/guardB.o 2>/dev/null | grep -E 'CIE|Augmentation|Personality|FDE|LSDA' | head -10"));
    println!("=== B: relocations in .eh_frame (what each CIE/FDE points at) ===");
    print!("{}", sh("readelf -r -W /tmp/guardB.o | sed -n '/rela.eh_frame/,/^$/p' | grep R_X86 | awk '{print $3, $5}' | c++filt"));
    println!("=== B: the LSDA (.gcc_except_table) is a few bytes of call-site records ===");
    print!("{}", sh("readelf -x .gcc_except_table.with_guard /tmp/guardB.o 2>/dev/null | tail -n +3"));
}
