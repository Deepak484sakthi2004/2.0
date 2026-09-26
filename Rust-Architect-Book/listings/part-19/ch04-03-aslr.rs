// verify: debug ok
// Address-space layout randomization: re-execute this program three times and print where the kernel and the loader
// put the executable, libc, the heap, the stack, and the vDSO. Then try to turn randomization off for one run.
use std::process::Command;

fn report() {
    // SAFETY: getauxval has no preconditions.
    let (phdr, vdso) = unsafe { (libc::getauxval(libc::AT_PHDR), libc::getauxval(libc::AT_SYSINFO_EHDR)) };
    let exe_base = phdr - 0x40; // program headers start at file offset 0x40 in the first LOAD segment (ch03-01)
    let libc_fn = libc::getpid as *const () as usize;
    let heap = Box::new(0u64);
    let stack_local = 0u64;
    println!(
        "exe {exe_base:#014x}  libc {libc_fn:#014x}  heap {:#014x}  stack {:#014x}  vdso {vdso:#014x}",
        &*heap as *const u64 as usize, &stack_local as *const u64 as usize
    );
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("report") {
        report();
        return;
    }
    let exe = std::env::current_exe().unwrap();
    println!("randomize_va_space = {}", std::fs::read_to_string("/proc/sys/kernel/randomize_va_space").unwrap().trim());
    for _ in 0..3 {
        let out = Command::new(&exe).arg("report").output().unwrap();
        print!("{}", String::from_utf8_lossy(&out.stdout));
    }
    println!("--- setarch -R (personality ADDR_NO_RANDOMIZE), twice ---");
    for _ in 0..2 {
        let out = Command::new("setarch").arg("-R").arg(&exe).arg("report").output().unwrap();
        print!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    }
}
