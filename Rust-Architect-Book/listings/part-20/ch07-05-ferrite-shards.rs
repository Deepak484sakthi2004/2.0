// verify: release ok
// Project L4's open question: Mutex or RwLock shards for Ferrite, and does the value type matter?
// 16 shards, 4,096 keys, 4 KiB values, 4 threads, 95% GET / 5% SET. Ferrite v1's `get` returns an owned copy
// (Vec<u8>), so a GET copies 4 KiB while holding the shard lock. Three designs:
//   Mutex<HashMap<_, Vec<u8>>>    GET copies under an exclusive lock
//   RwLock<HashMap<_, Vec<u8>>>   GET copies under a shared lock
//   RwLock<HashMap<_, Arc<[u8]>>> GET clones an Arc under the lock (the v2 decision: share immutable values)
// One Playground run, noisy.
use std::collections::HashMap;
use std::hint::black_box;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Instant;

const SHARDS: usize = 16;
const KEYS: u64 = 4096;
const VALUE: usize = 4096;
const OPS: u64 = 200_000;

trait Store: Send + Sync {
    fn get(&self, k: u64) -> usize; // returns the length of what it got (so the copy can't be optimized away)
    fn put(&self, k: u64, v: &[u8]);
}

struct MutexVec(Vec<Mutex<HashMap<u64, Vec<u8>>>>);
struct RwVec(Vec<RwLock<HashMap<u64, Vec<u8>>>>);
struct RwArc(Vec<RwLock<HashMap<u64, Arc<[u8]>>>>);

fn shard(k: u64) -> usize {
    (k.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 60) as usize % SHARDS
}

impl Store for MutexVec {
    fn get(&self, k: u64) -> usize {
        let v: Option<Vec<u8>> = self.0[shard(k)].lock().unwrap().get(&k).cloned(); // copy under the lock
        black_box(v).map_or(0, |v| v.len())
    }
    fn put(&self, k: u64, v: &[u8]) {
        self.0[shard(k)].lock().unwrap().insert(k, v.to_vec());
    }
}
impl Store for RwVec {
    fn get(&self, k: u64) -> usize {
        let v: Option<Vec<u8>> = self.0[shard(k)].read().unwrap().get(&k).cloned();
        black_box(v).map_or(0, |v| v.len())
    }
    fn put(&self, k: u64, v: &[u8]) {
        self.0[shard(k)].write().unwrap().insert(k, v.to_vec());
    }
}
impl Store for RwArc {
    fn get(&self, k: u64) -> usize {
        let v: Option<Arc<[u8]>> = self.0[shard(k)].read().unwrap().get(&k).cloned(); // refcount +1 under the lock
        black_box(v).map_or(0, |v| v.len())
    }
    fn put(&self, k: u64, v: &[u8]) {
        let v: Arc<[u8]> = Arc::from(v); // allocate and copy outside the lock
        self.0[shard(k)].write().unwrap().insert(k, v);
    }
}

fn bench(store: Arc<dyn Store>, threads: usize) -> f64 {
    let t = Instant::now();
    let hs: Vec<_> = (0..threads)
        .map(|t| {
            let s = store.clone();
            thread::spawn(move || {
                let val = vec![t as u8; VALUE];
                let mut x = 0x2545_F491_4F6C_DD1Du64 ^ t as u64;
                let mut got = 0usize;
                for _ in 0..OPS {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    let k = x % KEYS;
                    if x % 100 < 5 { s.put(k, &val) } else { got += s.get(k) }
                }
                black_box(got);
            })
        })
        .collect();
    for h in hs {
        h.join().unwrap();
    }
    (threads as u64 * OPS) as f64 / t.elapsed().as_secs_f64() / 1e6
}

fn main() {
    let fill = |s: &dyn Store| {
        for k in 0..KEYS {
            s.put(k, &[1u8; VALUE]);
        }
    };
    let m = Arc::new(MutexVec((0..SHARDS).map(|_| Mutex::new(HashMap::new())).collect()));
    let r = Arc::new(RwVec((0..SHARDS).map(|_| RwLock::new(HashMap::new())).collect()));
    let a = Arc::new(RwArc((0..SHARDS).map(|_| RwLock::new(HashMap::new())).collect()));
    fill(&*m);
    fill(&*r);
    fill(&*a);
    println!("M ops/s, 95% GET of 4 KiB values:");
    for threads in [1usize, 4] {
        println!(
            "  {threads} thread(s):  Mutex+Vec {:6.2}   RwLock+Vec {:6.2}   RwLock+Arc<[u8]> {:6.2}",
            bench(m.clone(), threads),
            bench(r.clone(), threads),
            bench(a.clone(), threads)
        );
    }
}
