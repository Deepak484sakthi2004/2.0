// verify: debug ok
// Part XIX review capstone: a release-artifact audit. Build the "PR" version of a small service with the flags the PR
// proposes, build the fixed version, and check both binaries the way a release gate would: PIE, full RELRO, NX stack,
// RUNPATH, NEEDED libraries, glibc floor, CPU baseline, build-id, and how debug info ships.
use std::process::Command;

const PR_SRC: &str = r#"
unsafe extern "C" { fn pidfd_open(pid: i32, flags: u32) -> i32; } // glibc wrapper, new in glibc 2.36
#[inline(never)]
fn score(amounts: &[u32]) -> u32 { amounts.iter().map(|a| a.wrapping_mul(3) ^ 0x55).fold(0, u32::wrapping_add) }
fn main() {
    let v: Vec<u32> = (0..4096).collect();
    let fd = unsafe { pidfd_open(std::process::id() as i32, 0) };
    println!("score {} pidfd {}", score(std::hint::black_box(&v)), fd >= 0);
}
"#;

const FIXED_SRC: &str = r#"
unsafe extern "C" { fn syscall(n: i64, ...) -> i64; } // the raw system call: no new glibc symbol needed
const SYS_PIDFD_OPEN: i64 = 434;
#[inline(never)]
fn score(amounts: &[u32]) -> u32 { amounts.iter().map(|a| a.wrapping_mul(3) ^ 0x55).fold(0, u32::wrapping_add) }
fn main() {
    let v: Vec<u32> = (0..4096).collect();
    let fd = unsafe { syscall(SYS_PIDFD_OPEN, std::process::id() as i64, 0i64) };
    println!("score {} pidfd {}", score(std::hint::black_box(&v)), fd >= 0);
}
"#;

const PR_FLAGS: &str = "-C opt-level=3 -C target-cpu=native -g -C relocation-model=static \
    -C link-arg=-Wl,-z,execstack -C link-arg=-Wl,-z,lazy \
    -C link-arg=-Wl,-rpath,/home/ci/runner/_work/payments-core/target/release/deps";
const FIXED_FLAGS: &str = "-C opt-level=3 -g";
const TARGET_GLIBC: (u32, u32) = (2, 35); // the hosts this service will run on

fn sh(script: &str) -> String {
    let o = Command::new("sh").args(["-c", script]).output().expect("sh");
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

/// Run a build step; the listing fails if the step fails.
fn must(script: &str) {
    let o = Command::new("sh").args(["-c", script]).output().expect("sh");
    assert!(o.status.success(), "step failed: {script}\n{}", String::from_utf8_lossy(&o.stderr));
}

struct Check {
    name: &'static str,
    ok: bool,
    detail: String,
}

fn audit(exe: &str) -> Vec<Check> {
    let header = sh(&format!("readelf -hW {exe}"));
    let segments = sh(&format!("readelf -lW {exe}"));
    let dynamic = sh(&format!("readelf -dW {exe}"));
    let sections = sh(&format!("readelf -SW {exe}"));
    let mut out = Vec::new();

    let pie = header.contains("DYN (Position-Independent Executable file)");
    let kind = header.lines().find(|l| l.contains("Type:")).unwrap_or("").split(':').nth(1).unwrap_or("").trim().to_string();
    out.push(Check { name: "PIE (ASLR for the executable)", ok: pie, detail: kind });

    let relro = segments.contains("GNU_RELRO");
    let now = dynamic.contains("BIND_NOW") || dynamic.lines().any(|l| l.contains("FLAGS_1") && l.contains("NOW"));
    out.push(Check { name: "full RELRO (GNU_RELRO + BIND_NOW)", ok: relro && now, detail: format!("RELRO {relro}, BIND_NOW {now}") });

    let stack = segments.lines().find(|l| l.contains("GNU_STACK")).map(|l| l.split_whitespace().nth(6).unwrap_or("?").to_string()).unwrap_or("none".into());
    out.push(Check { name: "non-executable stack", ok: stack == "RW", detail: format!("GNU_STACK {stack}") });

    let runpath: Vec<String> = dynamic.lines().filter(|l| l.contains("RPATH") || l.contains("RUNPATH"))
        .map(|l| l.split('[').nth(1).unwrap_or("").trim_end_matches(']').to_string()).collect();
    let runpath_ok = runpath.iter().all(|p| p.split(':').all(|d| d.starts_with("$ORIGIN")));
    out.push(Check { name: "no absolute RPATH/RUNPATH", ok: runpath_ok, detail: if runpath.is_empty() { "none".into() } else { runpath.join(",") } });

    let allowed = ["libc.so.6", "libgcc_s.so.1", "ld-linux-x86-64.so.2", "libm.so.6"];
    let needed: Vec<String> = dynamic.lines().filter(|l| l.contains("(NEEDED)"))
        .map(|l| l.split('[').nth(1).unwrap_or("").trim_end_matches(']').to_string()).collect();
    out.push(Check { name: "NEEDED within allowlist", ok: needed.iter().all(|n| allowed.contains(&n.as_str())), detail: needed.join(" ") });

    // The glibc floor is the newest *mandatory* version need (.gnu.version_r entries without the WEAK flag).
    let mut floor = (0u32, 0u32);
    for line in sh(&format!("readelf -V -W {exe}")).lines().filter(|l| l.contains("Name: GLIBC_") && !l.contains("WEAK")) {
        let v = line.split("GLIBC_").nth(1).unwrap().split_whitespace().next().unwrap();
        let mut it = v.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
        let ver = (it.next().unwrap_or(0), it.next().unwrap_or(0));
        floor = floor.max(ver);
    }
    let floor_name = format!("GLIBC_{}.{}", floor.0, floor.1);
    let users = sh(&format!("objdump -T {exe} | grep -F '({floor_name})' | awk '{{print $NF}}' | head -2 | tr '\\n' ' '"));
    out.push(Check { name: "glibc floor <= 2.35 (version needs)", ok: floor <= TARGET_GLIBC, detail: format!("{floor_name} ({})", users.trim()) });

    let wide = sh(&format!("objdump -d --no-show-raw-insn {exe} | grep -cE '%[yz]mm'")).trim().parse::<u32>().unwrap_or(0);
    out.push(Check { name: "x86-64 baseline (no AVX registers)", ok: wide == 0, detail: format!("{wide} instructions use ymm/zmm") });

    let build_id = sh(&format!("readelf -n {exe} | awk '/Build ID/ {{print $3}}'")).trim().to_string();
    out.push(Check { name: "build-id present", ok: !build_id.is_empty(), detail: build_id.chars().take(12).collect() });

    let debug_in_binary = sections.contains(".debug_info");
    let debuglink = sections.contains(".gnu_debuglink");
    let size = std::fs::metadata(exe).unwrap().len();
    out.push(Check {
        name: "debug info shipped separately",
        ok: !debug_in_binary && debuglink,
        detail: format!("{size} bytes, .debug_info {debug_in_binary}, .gnu_debuglink {debuglink}"),
    });
    out
}

fn main() {
    std::fs::create_dir_all("/tmp/release").unwrap();
    std::fs::write("/tmp/release/pr.rs", PR_SRC).unwrap();
    std::fs::write("/tmp/release/fixed.rs", FIXED_SRC).unwrap();
    must(&format!("cd /tmp/release && rustc --edition 2024 {PR_FLAGS} pr.rs -o payments-core-pr"));
    must(&format!("cd /tmp/release && rustc --edition 2024 {FIXED_FLAGS} fixed.rs -o payments-core-full \
                   && objcopy --only-keep-debug payments-core-full payments-core.debug \
                   && objcopy --strip-debug --add-gnu-debuglink=payments-core.debug payments-core-full payments-core"));
    print!("PR binary runs:    {}", sh("/tmp/release/payments-core-pr"));
    print!("fixed binary runs: {}", sh("/tmp/release/payments-core"));

    let pr = audit("/tmp/release/payments-core-pr");
    let fixed = audit("/tmp/release/payments-core");
    println!("{:<38} {:<6} {:<48} {:<6} {}", "check", "PR", "", "fixed", "");
    for (a, b) in pr.iter().zip(&fixed) {
        let mark = |ok: bool| if ok { "pass" } else { "FAIL" };
        println!("{:<38} {:<6} {:<48} {:<6} {}", a.name, mark(a.ok), a.detail, mark(b.ok), b.detail);
    }
    println!("PR fails {} of {} checks; fixed fails {}", pr.iter().filter(|c| !c.ok).count(), pr.len(), fixed.iter().filter(|c| !c.ok).count());
    assert!(fixed.iter().all(|c| c.ok), "the fixed build must pass every check");
}
