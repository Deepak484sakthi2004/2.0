// verify: debug ok
//! A std::thread is an OS thread: on Linux, a kernel task with its own TID and its own mmap'd stack.
//! Reads /proc while four threads are parked at a barrier, then again after they're joined.
use std::sync::{Arc, Barrier};
use std::thread;

/// The /proc/self/maps line of the mapping that contains `addr`, plus the line just below it (the guard page).
fn stack_mapping(addr: usize) -> (String, String) {
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
    let mut below = String::new();
    for line in maps.lines() {
        let range = line.split_whitespace().next().unwrap();
        let (lo, hi) = range.split_once('-').unwrap();
        let (lo, hi) = (usize::from_str_radix(lo, 16).unwrap(), usize::from_str_radix(hi, 16).unwrap());
        if lo <= addr && addr < hi {
            let size_kib = (hi - lo) / 1024;
            return (format!("{} {} KiB", &line[..line.find(" 00000000").unwrap()], size_kib), below);
        }
        below = line.to_string();
    }
    unreachable!("a stack address is always mapped")
}

fn threads_in_status() -> String {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    status.lines().find(|l| l.starts_with("Threads:")).unwrap().replace('\t', " ")
}

fn main() {
    let pid = std::process::id();
    println!("main: pid {pid}, tid {}", gettid());

    let n = 4;
    let ready = Arc::new(Barrier::new(n + 1)); // four workers + main
    let done = Arc::new(Barrier::new(n + 1));
    let mut handles = Vec::new();
    for i in 0..n {
        let (ready, done) = (Arc::clone(&ready), Arc::clone(&done));
        let builder = thread::Builder::new().name(format!("worker-{i}"));
        // One worker asks for a smaller stack, to show that the size is just an mmap length.
        let builder = if i == 3 { builder.stack_size(256 * 1024) } else { builder };
        handles.push(
            builder
                .spawn(move || {
                    let local = 0u8; // lives on this thread's stack
                    let (mapping, guard) = stack_mapping(&local as *const u8 as usize);
                    let tid = gettid();
                    ready.wait(); // everyone alive at the same time
                    done.wait(); // main has looked at /proc; now exit
                    (thread::current().name().unwrap().to_string(), tid, mapping, guard)
                })
                .expect("spawn failed"),
        );
    }

    ready.wait();
    let mut tasks: Vec<String> = std::fs::read_dir("/proc/self/task")
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    tasks.sort();
    println!("while running: /proc/self/task = {tasks:?}, {}", threads_in_status());
    done.wait();

    for h in handles {
        let (name, tid, mapping, guard) = h.join().unwrap();
        println!("{name}: tid {tid}, stack {mapping}");
        println!("{:>9}  guard {}", "", &guard[..guard.find(" 00000000").unwrap()]);
    }
    println!("after join:    {}", threads_in_status());
}

/// gettid(2) through the libc crate: `unsafe` only because it is an FFI call.
fn gettid() -> i32 {
    // SAFETY: gettid takes no arguments, has no preconditions, and always succeeds.
    unsafe { libc::gettid() }
}
