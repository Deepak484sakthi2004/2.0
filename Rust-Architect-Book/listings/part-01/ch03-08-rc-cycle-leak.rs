// verify: debug ok
use std::cell::RefCell;
use std::rc::Rc;

struct Node {
    name: &'static str,
    next: RefCell<Option<Rc<Node>>>,
}

impl Drop for Node {
    fn drop(&mut self) {
        println!("dropping {}", self.name);
    }
}

fn main() {
    {
        let a = Rc::new(Node { name: "a", next: RefCell::new(None) });
        let b = Rc::new(Node { name: "b", next: RefCell::new(Some(Rc::clone(&a))) });
        *a.next.borrow_mut() = Some(Rc::clone(&b)); // a -> b -> a: a cycle
        println!("a strong count = {}", Rc::strong_count(&a));
        println!("b strong count = {}", Rc::strong_count(&b));
    }
    println!("scope ended; no \"dropping\" line above means both nodes leaked");
}
