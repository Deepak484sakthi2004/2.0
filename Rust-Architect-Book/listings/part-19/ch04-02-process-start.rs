// verify: debug ok
// From execve to your `main`: what the kernel hands the process (the auxiliary vector), which mapping each address
// points into, the chain of frames between the ELF entry point and user code, and the C `main` rustc generates.
use std::process::Command;

fn sh(script: &str) -> String {
    let o = Command::new("sh").args(["-c", script]).output().expect("sh");
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

/// The /proc/self/maps line (perms + name) that contains `addr`.
fn mapping_of(addr: u64) -> String {
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
    for line in maps.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let (lo, hi) = f[0].split_once('-').unwrap();
        let (lo, hi) = (u64::from_str_radix(lo, 16).unwrap(), u64::from_str_radix(hi, 16).unwrap());
        if (lo..hi).contains(&addr) {
            let name = f.get(5).map_or("[anonymous]", |n| n.rsplit('/').next().unwrap());
            return format!("{} {name}", f[1]);
        }
    }
    "not mapped".into()
}

fn main() {
    // 1. The auxiliary vector: key/value pairs the kernel pushes onto the new stack, after argv and envp.
    let entries: [(&str, u64); 9] = [
        ("AT_PHDR", libc::AT_PHDR), ("AT_ENTRY", libc::AT_ENTRY), ("AT_BASE", libc::AT_BASE),
        ("AT_SYSINFO_EHDR", libc::AT_SYSINFO_EHDR), ("AT_RANDOM", libc::AT_RANDOM), ("AT_EXECFN", libc::AT_EXECFN),
        ("AT_PAGESZ", libc::AT_PAGESZ), ("AT_PHNUM", libc::AT_PHNUM), ("AT_SECURE", libc::AT_SECURE),
    ];
    println!("--- auxiliary vector ---");
    for (name, key) in entries {
        // SAFETY: getauxval has no preconditions; it returns 0 for a missing key.
        let v = unsafe { libc::getauxval(key) };
        let place = if v > 0x10000 { mapping_of(v) } else { String::new() };
        println!("{name:<16} {v:#16x}  {place}");
    }
    // SAFETY: AT_EXECFN points at a NUL-terminated string the kernel copied onto the initial stack.
    let execfn = unsafe { std::ffi::CStr::from_ptr(libc::getauxval(libc::AT_EXECFN) as *const libc::c_char) };
    println!("AT_EXECFN string: {execfn:?}");

    // 2. The frames between the entry point and this function.
    println!("--- frames from here down to the entry point ---");
    let bt = std::backtrace::Backtrace::force_capture().to_string();
    for line in bt.lines().filter(|l| l.trim_start().starts_with(|c: char| c.is_ascii_digit())) {
        println!("{}", line.trim());
    }

    // 3. The C `main` that rustc generated for this binary.
    let exe = std::env::current_exe().unwrap().display().to_string();
    println!("--- the C-ABI `main` symbol rustc emitted ---");
    print!("{}", sh(&format!("objdump -d --no-show-raw-insn --disassemble=main {exe} | sed -n '/<main>:/,/^$/p' | c++filt")));
    print!("_start = {}", sh(&format!("nm {exe} | awk '$3 == \"_start\" {{print $1}}'")));
    println!("--- _start (from glibc's crt1.o): the first instructions of the program ---");
    print!("{}", sh(&format!("objdump -d --no-show-raw-insn --disassemble=_start {exe} | sed -n '/<_start>:/,/^$/p'")));
}
