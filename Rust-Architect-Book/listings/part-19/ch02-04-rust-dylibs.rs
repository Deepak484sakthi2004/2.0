// verify: debug ok
// Two places where Rust itself uses dynamic linking: `-C prefer-dynamic` (std as a shared library), and the
// compiler loading a proc-macro crate with dlopen.
use std::process::Command;

fn sh(script: &str) -> String {
    let o = Command::new("sh").args(["-c", script]).output().expect("sh");
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

const MACRO: &str = r#"
extern crate proc_macro; // plain rustc (no Cargo) doesn't add it to the extern prelude
use proc_macro::TokenStream;
#[proc_macro]
pub fn answer(_input: TokenStream) -> TokenStream { "42u32".parse().unwrap() }
"#;

const USER: &str = r#"
fn main() { println!("answer!() = {}", answer_macro::answer!()); }
"#;

fn main() {
    println!("--- 1. -C prefer-dynamic: std becomes a NEEDED shared library ---");
    std::fs::write("/tmp/hi.rs", "fn main() { println!(\"hi\"); }\n").unwrap();
    must("cd /tmp && rustc --edition 2024 -C opt-level=2 -C strip=symbols hi.rs -o hi-static-std \
          && rustc --edition 2024 -C opt-level=2 -C strip=symbols -C prefer-dynamic hi.rs -o hi-dyn-std");
    print!("{}", sh("cd /tmp && stat -c '%n %s bytes' hi-static-std hi-dyn-std && readelf -d hi-dyn-std | grep NEEDED | head -2"));
    print!("{}", sh("cd /tmp && env -u LD_LIBRARY_PATH ./hi-dyn-std; echo \"exit status $?\""));
    print!("{}", sh("cd /tmp && SYSROOT=$(rustc --print sysroot) && LD_LIBRARY_PATH=$SYSROOT/lib/rustlib/x86_64-unknown-linux-gnu/lib ./hi-dyn-std \
                     && ls $SYSROOT/lib/rustlib/x86_64-unknown-linux-gnu/lib/libstd-*.so | xargs -n1 basename"));

    println!("--- 2. rustc loads a proc-macro crate with dlopen ---");
    std::fs::write("/tmp/answer_macro.rs", MACRO).unwrap();
    std::fs::write("/tmp/user.rs", USER).unwrap();
    must("cd /tmp && rustc --edition 2024 --crate-type=proc-macro answer_macro.rs -o libanswer_macro.so");
    print!("{}", sh("cd /tmp && file -b libanswer_macro.so | cut -d, -f1-2 && nm -D --defined-only libanswer_macro.so | grep -i proc_macro_decls"));
    // LD_DEBUG=files makes the dynamic loader report every object it loads, including dlopen'd ones.
    print!("{}", sh("cd /tmp && LD_DEBUG=files rustc --edition 2024 user.rs --extern answer_macro=libanswer_macro.so -o user 2>&1 \
                     | grep -E 'libanswer_macro' | grep -E 'dynamically loaded|calling init' | sed 's/^ *[0-9]*: *//' | head -3"));
    let out = sh("/tmp/user");
    print!("{out}");
    assert_eq!(out.trim(), "answer!() = 42", "the program using the proc macro must build and run");
}

/// Run a build step; the listing fails if the step fails.
fn must(script: &str) {
    let o = Command::new("sh").args(["-c", script]).output().expect("sh");
    assert!(o.status.success(), "step failed: {script}\n{}", String::from_utf8_lossy(&o.stderr));
}
