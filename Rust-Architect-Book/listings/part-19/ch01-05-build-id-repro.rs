// verify: debug ok
// Is a rebuild the "same" binary? Build one source file in two different directories (as two CI runners would),
// with and without --remap-path-prefix, and compare the GNU build-id: the key that matches a stripped binary to its
// debug file and a crash report to its symbols.
use std::process::Command;

const SRC: &str = r#"
fn main() {
    let n: u64 = std::env::args().count() as u64;
    if n > 5 { panic!("too many arguments: {n}"); }
    println!("ok {n}");
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

fn build_id(path: &str) -> String {
    sh(&format!("readelf -n {path} | awk '/Build ID/ {{print $3}}'")).trim().to_string()
}

fn main() {
    for dir in ["/tmp/runner-a/svc", "/tmp/runner-b/svc"] {
        must(&format!("mkdir -p {dir}/src"));
        std::fs::write(format!("{dir}/src/main.rs"), SRC).unwrap();
    }
    let variants = [
        ("no debug info", "-C opt-level=2"),
        ("debug info", "-C opt-level=2 -g"),
        ("debug info + remap", "-C opt-level=2 -g --remap-path-prefix=$PWD=/build"),
    ];
    for (label, flags) in variants {
        let mut ids = Vec::new();
        for dir in ["/tmp/runner-a/svc", "/tmp/runner-b/svc"] {
            must(&format!("cd {dir} && rustc --edition 2024 {flags} src/main.rs -o svc"));
            ids.push(build_id(&format!("{dir}/svc")));
        }
        let same = if ids[0] == ids[1] { "SAME" } else { "DIFFERENT" };
        println!("{label:<20} runner-a {}  runner-b {}  -> {same}", &ids[0][..12], &ids[1][..12]);
    }
    println!("--- where the directory hides in the debug build (runner-a, without remap) ---");
    must("cd /tmp/runner-a/svc && rustc --edition 2024 -C opt-level=2 -g src/main.rs -o svc");
    print!("{}", sh("strings -a /tmp/runner-a/svc/svc | grep -c '/tmp/runner-a/svc'"));
    must("cd /tmp/runner-a/svc && rustc --edition 2024 -C opt-level=2 -g --remap-path-prefix=$PWD=/build src/main.rs -o svc");
    print!("{}", sh("strings -a /tmp/runner-a/svc/svc | grep -c '/tmp/runner-a/svc'"));
}
