// verify: debug ok
use std::cell::RefCell;
use std::rc::{Rc, Weak};

struct Dept {
    name: String,
    parent: RefCell<Weak<Dept>>,      // back-edge: does NOT keep the parent alive
    children: RefCell<Vec<Rc<Dept>>>, // owning edges
}

impl Drop for Dept {
    fn drop(&mut self) {
        println!("  drop {}", self.name);
    }
}

fn dept(name: &str) -> Rc<Dept> {
    Rc::new(Dept { name: name.into(), parent: RefCell::new(Weak::new()), children: RefCell::new(vec![]) })
}

fn adopt(parent: &Rc<Dept>, child: Rc<Dept>) {
    *child.parent.borrow_mut() = Rc::downgrade(parent);
    parent.children.borrow_mut().push(child);
}

fn path(d: &Rc<Dept>) -> String {
    let mut parts = vec![d.name.clone()];
    let mut current = d.parent.borrow().upgrade();
    while let Some(p) = current {
        parts.push(p.name.clone());
        current = p.parent.borrow().upgrade();
    }
    parts.reverse();
    parts.join(" / ")
}

fn main() {
    let payments;
    {
        let root = dept("Meridian");
        let eng = dept("Engineering");
        payments = dept("Payments");
        adopt(&eng, Rc::clone(&payments));
        adopt(&root, eng);
        println!("{}", path(&payments));
        println!("root:     strong={} weak={}", Rc::strong_count(&root), Rc::weak_count(&root));
        println!("payments: strong={} weak={}", Rc::strong_count(&payments), Rc::weak_count(&payments));
        println!("-- `root` goes out of scope --");
    }
    println!("parent still alive? {}", payments.parent.borrow().upgrade().is_some());
    println!("path now: {}", path(&payments));
    println!("-- end of main --");
}
