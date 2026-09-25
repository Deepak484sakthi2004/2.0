// verify: debug ok
use std::collections::HashMap;
use std::sync::Mutex;
use std::thread;

struct Inventory {
    stock: Mutex<HashMap<String, i64>>, // the lock OWNS the data it protects
}

impl Inventory {
    fn reserve(&self, sku: &str) {
        let mut stock = self.stock.lock().unwrap();
        *stock.entry(sku.to_string()).or_insert(0) -= 1;
    }

    fn available(&self, sku: &str) -> i64 {
        // There is no path to the map that skips the lock.
        self.stock.lock().unwrap().get(sku).copied().unwrap_or(0)
    }
}

fn main() {
    let inventory = Inventory {
        stock: Mutex::new(HashMap::from([("sku-1".to_string(), 1_000)])),
    };
    thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| {
                for _ in 0..100 {
                    inventory.reserve("sku-1");
                }
            });
        }
    });
    println!("available: {}", inventory.available("sku-1"));
}
