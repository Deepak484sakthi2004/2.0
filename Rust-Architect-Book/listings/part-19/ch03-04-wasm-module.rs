// verify: debug ok
// A different executable format: build a tiny no_std library for wasm32-unknown-unknown with the Playground's rustc
// and read the module by hand (magic, version, sections, exports). Compare with ELF: no program headers, no
// interpreter, no relocations to apply, no system calls.
use std::process::Command;

const SRC: &str = r#"
#![no_std]
#[unsafe(no_mangle)]
pub extern "C" fn fee_bps(amount_cents: i64) -> i64 { amount_cents * 29 / 10_000 }
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
"#;

/// Run a build step; the listing fails if the step fails.
fn must(script: &str) {
    let o = Command::new("sh").args(["-c", script]).output().expect("sh");
    assert!(o.status.success(), "step failed: {script}\n{}", String::from_utf8_lossy(&o.stderr));
}

/// Unsigned LEB128, the variable-length integer encoding wasm uses everywhere.
fn uleb(b: &[u8], pos: &mut usize) -> u64 {
    let (mut result, mut shift) = (0u64, 0);
    loop {
        let byte = b[*pos];
        *pos += 1;
        result |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return result;
        }
        shift += 7;
    }
}

fn section_name(id: u8) -> &'static str {
    match id {
        0 => "custom", 1 => "type", 2 => "import", 3 => "function", 4 => "table", 5 => "memory", 6 => "global",
        7 => "export", 8 => "start", 9 => "element", 10 => "code", 11 => "data", 12 => "datacount", _ => "?",
    }
}

fn main() {
    std::fs::write("/tmp/fee.rs", SRC).unwrap();
    must("cd /tmp && rustc --edition 2024 --target wasm32-unknown-unknown --crate-type=cdylib -C opt-level=2 -C strip=debuginfo fee.rs -o fee.wasm");
    let b = std::fs::read("/tmp/fee.wasm").unwrap();
    println!("fee.wasm: {} bytes, magic {:02x?} ({:?}), version {}", b.len(), &b[0..4], std::str::from_utf8(&b[1..4]).unwrap(),
        u32::from_le_bytes(b[4..8].try_into().unwrap()));
    let mut pos = 8;
    while pos < b.len() {
        let id = b[pos];
        pos += 1;
        let size = uleb(&b, &mut pos) as usize;
        let body = &b[pos..pos + size];
        let mut detail = String::new();
        if id == 0 {
            let mut p = 0;
            let n = uleb(body, &mut p) as usize;
            detail = format!("name {:?}", std::str::from_utf8(&body[p..p + n]).unwrap());
        } else if id == 7 {
            let mut p = 0;
            let count = uleb(body, &mut p);
            let mut names = Vec::new();
            for _ in 0..count {
                let n = uleb(body, &mut p) as usize;
                let name = std::str::from_utf8(&body[p..p + n]).unwrap().to_string();
                p += n;
                let kind = ["func", "table", "memory", "global"][body[p] as usize];
                p += 1;
                let index = uleb(body, &mut p);
                names.push(format!("{name} ({kind} {index})"));
            }
            detail = names.join(", ");
        }
        println!("  section {id:>2} {:<9} {size:>5} bytes  {detail}", section_name(id));
        pos += size;
    }
}
