// verify: debug error:future
//! An Rc held across an .await makes the whole future !Send: it can't move to another thread.
use std::rc::Rc;

async fn flush() {}

async fn handler() -> usize {
    let counter = Rc::new(1usize);
    flush().await; // `counter` is live across this await: it's stored in the future
    *counter
}

fn main() {
    // What a multi-threaded executor does: move the future to a worker thread.
    let fut = handler(); // created here...
    std::thread::spawn(move || futures::executor::block_on(fut)); // ...and moved to another thread
}
