// verify: debug ok
// Part 1: inside the running process, find the GOT slot a call instruction uses and read what the dynamic loader
// wrote there. Part 2: compile C with gcc to show the relaxable relocation (GOTPCRELX) and what the linker does with it.
use std::process::Command;

#[inline(never)]
fn call_getpid() -> i32 {
    // SAFETY: getpid has no preconditions.
    unsafe { libc::getpid() }
}

fn mapping_of(addr: usize) -> String {
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
    for line in maps.lines() {
        let range = line.split_whitespace().next().unwrap();
        let (lo, hi) = range.split_once('-').unwrap();
        let (lo, hi) = (usize::from_str_radix(lo, 16).unwrap(), usize::from_str_radix(hi, 16).unwrap());
        if (lo..hi).contains(&addr) {
            let f: Vec<&str> = line.split_whitespace().collect();
            return format!("{} {}", f[1], f.get(5).unwrap_or(&"[anon]"));
        }
    }
    "unmapped".into()
}

fn sh(script: &str) -> String {
    let o = Command::new("sh").args(["-c", script]).output().expect("sh");
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

fn main() {
    assert!(call_getpid() > 0);
    // Part 1. Scan the first bytes of call_getpid for `ff 15 <disp32>`: call qword ptr [rip + disp32].
    let code = call_getpid as *const u8;
    // SAFETY: the function's code is mapped readable (r-xp); we read at most 64 bytes of it.
    let bytes = unsafe { std::slice::from_raw_parts(code, 64) };
    let i = bytes.windows(2).position(|w| w == [0xff, 0x15]).expect("no indirect call found");
    let disp = i32::from_le_bytes(bytes[i + 2..i + 6].try_into().unwrap());
    let next_ip = code as usize + i + 6;
    let slot = (next_ip as isize + disp as isize) as usize;
    // SAFETY: `slot` is the GOT entry this instruction reads; it's mapped and 8-byte aligned.
    let target = unsafe { *(slot as *const usize) };
    // SAFETY: dlsym with a valid NUL-terminated name.
    let dl = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"getpid".as_ptr()) } as usize;
    println!("call_getpid at {:#x}: `ff 15` at +{i}, disp32 = {disp:#x}", code as usize);
    println!("GOT slot        {slot:#x}  in mapping: {}", mapping_of(slot));
    println!("slot contains   {target:#x}  in mapping: {}", mapping_of(target));
    println!("dlsym(getpid) = {dl:#x}  same as slot: {}", dl == target);

    // Part 2. C with -fno-plt: gcc marks GOT loads it may rewrite with R_X86_64_GOTPCRELX.
    std::fs::write("/tmp/a.c", "int helper(int x);\nint getpid(void);\nint caller(int x) { return helper(x) + getpid(); }\nint main(void) { return caller(1) > 0 ? 0 : 1; }\n").unwrap();
    std::fs::write("/tmp/b.c", "int helper(int x) { return x * 3; }\n").unwrap();
    println!("--- gcc -O1 -fno-plt -c a.c: relocations in caller ---");
    must("cd /tmp && gcc -O1 -fno-plt -fPIE -c a.c b.c");
    print!("{}", sh("objdump -dr --disassemble=caller /tmp/a.o | sed -n '/<caller>/,/^$/p'"));
    println!("--- after linking a.o + b.o into a PIE ---");
    must("cd /tmp && gcc -pie a.o b.o -o ab");
    print!("{}", sh("cd /tmp && objdump -d --disassemble=caller ab | sed -n '/<caller>/,/^$/p' && ./ab && echo exit=$?"));
}

/// Run a build step; the listing fails if the step fails.
fn must(script: &str) {
    let o = Command::new("sh").args(["-c", script]).output().expect("sh");
    assert!(o.status.success(), "step failed: {script}\n{}", String::from_utf8_lossy(&o.stderr));
}
