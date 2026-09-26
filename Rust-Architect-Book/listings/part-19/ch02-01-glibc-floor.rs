// verify: debug ok
// Which glibc does this binary need, and what happens on a system that doesn't have it?
// We can't install an old glibc here, so we simulate one: rename a version this binary requires (same length,
// so the ELF layout doesn't change) and let the real dynamic loader try to start the copy.
use std::collections::BTreeMap;
use std::process::Command;

fn sh(script: &str) -> String {
    let o = Command::new("sh").args(["-c", script]).output().expect("sh");
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

fn version_key(v: &str) -> Vec<u32> {
    v.trim_start_matches("GLIBC_").split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("quiet") {
        return;
    }
    let exe = std::env::current_exe().unwrap().display().to_string();

    // 1. The versions the binary requires, and which imported symbols pull in the newest ones.
    let mut by_version: BTreeMap<Vec<u32>, (String, Vec<String>)> = BTreeMap::new();
    for line in sh(&format!("objdump -T {exe}")).lines() {
        if let (Some(l), Some(r)) = (line.find("(GLIBC_"), line.find(')')) {
            let ver = line[l + 1..r].to_string();
            let weak = line.contains(" w ");
            let sym = line.split_whitespace().last().unwrap_or("").to_string() + if weak { " (weak)" } else { "" };
            by_version.entry(version_key(&ver)).or_insert((ver, Vec::new())).1.push(sym);
        }
    }
    println!("glibc versions required ({} distinct):", by_version.len());
    for (ver, syms) in by_version.values().rev().take(4) {
        println!("  {ver:<12} {}", syms.join(", "));
    }
    // The loader checks .gnu.version_r ("version needs"), not individual symbols. A need without the WEAK flag is
    // mandatory, even if every symbol that uses it is a weak reference.
    println!("version needs for libc.so.6 (readelf -V), newest three:");
    print!("{}", sh(&format!("readelf -V -W {exe} | awk '/File: libc.so.6/ {{f = 1; next}} /File:/ {{f = 0}} f && /Name: GLIBC_/' | tail -3 | sed 's/^ *0x[0-9a-f]*: *//'")));

    // 2. Simulate older glibcs by renaming a required version in a copy of this binary.
    let bytes = std::fs::read(&exe).unwrap();
    for (from, to) in [("GLIBC_2.39", "GLIBC_2.99"), ("GLIBC_2.34", "GLIBC_2.94")] {
        let mut copy = bytes.clone();
        let needle = from.as_bytes();
        let n = copy.windows(needle.len()).filter(|w| *w == needle).count();
        let pos = copy.windows(needle.len()).position(|w| w == needle).unwrap();
        copy[pos..pos + needle.len()].copy_from_slice(to.as_bytes());
        let path = format!("/tmp/needs-{to}");
        std::fs::write(&path, &copy).unwrap();
        println!("--- copy with {from} renamed to {to} ({n} occurrence(s) of the string) ---");
        print!("{}", sh(&format!("chmod +x {path} && {path} quiet; echo \"exit status $?\"")));
    }
}
