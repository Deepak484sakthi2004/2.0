// verify: release ok
// Why this Part can't run `perf` on the Playground: the kernel refuses hardware counters to this process.
fn main() {
    let paranoid = std::fs::read_to_string("/proc/sys/kernel/perf_event_paranoid").unwrap_or_default();
    println!("perf_event_paranoid = {}", paranoid.trim());

    // A zeroed perf_event_attr asks for PERF_TYPE_HARDWARE (0) / PERF_COUNT_HW_CPU_CYCLES (0) on this thread.
    let mut attr = [0u8; 128];
    attr[4..8].copy_from_slice(&128u32.to_ne_bytes()); // attr.size
    // SAFETY: perf_event_open reads `attr.size` bytes from a valid, initialized buffer; the other arguments are
    // plain integers (pid 0 = this thread, cpu -1 = any, group -1 = none, flags 0).
    let fd = unsafe { libc::syscall(libc::SYS_perf_event_open, attr.as_ptr(), 0, -1, -1, 0) };
    println!("perf_event_open(cycles, this thread) = {fd}: {}", std::io::Error::last_os_error());
}
