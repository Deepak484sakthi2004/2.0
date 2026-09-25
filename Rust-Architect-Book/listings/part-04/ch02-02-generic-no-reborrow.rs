// verify: debug error:E0382
fn bump(counter: &mut u32) {
    *counter += 1;
}

fn record<T: std::fmt::Debug>(value: T) {
    // a generic "metrics" wrapper added during a refactor
    println!("recorded {value:?}");
}

fn main() {
    let mut hits = 0u32;
    let r = &mut hits;
    bump(r); // implicit reborrow: &mut *r
    bump(r); // r is still usable
    record(r); // generic parameter T = &mut u32: NO implicit reborrow, `r` is MOVED
    *r += 1;
    println!("{hits}");
}
