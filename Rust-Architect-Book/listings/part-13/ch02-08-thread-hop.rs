// verify: release ok
//! On the multi-thread runtime a task can resume on a different worker after any .await.
//! thread_local! state therefore "changes" under a task's feet; task_local! follows the task.
use std::cell::Cell;
use std::collections::HashSet;

thread_local! {
    static REQUEST_ID: Cell<u64> = const { Cell::new(0) };
}
tokio::task_local! {
    static TASK_REQUEST_ID: u64;
}

async fn handle(id: u64) -> (usize, u32, u32) {
    let mut threads = HashSet::new();
    REQUEST_ID.with(|r| r.set(id)); // "set once at the start of the request"
    let (mut tl_wrong, mut task_local_wrong) = (0, 0);
    for _ in 0..200 {
        threads.insert(std::thread::current().id());
        tokio::task::yield_now().await; // any .await may move the task
        if REQUEST_ID.with(|r| r.get()) != id {
            tl_wrong += 1;
        }
        if TASK_REQUEST_ID.get() != id {
            task_local_wrong += 1;
        }
    }
    (threads.len(), tl_wrong, task_local_wrong)
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let tasks: Vec<_> = (1..=8u64).map(|id| tokio::spawn(TASK_REQUEST_ID.scope(id, handle(id)))).collect();
    let (mut max_threads, mut tl_wrong, mut task_local_wrong) = (0, 0, 0);
    for t in tasks {
        let (n, a, b) = t.await.unwrap();
        max_threads = max_threads.max(n);
        tl_wrong += a;
        task_local_wrong += b;
    }
    println!("most worker threads one task ran on: {max_threads}");
    println!("reads of the thread_local that saw ANOTHER request's id: {tl_wrong} of 1,600");
    println!("reads of the task_local that saw another request's id:  {task_local_wrong} of 1,600");
}
