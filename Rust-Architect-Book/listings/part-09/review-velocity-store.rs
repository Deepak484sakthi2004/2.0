// verify: debug ok
// Part IX review capstone: a Java-shaped port. It compiles, it passes its smoke test, and it has
// at least a dozen collection and memory problems. Find them before reading the answer key.
use std::collections::{HashMap, LinkedList};

#[derive(Clone, Debug)]
pub struct Event {
    pub merchant: String,
    pub card_hash: String, // 64 hex chars
    pub amount: f64,
    pub ts_ms: u64,
}

#[derive(Default)]
pub struct VelocityStore {
    events: HashMap<String, LinkedList<Event>>,
    amounts_by_merchant: HashMap<String, Vec<Box<f64>>>,
    card_graph: HashMap<String, Vec<String>>, // card -> merchants it was used at
    hot_merchants: Vec<(String, u64)>,
}

impl VelocityStore {
    pub fn record(&mut self, e: Event) {
        let key = format!("{}", e.merchant);
        let list = self.events.entry(key.clone()).or_insert(LinkedList::new());
        list.push_back(e.clone());
        self.amounts_by_merchant.entry(key).or_default().push(Box::new(e.amount));
        self.card_graph.entry(e.card_hash.clone()).or_default().push(e.merchant.clone());

        // keep a one-hour window of events per merchant
        let cutoff = e.ts_ms.saturating_sub(3_600_000);
        let list = self.events.get_mut(&e.merchant).unwrap();
        while list.front().map_or(false, |x| x.ts_ms < cutoff) {
            list.pop_front();
        }

        // top 10 merchants by events in the window, refreshed on every event
        self.hot_merchants = self.events.iter().map(|(m, l)| (m.clone(), l.len() as u64)).collect();
        self.hot_merchants.sort_by(|a, b| b.1.cmp(&a.1));
        self.hot_merchants.truncate(10);
    }

    pub fn total(&self, merchant: &str) -> f64 {
        self.amounts_by_merchant.get(merchant).map_or(0.0, |v| v.iter().map(|b| **b).sum())
    }

    pub fn hot(&self) -> &[(String, u64)] {
        &self.hot_merchants
    }

    /// Every merchant linked to `card` through chains of shared cards.
    pub fn linked_merchants(&self, card: &str, seen: &mut Vec<String>) {
        if let Some(ms) = self.card_graph.get(card) {
            for m in ms {
                if !seen.contains(m) {
                    seen.push(m.clone());
                    for (c, ms2) in &self.card_graph {
                        if ms2.contains(m) {
                            self.linked_merchants(c, seen);
                        }
                    }
                }
            }
        }
    }
}

fn main() {
    let mut s = VelocityStore::default();
    let ev = |m: &str, c: &str, amount: f64, ts_ms: u64| Event { merchant: m.into(), card_hash: c.into(), amount, ts_ms };
    s.record(ev("coffee-42", "card-a", 3.50, 1_000));
    s.record(ev("coffee-42", "card-b", 4.10, 2_000));
    s.record(ev("books-7", "card-b", 12.00, 3_000));
    s.record(ev("games-9", "card-c", 60.00, 4_000));
    s.record(ev("books-7", "card-c", 8.25, 3_700_000));

    println!("total coffee-42 = {:.2}", s.total("coffee-42"));
    println!("total books-7   = {:.2}", s.total("books-7"));
    let mut linked = Vec::new();
    s.linked_merchants("card-a", &mut linked);
    linked.sort();
    println!("linked to card-a: {linked:?}");
    println!("hot merchants: {} entries, top count {}", s.hot().len(), s.hot()[0].1);
}
