// verify: debug ok
// The same parent/child link with shared ownership: Rc for owning edges,
// Weak for back edges (so the cycle does not leak), RefCell to mutate after construction.
use std::cell::RefCell;
use std::rc::{Rc, Weak};

struct Node {
    name: String,
    parent: RefCell<Weak<Node>>,
    children: RefCell<Vec<Rc<Node>>>,
}

impl Node {
    fn new(name: &str) -> Rc<Node> {
        Rc::new(Node {
            name: name.to_string(),
            parent: RefCell::new(Weak::new()),
            children: RefCell::new(Vec::new()),
        })
    }
}

fn main() {
    let root = Node::new("root");
    let etc = Node::new("etc");
    *etc.parent.borrow_mut() = Rc::downgrade(&root);
    root.children.borrow_mut().push(Rc::clone(&etc));

    let parent_name = etc.parent.borrow().upgrade().map(|p| p.name.clone());
    println!("etc's parent: {parent_name:?}");
    println!("root: strong = {}, weak = {}", Rc::strong_count(&root), Rc::weak_count(&root));
}
