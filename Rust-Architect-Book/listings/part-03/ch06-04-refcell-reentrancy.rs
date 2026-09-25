// verify: debug panic borrowed
use std::cell::RefCell;
use std::rc::Rc;

type Handler = Box<dyn Fn(&str)>;

struct EventBus {
    handlers: RefCell<Vec<Handler>>,
}

impl EventBus {
    fn subscribe(&self, handler: Handler) {
        self.handlers.borrow_mut().push(handler);
    }

    fn publish(&self, event: &str) {
        for handler in self.handlers.borrow().iter() {
            // a shared borrow held for the whole loop
            handler(event);
        }
    }
}

fn main() {
    let bus = Rc::new(EventBus { handlers: RefCell::new(Vec::new()) });
    bus.subscribe(Box::new(|e| println!("audit: {e}")));

    let bus_for_handler = Rc::clone(&bus);
    bus.subscribe(Box::new(move |e| {
        if e == "user.created" {
            // A handler that registers a follow-up handler while it is being notified.
            bus_for_handler.subscribe(Box::new(|e| println!("welcome-email: {e}")));
        }
    }));

    bus.publish("user.created");
}
