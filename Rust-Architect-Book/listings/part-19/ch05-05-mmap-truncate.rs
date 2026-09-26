// verify: debug ok
// The mmap failure mode: the file shrinks while it is mapped (log rotation with `copytruncate`). Touching a page past
// the new end of file raises SIGBUS and kills the process. This is why memmap's `Mmap::map` is an `unsafe fn`.
// The child does the dangerous part; the parent reports how the child ended.
use std::io::Write;
use std::os::unix::process::ExitStatusExt;

fn child() {
    let path = "/tmp/rotating.log";
    {
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(&vec![b'x'; 1 << 20]).unwrap(); // 1 MiB
    }
    let f = std::fs::File::open(path).unwrap();
    // SAFETY (deliberately violated below): the mapping stays valid only while the file keeps its length.
    let map = unsafe { memmap::Mmap::map(&f).unwrap() };
    println!("child: mapped {} bytes; byte at 512 KiB = {:?}", map.len(), map[512 * 1024] as char);
    // Another process rotates the log: copy it away, then truncate in place.
    std::fs::OpenOptions::new().write(true).open(path).unwrap().set_len(0).unwrap();
    println!("child: file truncated to 0 bytes; reading offset 512 KiB of the mapping again");
    let b = std::hint::black_box(map[512 * 1024]);
    println!("child: never printed: {b}");
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("child") {
        child();
        return;
    }
    let out = std::process::Command::new(std::env::current_exe().unwrap()).arg("child").output().unwrap();
    print!("{}", String::from_utf8_lossy(&out.stdout));
    println!("parent: child ended with {:?}, signal {:?} (SIGBUS = {})", out.status, out.status.signal(), libc::SIGBUS);
    assert_eq!(out.status.signal(), Some(libc::SIGBUS));
}
