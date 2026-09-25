// verify: debug ok
//! Mutex<T> owns the data it protects: the only road to the HashMap goes through lock().
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

fn main() {
    let hits: Arc<Mutex<HashMap<String, u64>>> = Arc::new(Mutex::new(HashMap::new()));

    let workers: Vec<_> = (0..4)
        .map(|w| {
            let hits = Arc::clone(&hits);
            thread::spawn(move || {
                for i in 0..1_000 {
                    let route = if i % 4 == w { "/checkout" } else { "/health" };
                    // Keep the critical section tiny: build the key before locking.
                    let key = route.to_string();
                    let mut map = hits.lock().unwrap(); // MutexGuard<HashMap<..>>: DerefMut to the map
                    *map.entry(key).or_insert(0) += 1;
                } // guard dropped at the end of each iteration: unlocked
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }

    // Sole owner again: no locking needed to get the data out.
    let map = Arc::try_unwrap(hits).expect("all workers joined").into_inner().unwrap();
    let mut rows: Vec<_> = map.into_iter().collect();
    rows.sort();
    println!("{rows:?}");
}
