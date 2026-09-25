// verify: debug ok
struct Noisy(&'static str);

impl Drop for Noisy {
    fn drop(&mut self) {
        println!("  drop {}", self.0);
    }
}

struct Order {
    id: u32,
    customer: Noisy,
    lines: Vec<Noisy>,
}

impl Drop for Order {
    fn drop(&mut self) {
        println!("  drop Order #{} (its fields follow)", self.id);
    }
}

fn ship(order: Order) -> u32 {
    // `ship` OWNS the order now
    println!("shipping #{}", order.id);
    order.id
} // `order` goes out of scope here: the whole ownership tree is dropped

fn main() {
    let a = Order { id: 1, customer: Noisy("customer A"), lines: vec![Noisy("line A1"), Noisy("line A2")] };
    let b = Order { id: 2, customer: Noisy("customer B"), lines: vec![Noisy("line B1")] };
    let shipped = ship(a); // ownership of `a` moves into `ship`
    println!("shipped #{shipped}; main ends");
} // `b` goes out of scope here
