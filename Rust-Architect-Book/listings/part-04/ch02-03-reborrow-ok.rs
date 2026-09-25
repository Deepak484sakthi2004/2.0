// verify: debug ok
fn bump(counter: &mut u32) {
    *counter += 1;
}

fn record<T: std::fmt::Debug>(value: T) {
    println!("recorded {value:?}");
}

fn main() {
    let mut hits = 0u32;
    let r = &mut hits;
    bump(r);
    bump(r);
    record(&mut *r); // an EXPLICIT reborrow: a fresh, shorter &mut derived from r
    *r += 1; // r usable again once the reborrow has ended
    record(&*r); // a shared reborrow works the same way
    println!("{hits}");
}
