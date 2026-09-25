// verify: debug error:E0277
use std::marker::PhantomData;
use std::rc::Rc;

// Id written with PhantomData<T>: it now "contains a T" for auto-trait purposes.
pub struct Id<T> {
    raw: u64,
    _entity: PhantomData<T>,
}

pub struct Session {
    pub user: Rc<str>, // sessions are single-threaded: Rc, not Arc
}

fn audit(id: Id<Session>) -> u64 {
    id.raw
}

fn main() {
    let id: Id<Session> = Id { raw: 42, _entity: PhantomData };
    // Sending a u64 to another thread should be trivial...
    let h = std::thread::spawn(move || audit(id));
    println!("{}", h.join().unwrap());
}
