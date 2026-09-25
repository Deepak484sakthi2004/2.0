// verify: debug error:E0282
// Method lookup walks the receiver's autoderef steps in order: Rc<RefCell<..>>, then RefCell<..>.
// With `std::borrow::Borrow` in scope, the TRAIT method `Borrow::borrow` applies at the first
// step (every T: Borrow<T>), so it wins over the INHERENT `RefCell::borrow` one step further in.
use std::borrow::Borrow; // added by an IDE auto-import for an unrelated helper
use std::cell::RefCell;
use std::rc::Rc;

fn main() {
    let routes: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(vec!["/pay".to_string()]));
    let r = routes.borrow(); // meant: RefCell::borrow
    println!("{} route(s)", r.len());
}
