// verify: release ok
// One run on a shared machine: noisy. Compare ratios.
use std::hint::black_box;
use std::time::Instant;

/// 64 bytes: exactly one cache line per order.
#[allow(dead_code)]
#[derive(Clone, Copy)]
struct Order {
    id: u64,
    account: u64,
    price: i64,
    qty: u64,
    ts: u64,
    venue: u64,
    flags: u64,
    side: u64,
}

/// The same data, one Vec per field ("struct of arrays").
struct Orders {
    price: Vec<i64>,
    qty: Vec<u64>,
    // ...the other six columns would live here too
}

fn main() {
    println!("size_of::<Order>() = {}", std::mem::size_of::<Order>());
    let n = 2_000_000usize;
    let aos: Vec<Order> = (0..n as u64)
        .map(|i| Order { id: i, account: i % 97, price: (i % 1000) as i64, qty: i % 13, ts: i, venue: 1, flags: 0, side: i % 2 })
        .collect();
    let soa = Orders { price: aos.iter().map(|o| o.price).collect(), qty: aos.iter().map(|o| o.qty).collect() };
    println!("AoS bytes touched per scan: {} MB; SoA price column: {} MB", n * 64 >> 20, n * 8 >> 20);

    for round in 0..3 {
        let t = Instant::now();
        let s1: i64 = black_box(&aos).iter().map(|o| o.price).sum();
        let t_aos = t.elapsed();
        let t = Instant::now();
        let s2: i64 = black_box(&soa.price).iter().sum();
        let t_soa = t.elapsed();
        let t = Instant::now();
        let n1: i64 = black_box(&aos).iter().map(|o| o.price * o.qty as i64).sum();
        let t_aos2 = t.elapsed();
        let t = Instant::now();
        let n2: i64 = black_box(&soa.price).iter().zip(&soa.qty).map(|(p, q)| p * *q as i64).sum();
        let t_soa2 = t.elapsed();
        assert!(s1 == s2 && n1 == n2);
        println!(
            "round {round}: sum(price) AoS {t_aos:>9.2?} SoA {t_soa:>9.2?} | sum(price*qty) AoS {t_aos2:>9.2?} SoA {t_soa2:>9.2?}"
        );
    }
}
