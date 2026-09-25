// verify: debug ok
// Two fixes: don't import `Borrow` into modules that use RefCell, or name the method you mean.
use std::borrow::Borrow;
use std::cell::RefCell;
use std::rc::Rc;

fn first_len<K: Borrow<str>>(keys: &[K]) -> usize {
    keys.first().map(|k| k.borrow().len()).unwrap_or(0) // the helper that needed the import
}

fn main() {
    let routes: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(vec!["/pay".to_string()]));
    let r = RefCell::borrow(&routes); // fully qualified: no lookup ambiguity
    println!("{} route(s), first key len {}", r.len(), first_len(&r));
}
