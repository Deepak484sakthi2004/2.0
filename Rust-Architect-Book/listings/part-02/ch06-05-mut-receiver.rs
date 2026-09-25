// verify: debug error:E0596
struct Counter {
    hits: u64,
}

impl Counter {
    fn hit(&mut self) {
        self.hits += 1;
    }
}

fn main() {
    let c = Counter { hits: 0 };
    c.hit();
    println!("{}", c.hits);
}
