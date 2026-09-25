// verify: debug ok
use std::marker::PhantomData;
use std::rc::Rc;

pub struct Id<T> {
    raw: u64,
    _entity: PhantomData<T>, // !Send when T is !Send
}

pub struct Session {
    pub user: Rc<str>,
}

fn main() {
    let id: Id<Session> = Id { raw: 42, _entity: PhantomData };
    // Edition 2021+ closures capture disjoint fields: this closure captures only `id.raw` (a u64),
    // so Id<Session>'s !Send never comes into play.
    let h = std::thread::spawn(move || id.raw);
    println!("{}", h.join().unwrap());
}
