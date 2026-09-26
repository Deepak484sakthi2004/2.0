// verify: debug ok
//! !Send state is fine on ONE thread: a LocalSet runs spawn_local tasks on the thread that drives it.
use std::cell::RefCell;
use std::rc::Rc;
use tokio::task::LocalSet;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let counts: Rc<RefCell<Vec<u32>>> = Rc::new(RefCell::new(Vec::new()));
    let local = LocalSet::new();
    for i in 0..3 {
        let counts = Rc::clone(&counts);
        local.spawn_local(async move {
            tokio::task::yield_now().await; // Rc alive across .await: fine, this task never changes threads
            counts.borrow_mut().push(i);
        });
    }
    local.await; // runs every spawn_local task to completion
    println!("counts = {:?}, Rc strong count = {}", counts.borrow(), Rc::strong_count(&counts));
}
