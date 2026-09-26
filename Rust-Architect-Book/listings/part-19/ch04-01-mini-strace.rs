// verify: debug ok
// A minimal strace built on ptrace(2): re-executes this binary in "scenario" mode under PTRACE_TRACEME and records
// every system call (number, a few decoded arguments, return value) of every thread. Linux x86-64 only.
use std::collections::HashMap;
use std::ffi::CString;
use std::io::Write;
use std::os::unix::fs::FileExt;

fn syscall_name(nr: u64) -> &'static str {
    match nr {
        0 => "read", 1 => "write", 3 => "close", 5 => "fstat", 7 => "poll", 8 => "lseek", 9 => "mmap",
        10 => "mprotect", 11 => "munmap", 12 => "brk", 13 => "rt_sigaction", 14 => "rt_sigprocmask",
        15 => "rt_sigreturn", 16 => "ioctl", 17 => "pread64", 21 => "access", 24 => "sched_yield", 28 => "madvise",
        39 => "getpid", 56 => "clone", 58 => "vfork", 59 => "execve", 60 => "exit", 61 => "wait4", 72 => "fcntl",
        89 => "readlink", 131 => "sigaltstack", 157 => "prctl", 158 => "arch_prctl", 186 => "gettid",
        202 => "futex", 204 => "sched_getaffinity", 218 => "set_tid_address", 228 => "clock_gettime",
        230 => "clock_nanosleep", 231 => "exit_group", 257 => "openat", 262 => "newfstatat", 273 => "set_robust_list",
        247 => "waitid", 293 => "pipe2", 302 => "prlimit64", 318 => "getrandom", 332 => "statx", 334 => "rseq", 434 => "pidfd_open",
        435 => "clone3", 439 => "faccessat2",
        _ => "?",
    }
}

struct Call {
    tid: i32,
    nr: u64,
    args: [u64; 4],
    ret: i64,
    path: Option<String>, // decoded for openat
}

fn read_c_string(pid: i32, addr: u64) -> Option<String> {
    let mem = std::fs::File::open(format!("/proc/{pid}/mem")).ok()?;
    let mut buf = [0u8; 256];
    let n = mem.read_at(&mut buf, addr).ok()?;
    let end = buf[..n].iter().position(|&b| b == 0).unwrap_or(n);
    Some(String::from_utf8_lossy(&buf[..end]).into_owned())
}

/// Run `self <scenario>` under ptrace and return every completed system call, in order.
fn trace(scenario: &str, clean_env: bool) -> Vec<Call> {
    let exe = CString::new(std::env::current_exe().unwrap().to_str().unwrap()).unwrap();
    let arg = CString::new(scenario).unwrap();
    let env_path = CString::new("PATH=/usr/bin:/bin").unwrap();
    let dev_null = CString::new("/dev/null").unwrap();
    let mut calls = Vec::new();
    // SAFETY: fork/exec/ptrace/waitpid are used as documented; the child only calls async-signal-safe functions
    // (ptrace, execv/execve, _exit) between fork and exec; `regs` is plain data filled in by the kernel.
    unsafe {
        let pid = libc::fork();
        if pid == 0 {
            libc::ptrace(libc::PTRACE_TRACEME, 0, 0, 0);
            let null = libc::open(dev_null.as_ptr(), libc::O_WRONLY);
            libc::dup2(null, 1); // the scenario's stdout goes to /dev/null (still line-buffered: Rust's stdout always is)
            libc::close(null);
            let argv = [exe.as_ptr(), arg.as_ptr(), std::ptr::null()];
            if clean_env {
                let envp = [env_path.as_ptr(), std::ptr::null()];
                libc::execve(exe.as_ptr(), argv.as_ptr(), envp.as_ptr());
            } else {
                libc::execv(exe.as_ptr(), argv.as_ptr());
            }
            libc::_exit(127);
        }
        let mut status = 0;
        libc::waitpid(pid, &mut status, 0); // the SIGTRAP stop after execve succeeds
        let opts = libc::PTRACE_O_TRACESYSGOOD | libc::PTRACE_O_EXITKILL | libc::PTRACE_O_TRACECLONE;
        libc::ptrace(libc::PTRACE_SETOPTIONS, pid, 0, opts);
        let mut pending: HashMap<i32, Call> = HashMap::new(); // syscall entered, not yet returned (per thread)
        libc::ptrace(libc::PTRACE_SYSCALL, pid, 0, 0);
        loop {
            let tid = libc::waitpid(-1, &mut status, libc::__WALL);
            if tid < 0 {
                break;
            }
            if libc::WIFEXITED(status) || libc::WIFSIGNALED(status) {
                if tid == pid {
                    break;
                }
                continue;
            }
            let sig = libc::WSTOPSIG(status);
            let mut inject = 0;
            if sig == (libc::SIGTRAP | 0x80) {
                let mut regs: libc::user_regs_struct = std::mem::zeroed();
                libc::ptrace(libc::PTRACE_GETREGS, tid, 0, &mut regs);
                if let Some(mut c) = pending.remove(&tid) {
                    c.ret = regs.rax as i64; // syscall-exit stop
                    calls.push(c);
                } else {
                    let path = if regs.orig_rax == 257 { read_c_string(tid, regs.rsi) } else { None };
                    let args = [regs.rdi, regs.rsi, regs.rdx, regs.r10];
                    pending.insert(tid, Call { tid, nr: regs.orig_rax, args, ret: 0, path });
                }
            } else if sig != libc::SIGTRAP && sig != libc::SIGSTOP {
                inject = sig; // a real signal for the tracee: deliver it
            }
            libc::ptrace(libc::PTRACE_SYSCALL, tid, 0, inject);
        }
        // exit_group never returns: record it anyway
        for (_, c) in pending.drain() {
            calls.push(c);
        }
    }
    calls
}

fn describe(c: &Call) -> String {
    let [a0, a1, a2, a3] = c.args;
    let detail = match c.nr {
        257 => format!("{:?}", c.path.as_deref().unwrap_or("?")),
        9 => format!("len={a1} prot={a2} flags={a3:#x}"),
        10 => format!("len={a1} prot={a2}"),
        11 => format!("len={a1}"),
        13 => format!("sig={a0}"),
        56 => format!("flags={a0:#x}"),
        158 => format!("code={a0:#x}"),
        202 => format!("op={}", a1 & 0x7f),
        302 => format!("resource={a1}"),
        1 => format!("fd={a0} len={a2}"),
        0 => format!("fd={a0}"),
        7 => format!("nfds={a1}"),
        _ => String::new(),
    };
    let ret = if c.ret < 0 && c.ret > -4096 { format!("-{}", std::io::Error::from_raw_os_error((-c.ret) as i32)) } else { format!("{}", c.ret) };
    format!("{}({}) = {}", syscall_name(c.nr), detail, ret)
}

fn counts(calls: &[Call]) -> String {
    let mut m: Vec<(&str, usize)> = Vec::new();
    for c in calls {
        let n = syscall_name(c.nr);
        match m.iter_mut().find(|(k, _)| *k == n) {
            Some((_, v)) => *v += 1,
            None => m.push((n, 1)),
        }
    }
    m.iter().map(|(k, v)| format!("{k}x{v}")).collect::<Vec<_>>().join(" ")
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        // ---- scenarios, run as the traced child ----
        Some("empty") => {}
        Some("hello") => println!("hello"),
        Some("println100") => {
            for i in 0..100 {
                println!("line {i}");
            }
        }
        Some("bufwriter100") => {
            let mut out = std::io::BufWriter::new(std::io::stdout().lock());
            for i in 0..100 {
                writeln!(out, "line {i}").unwrap();
            }
        }
        Some("spawn") => {
            let h = std::thread::spawn(|| 7);
            let _ = h.join();
        }
        Some("command") => {
            let st = std::process::Command::new("/bin/true").status().unwrap();
            assert!(st.success());
        }
        Some("uncontended") | Some("contended") => {
            let threads = if std::env::args().nth(1).as_deref() == Some("contended") { 4 } else { 1 };
            let m = std::sync::Arc::new(std::sync::Mutex::new(0u64));
            let hs: Vec<_> = (0..threads)
                .map(|_| {
                    let m = m.clone();
                    std::thread::spawn(move || {
                        for _ in 0..10_000 {
                            *m.lock().unwrap() += 1;
                        }
                    })
                })
                .collect();
            for h in hs {
                h.join().unwrap();
            }
            assert_eq!(*m.lock().unwrap(), 10_000 * threads);
        }
        Some(_) => {}
        // ---- the tracer ----
        None => {
            let inherited = trace("empty", false);
            let clean = trace("empty", true);
            let failed_opens = |v: &[Call]| v.iter().filter(|c| c.nr == 257 && c.ret < 0).count();
            println!("== empty main(), cargo's environment: {} syscalls, {} failed openat", inherited.len(), failed_opens(&inherited));
            println!("== empty main(), clean environment:   {} syscalls, {} failed openat", clean.len(), failed_opens(&clean));
            for c in &clean {
                println!("   {}", describe(c));
            }
            for s in ["hello", "println100", "bufwriter100"] {
                let v = trace(s, true);
                let writes = v.iter().filter(|c| c.nr == 1 && c.args[0] == 1).count();
                println!("== {s}: {} syscalls, {} write(1, ...)", v.len(), writes);
            }
            let base = clean.len();
            let spawn = trace("spawn", true);
            println!("== spawn + join: {} syscalls ({} more than empty); threads seen: {}", spawn.len(), spawn.len() - base,
                spawn.iter().map(|c| c.tid).collect::<std::collections::BTreeSet<_>>().len());
            let first_new = spawn.iter().position(|c| c.nr == 204).unwrap_or(0);
            for c in &spawn[first_new..] {
                println!("   [{}] {}", if c.tid == spawn[0].tid { "main " } else { "child" }, describe(c));
            }
            println!("== counts, spawn + join: {}", counts(&spawn));

            // A child process: which system calls does Command::status() make in the parent?
            let cmd = trace("command", true);
            println!("== Command::new(\"/bin/true\").status(): {} syscalls ({} more than empty)", cmd.len(), cmd.len() - base);
            let start = cmd.iter().position(|c| matches!(c.nr, 56 | 58 | 435)).unwrap_or(0);
            for c in &cmd[start.saturating_sub(4)..] {
                println!("   {}", describe(c));
            }

            // Mutex: the fast path is a userspace atomic; futex(2) only when a thread must sleep or wake a sleeper.
            for s in ["uncontended", "contended"] {
                let v = trace(s, true);
                let mut ops: std::collections::BTreeMap<u64, usize> = std::collections::BTreeMap::new();
                for c in v.iter().filter(|c| c.nr == 202) {
                    *ops.entry(c.args[1] & 0x7f).or_default() += 1;
                }
                let names: Vec<String> = ops.iter().map(|(op, n)| {
                    let name = match op { 0 => "WAIT", 1 => "WAKE", 9 => "WAIT_BITSET", 10 => "WAKE_BITSET", _ => "other" };
                    format!("{name}x{n}")
                }).collect();
                println!("== {s} Mutex, {} lock/unlock pairs: {} futex calls [{}]", if s == "contended" { "4 x 10,000" } else { "1 x 10,000" },
                    v.iter().filter(|c| c.nr == 202).count(), names.join(" "));
            }
        }
    }
}
