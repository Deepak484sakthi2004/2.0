// verify: debug ok
// Build the same program twice with the Playground's rustc: dynamically linked (the default) and statically
// linked against glibc (`-C target-feature=+crt-static`). Compare size, dependencies, startup, and LD_PRELOAD.
use std::process::Command;
use std::time::Instant;

const HELLO: &str = r#"
fn main() {
    if std::env::args().nth(1).as_deref() == Some("dns") {
        use std::net::ToSocketAddrs;
        println!("{:?}", "localhost:80".to_socket_addrs().map(|a| a.count()));
        return;
    }
    println!("pid via std::process::id() = {}", std::process::id());
}
"#;

fn sh(script: &str) -> String {
    let o = Command::new("sh").args(["-c", script]).output().expect("sh");
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

fn main() {
    std::fs::write("/tmp/hello.rs", HELLO).unwrap();
    // Opt-level and debug info are the same for both; only the linking mode differs.
    must("cd /tmp && rustc --edition 2024 -C opt-level=2 -C strip=symbols hello.rs -o hello-dyn \
          && rustc --edition 2024 -C opt-level=2 -C strip=symbols -C target-feature=+crt-static hello.rs -o hello-static");
    for f in ["hello-dyn", "hello-static"] {
        println!("--- {f} ---");
        print!("{}", sh(&format!("cd /tmp && stat -c '%s bytes' {f} && file -b {f} | cut -d, -f1-2,4 && ldd {f} 2>&1 | head -3")));
    }

    // Startup cost: spawn each 200 times (one run on a shared machine: noisy), first with the environment this
    // program inherited from cargo (a long LD_LIBRARY_PATH), then with LD_LIBRARY_PATH removed.
    for clean in [false, true] {
        for f in ["hello-dyn", "hello-static"] {
            let path = format!("/tmp/{f}");
            let t = Instant::now();
            for _ in 0..200 {
                let mut cmd = Command::new(&path);
                if clean {
                    cmd.env_remove("LD_LIBRARY_PATH");
                }
                cmd.output().unwrap();
            }
            let label = if clean { "without LD_LIBRARY_PATH" } else { "cargo's environment   " };
            println!("{f:<12} {label}: {:.0} µs per spawn+run+wait (200 runs)", t.elapsed().as_secs_f64() * 1e6 / 200.0);
        }
    }

    // Symbol interposition: a preloaded library that defines getpid() wins over libc's, for dynamic binaries only.
    std::fs::write("/tmp/fake.c", "int getpid(void) { return 4242; }\n").unwrap();
    must("cd /tmp && gcc -shared -fPIC -O1 fake.c -o libfake.so");
    for f in ["hello-dyn", "hello-static"] {
        print!("LD_PRELOAD=libfake.so {f}: {}", sh(&format!("cd /tmp && LD_PRELOAD=/tmp/libfake.so ./{f}")));
    }
    // Name resolution in a static glibc binary still wants glibc's shared NSS modules at run time.
    for f in ["hello-dyn", "hello-static"] {
        print!("{f} dns: {}", sh(&format!("cd /tmp && ./{f} dns")));
    }
}

/// Run a build step; the listing fails if the step fails.
fn must(script: &str) {
    let o = Command::new("sh").args(["-c", script]).output().expect("sh");
    assert!(o.status.success(), "step failed: {script}\n{}", String::from_utf8_lossy(&o.stderr));
}
