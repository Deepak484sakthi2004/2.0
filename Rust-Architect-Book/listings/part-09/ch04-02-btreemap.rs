// verify: debug ok
use std::collections::{BTreeMap, VecDeque};

/// Price in integer ticks (never f64 keys), each level a FIFO queue of order quantities.
#[derive(Default)]
struct Book {
    bids: BTreeMap<i64, VecDeque<u64>>,
    asks: BTreeMap<i64, VecDeque<u64>>,
}

impl Book {
    fn add(&mut self, side: char, price: i64, qty: u64) {
        let levels = if side == 'B' { &mut self.bids } else { &mut self.asks };
        levels.entry(price).or_default().push_back(qty);
    }
    fn best_bid(&self) -> Option<i64> {
        self.bids.last_key_value().map(|(p, _)| *p)
    }
    fn best_ask(&self) -> Option<i64> {
        self.asks.first_key_value().map(|(p, _)| *p)
    }
    fn depth(&self, side: char, n: usize) -> Vec<(i64, u64)> {
        let sum = |(p, q): (&i64, &VecDeque<u64>)| (*p, q.iter().sum());
        if side == 'B' {
            self.bids.iter().rev().take(n).map(sum).collect()
        } else {
            self.asks.iter().take(n).map(sum).collect()
        }
    }
}

fn main() {
    let mut book = Book::default();
    for (side, price, qty) in [
        ('B', 10_040, 5), ('B', 10_050, 2), ('B', 10_050, 7), ('B', 10_030, 1), ('B', 10_045, 4),
        ('S', 10_060, 3), ('S', 10_070, 8), ('S', 10_055, 6),
    ] {
        book.add(side, price, qty);
    }
    println!("best bid {:?}, best ask {:?}", book.best_bid(), book.best_ask());
    println!("top 3 bids: {:?}", book.depth('B', 3));
    println!("top 3 asks: {:?}", book.depth('S', 3));
    let near: Vec<i64> = book.bids.range(10_040..=10_050).map(|(p, _)| *p).collect();
    println!("bid levels in [10040, 10050]: {near:?}");

    // Time series: range queries by timestamp are the reason to pay for ordering.
    let series: BTreeMap<u64, f64> = (0..1_000u64).map(|s| (1_700_000_000 + s * 60, s as f64)).collect();
    let window: Vec<u64> = series.range(1_700_000_000 + 600..1_700_000_000 + 900).map(|(t, _)| *t).collect();
    println!("points in a 5-minute window: {window:?}");
    let before = series.range(..1_700_000_000 + 125).next_back();
    println!("last point at or before t+125s: {before:?}");
}
