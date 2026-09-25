// verify: debug build
// The order-book design from the Part III review. It compiles. Every problem in it is an OWNERSHIP
// design problem that the compiler accepted because Rc<RefCell<...>> moved the checks to run time (or nowhere).
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone)]
pub struct Order {
    pub id: u64,
    pub price_cents: i64,
    pub qty: u32,
    pub book: Option<Rc<RefCell<OrderBook>>>, // back-reference "for convenience"
}

pub struct OrderBook {
    pub bids: Vec<Rc<RefCell<Order>>>,
    pub asks: Vec<Rc<RefCell<Order>>>,
    pub by_id: HashMap<u64, Rc<RefCell<Order>>>,
    pub history: Vec<Order>, // a snapshot of every order ever placed
}

impl OrderBook {
    pub fn place(book: &Rc<RefCell<OrderBook>>, mut order: Order, is_bid: bool) {
        order.book = Some(Rc::clone(book));
        let shared = Rc::new(RefCell::new(order.clone()));
        let mut b = book.borrow_mut();
        b.history.push(order);
        b.by_id.insert(shared.borrow().id, Rc::clone(&shared));
        if is_bid {
            b.bids.push(shared);
        } else {
            b.asks.push(shared);
        }
        b.bids.sort_by(|x, y| y.borrow().price_cents.cmp(&x.borrow().price_cents));
    }

    pub fn cancel(&mut self, id: u64) {
        self.by_id.remove(&id);
    }
}
