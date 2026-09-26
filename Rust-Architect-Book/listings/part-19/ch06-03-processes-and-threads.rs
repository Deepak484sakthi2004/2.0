// verify: debug ok
// Processes and threads are both kernel tasks. fork() copies an address space lazily (copy-on-write: measured as page
// faults in the child); a thread is a task that shares everything (decode the clone flags ch04-01 recorded); and the
// kernel lists threads under /proc/self/task with the names std gives them.
fn minor_faults() -> i64 {
    // SAFETY: getrusage writes into the zeroed struct we pass.
    unsafe {
        let mut u: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut u);
        u.ru_minflt
    }
}

fn main() {
    // 1. fork + copy-on-write. Single-threaded at this point, so fork is safe to use here.
    let mut data = vec![1u8; 64 << 20]; // 64 MiB, every page touched
    // SAFETY: the process has one thread; the child only touches its own copy of `data`, prints, and _exits.
    let pid = unsafe { libc::fork() };
    if pid == 0 {
        let f0 = minor_faults();
        for i in (0..data.len() / 4).step_by(4096) {
            data[i] = 2; // write to the first 16 MiB: 4,096 pages
        }
        let f1 = minor_faults();
        let _ = std::hint::black_box(data[0]);
        let reads: u64 = data.iter().step_by(4096).map(|&b| u64::from(b)).sum();
        let f2 = minor_faults();
        println!("child:  wrote 16 MiB -> {} faults (copy-on-write); read all 64 MiB -> {} faults; sum {reads}", f1 - f0, f2 - f1);
        // SAFETY: _exit skips atexit handlers and stdio flushing, which belong to the parent's copy.
        unsafe { libc::_exit(0) };
    }
    let mut status = 0;
    // SAFETY: waiting for the child we just forked.
    unsafe { libc::waitpid(pid, &mut status, 0) };
    println!("parent: child exited ({status}); parent still sees data[0] = {}", data[0]);

    // 2. The clone flags std's thread spawn passed (recorded by ch04-01's tracer: 0x3d0f00).
    let recorded: i32 = 0x3d0f00;
    let known = [
        ("CLONE_VM", libc::CLONE_VM), ("CLONE_FS", libc::CLONE_FS), ("CLONE_FILES", libc::CLONE_FILES),
        ("CLONE_SIGHAND", libc::CLONE_SIGHAND), ("CLONE_THREAD", libc::CLONE_THREAD), ("CLONE_SYSVSEM", libc::CLONE_SYSVSEM),
        ("CLONE_SETTLS", libc::CLONE_SETTLS), ("CLONE_PARENT_SETTID", libc::CLONE_PARENT_SETTID),
        ("CLONE_CHILD_CLEARTID", libc::CLONE_CHILD_CLEARTID),
    ];
    let names: Vec<&str> = known.iter().filter(|(_, f)| recorded & f != 0).map(|(n, _)| *n).collect();
    let covered = known.iter().filter(|(_, f)| recorded & f != 0).fold(0, |a, (_, f)| a | f);
    println!("thread clone flags {recorded:#x} = {}{}", names.join(" | "), if covered == recorded { "" } else { " | ?" });

    // 3. Threads as tasks: ids and names in /proc/self/task.
    let started = std::sync::Arc::new(std::sync::Barrier::new(3));
    let workers: Vec<_> = (0..2)
        .map(|i| {
            let started = started.clone();
            std::thread::Builder::new()
                .name(format!("settlement-worker-{i}"))
                .spawn(move || {
                    started.wait();
                    std::thread::sleep(std::time::Duration::from_millis(100));
                })
                .unwrap()
        })
        .collect();
    started.wait(); // both workers are running
    // SAFETY: getpid/gettid have no preconditions.
    let (pid, tid) = unsafe { (libc::getpid(), libc::gettid()) };
    println!("main thread: pid {pid}, tid {tid}");
    let mut tasks: Vec<(i32, String)> = std::fs::read_dir("/proc/self/task").unwrap()
        .map(|e| {
            let id: i32 = e.unwrap().file_name().into_string().unwrap().parse().unwrap();
            let comm = std::fs::read_to_string(format!("/proc/self/task/{id}/comm")).unwrap();
            (id, comm.trim().to_string())
        })
        .collect();
    tasks.sort();
    for (id, comm) in &tasks {
        println!("  task {id:>5}  comm {comm:?}");
    }
    for w in workers {
        w.join().unwrap();
    }
}
