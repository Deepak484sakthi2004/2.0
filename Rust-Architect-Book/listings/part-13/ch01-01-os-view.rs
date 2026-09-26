// verify: debug ok
//! Where Tokio ends and the OS begins: what the kernel can see of a Tokio runtime.
//! Threads come from /proc/self/task/*/comm; file descriptors from /proc/self/fd; what an epoll
//! instance is watching from /proc/self/fdinfo/<fd> (one "tfd:" line per registered descriptor).
use std::fs;
use std::time::Duration;

fn threads() -> Vec<String> {
    // A thread that is exiting can vanish between read_dir and reading its comm file: skip it.
    let mut names: Vec<String> = fs::read_dir("/proc/self/task")
        .unwrap()
        .filter_map(|e| fs::read_to_string(e.ok()?.path().join("comm")).ok())
        .map(|s| s.trim().to_string())
        .collect();
    names.sort();
    names
}

/// (fd number, what it is), skipping stdio pipes and the directory handle read_dir itself opens.
fn fds() -> Vec<(u32, String)> {
    let mut v: Vec<(u32, String)> = fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(|e| {
            let e = e.ok()?;
            let n: u32 = e.file_name().to_string_lossy().parse().ok()?;
            let target = fs::read_link(e.path()).ok()?.display().to_string();
            Some((n, target))
        })
        .filter(|(n, t)| *n > 2 && !t.starts_with("/proc"))
        .collect();
    v.sort();
    v
}

/// The descriptors an epoll instance watches: the "tfd:" lines of its fdinfo.
fn epoll_targets(fd: u32) -> Vec<u32> {
    fs::read_to_string(format!("/proc/self/fdinfo/{fd}"))
        .unwrap()
        .lines()
        .filter_map(|l| l.strip_prefix("tfd:"))
        .filter_map(|rest| rest.split_whitespace().next()?.parse().ok())
        .collect()
}

fn main() {
    println!("before any runtime");
    println!("  threads: {:?}", threads());
    println!("  fds:     {:?}", fds());

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all() // I/O driver + time driver (+ signal driver, compiled in with the "full" feature)
        .build()
        .unwrap();

    rt.block_on(async {
        // A listener, so the I/O driver has something of ours to watch.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let accept = tokio::spawn(async move { listener.accept().await.map(|_| ()) });
        tokio::time::sleep(Duration::from_millis(20)).await; // let the accept task register interest

        println!("inside a 4-worker runtime with one listening socket");
        println!("  threads: {:?}", threads());
        for (fd, what) in fds() {
            if what == "anon_inode:[eventpoll]" {
                println!("  fd {fd:>2} {what:<26} watching fds {:?}", epoll_targets(fd));
            } else {
                println!("  fd {fd:>2} {what}");
            }
        }
        let m = tokio::runtime::Handle::current().metrics();
        println!("  runtime metrics: {} workers, {} alive tasks", m.num_workers(), m.num_alive_tasks());
        accept.abort();
    });

    drop(rt);
    std::thread::sleep(Duration::from_millis(50)); // give exiting worker threads time to disappear
    println!("after dropping the runtime");
    println!("  threads: {:?}", threads());
    println!("  fds:     {:?}", fds());
}
