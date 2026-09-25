// verify: debug ok
// verify: debug@2018 error:E0277
//! Edition 2021 changed what a closure captures: `job.id` alone (a u64), not the whole `job`.
//! Under edition 2018 the closure captures `job`, whose Rc field is not Send, so the spawn is rejected.
use std::rc::Rc;
use std::thread;

struct Job {
    id: u64,
    trace: Rc<String>, // a per-thread trace buffer: not Send
}

fn main() {
    let job = Job { id: 21, trace: Rc::new(String::from("req-7")) };
    let h = thread::spawn(move || job.id * 2); // 2021+: captures only `job.id`
    println!("result {} (trace {} stays on main)", h.join().unwrap(), job.trace);
}
