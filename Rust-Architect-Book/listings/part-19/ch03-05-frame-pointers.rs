// verify: debug ok
// Frame pointers vs unwind tables. Release builds on x86-64 Linux omit the frame pointer by default and rely on
// .eh_frame for unwinding; -C force-frame-pointers=yes keeps rbp as a linked list of frames that sampling profilers
// can walk cheaply. Compare the prologues and the code size.
use std::process::Command;

const SRC: &str = r#"
#[inline(never)]
pub fn checksum(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data { h = (h ^ b as u64).wrapping_mul(0x100000001b3); }
    h
}
#[inline(never)]
pub fn frame_hash(frames: &[Vec<u8>]) -> u64 {
    frames.iter().map(|f| checksum(f)).fold(0, |a, x| a ^ x)
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
    std::fs::write("/tmp/fp.rs", SRC).unwrap();
    for (name, flag) in [("default", ""), ("forced", "-C force-frame-pointers=yes")] {
        must(&format!("cd /tmp && rustc --edition 2024 --crate-type=lib --emit=obj -C opt-level=3 {flag} fp.rs -o fp-{name}.o"));
        println!("=== {name} {flag} ===");
        let sym = sh(&format!("nm /tmp/fp-{name}.o | grep '10frame_hash$' | awk '{{print $3}}'")).trim().to_string();
        print!("{}", sh(&format!(
            "objdump -d --no-addresses --no-show-raw-insn --disassemble={sym} /tmp/fp-{name}.o | sed -n '/>:$/,/^$/p' | head -8 | c++filt"
        )));
        print!(".text bytes: {}", sh(&format!("size -A /tmp/fp-{name}.o | awk '/^\\.text/ {{s += $2}} END {{print s}}'")));
        print!(".eh_frame bytes: {}", sh(&format!("size -A /tmp/fp-{name}.o | awk '$1 == \".eh_frame\" {{print $2}}'")));
    }
}
