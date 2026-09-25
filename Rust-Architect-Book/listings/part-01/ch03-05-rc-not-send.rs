// verify: debug error:E0277
use std::rc::Rc;
use std::thread;

fn main() {
    let shared = Rc::new(vec![1, 2, 3]);
    let clone = Rc::clone(&shared);
    let handle = thread::spawn(move || {
        println!("{:?}", clone);
    });
    handle.join().unwrap();
}
