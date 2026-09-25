// verify: debug ok
fn average(xs: &[u32]) -> Option<f64> {
    // a shared borrow: read-only access
    if xs.is_empty() {
        return None;
    }
    Some(xs.iter().map(|&x| x as f64).sum::<f64>() / xs.len() as f64)
}

fn normalize(xs: &mut Vec<u32>) {
    // an exclusive borrow: may mutate
    xs.sort_unstable();
    xs.dedup();
}

fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    // the returned reference borrows from the inputs (Part IV explains 'a)
    if a.len() >= b.len() { a } else { b }
}

fn main() {
    let mut latencies = vec![120, 45, 45, 300, 80];

    let avg = average(&latencies); // readers...
    let r1 = &latencies;
    let r2 = &latencies; // ...as many as you like, at the same time
    println!("avg={avg:?} len via r1={} first via r2={}", r1.len(), r2[0]);

    normalize(&mut latencies); // r1 and r2 are no longer used, so an exclusive borrow is allowed
    println!("normalized: {latencies:?}");

    let m = &mut latencies; // one writer...
    m.push(999);
    println!("after push: {latencies:?}"); // ...whose borrow ended at its last use

    latencies.push(latencies.len() as u32); // two-phase borrow: the argument is evaluated first
    println!("pushed its own length: {latencies:?}");

    let service = String::from("gateway");
    let winner = longest(&service, "db");
    println!("longest: {winner}");
}
