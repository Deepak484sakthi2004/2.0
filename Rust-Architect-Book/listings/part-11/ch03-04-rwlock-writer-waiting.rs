// verify: debug ok
//! A reader holds the lock, a writer queues up behind it. May a NEW reader get in?
//! std's RwLock on Linux (futex-based) says no: waiting writers block new readers, even a thread that
//! already holds a read lock. That's why "read lock inside a read lock" can deadlock. parking_lot compared.
use std::sync::mpsc;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

fn main() {
    let routes = Arc::new(RwLock::new(vec!["/a"]));
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let (report_tx, report_rx) = mpsc::channel::<String>();

    // Reader 1: takes a read lock and holds it until told to release.
    let r1 = {
        let (routes, report_tx) = (Arc::clone(&routes), report_tx.clone());
        thread::spawn(move || {
            let g = routes.read().unwrap();
            release_rx.recv().unwrap(); // (main is testing things meanwhile)
            // Re-entrant read while the writer waits: with read() this thread would wait for itself.
            let again = routes.try_read().is_ok();
            report_tx.send(format!("reader 1 re-reads while writer waits: try_read ok = {again}")).unwrap();
            drop(g);
        })
    };
    thread::sleep(Duration::from_millis(100));

    // Writer: blocks behind reader 1.
    let w = {
        let (routes, report_tx) = (Arc::clone(&routes), report_tx.clone());
        thread::spawn(move || {
            routes.write().unwrap().push("/b");
            report_tx.send("writer got the lock".to_string()).unwrap();
        })
    };
    thread::sleep(Duration::from_millis(100)); // let the writer reach its futex wait

    // A new reader on another thread: may it join reader 1?
    let new_reader = routes.try_read().is_ok();
    println!("new reader while writer waits (std):   try_read ok = {new_reader}");

    release_tx.send(()).unwrap();
    r1.join().unwrap();
    w.join().unwrap();
    drop(report_tx);
    for line in report_rx {
        println!("{line}");
    }
    println!("final routes: {:?}", routes.read().unwrap());

    // parking_lot::RwLock: the same experiment.
    let pl = Arc::new(parking_lot::RwLock::new(0u32));
    let g = pl.read();
    let w = {
        let pl = Arc::clone(&pl);
        thread::spawn(move || *pl.write() += 1)
    };
    thread::sleep(Duration::from_millis(100));
    let other = {
        let pl = Arc::clone(&pl);
        thread::spawn(move || pl.try_read().is_some()).join().unwrap()
    };
    println!("new reader while writer waits (parking_lot): try_read ok = {other}");
    println!("same thread re-reads (parking_lot):          try_read ok = {}", pl.try_read().is_some());
    println!("same thread re-reads, read_recursive:         ok = {}", { let _r = pl.read_recursive(); true });
    drop(g);
    w.join().unwrap();
    println!("parking_lot value after writer: {}", *pl.read());
}
