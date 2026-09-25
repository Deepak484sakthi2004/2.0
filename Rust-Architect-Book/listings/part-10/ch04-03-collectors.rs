// verify: debug ok
// Java Collectors, translated: groupingBy, summingLong, partitioningBy, joining, toMap, and a custom collector.
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Copy)]
struct Txn {
    merchant: &'static str,
    cents: u64,
    refunded: bool,
}

/// A custom "collector": anything implementing FromIterator can be the target of collect().
#[derive(Debug, Default)]
struct Histogram {
    buckets: [u32; 4], // <1k, <10k, <100k, >=100k cents
}

impl FromIterator<u64> for Histogram {
    fn from_iter<I: IntoIterator<Item = u64>>(iter: I) -> Self {
        let mut h = Histogram::default();
        for cents in iter {
            let i = match cents {
                0..1_000 => 0,
                1_000..10_000 => 1,
                10_000..100_000 => 2,
                _ => 3,
            };
            h.buckets[i] += 1;
        }
        h
    }
}

fn main() {
    let txns = [
        Txn { merchant: "acme", cents: 1_250, refunded: false },
        Txn { merchant: "zeta", cents: 99_000, refunded: false },
        Txn { merchant: "acme", cents: 400, refunded: true },
        Txn { merchant: "beta", cents: 250_000, refunded: false },
        Txn { merchant: "acme", cents: 7_700, refunded: false },
    ];

    // groupingBy(merchant, summingLong(cents)): a fold into a map (BTreeMap for sorted output).
    let by_merchant: BTreeMap<&str, u64> = txns.iter().fold(BTreeMap::new(), |mut m, t| {
        *m.entry(t.merchant).or_insert(0) += t.cents;
        m
    });
    println!("sum by merchant: {by_merchant:?}");

    // partitioningBy(refunded)
    let (refunded, kept): (Vec<Txn>, Vec<Txn>) = txns.iter().partition(|t| t.refunded);
    println!("refunded={} kept={}", refunded.len(), kept.len());

    // joining(", ") over distinct merchants, in first-seen order
    let mut seen = HashSet::new();
    let merchants: Vec<&str> = txns.iter().map(|t| t.merchant).filter(|m| seen.insert(*m)).collect();
    println!("merchants: {}", merchants.join(", "));

    // unzip: one pass, two collections
    let (names, amounts): (Vec<&str>, Vec<u64>) = txns.iter().map(|t| (t.merchant, t.cents)).unzip();
    println!("names={names:?} amounts={amounts:?}");

    // a custom collector
    let hist: Histogram = txns.iter().map(|t| t.cents).collect();
    println!("{hist:?}");
}
