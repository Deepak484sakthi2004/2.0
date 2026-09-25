// verify: release ok
//! Concurrent map options under one workload: 4 threads, 10,000 keys, 95% reads / 5% writes.
//! (The Part IX 9.3 promise.) Wall time per operation per thread. One run, noisy.
use std::collections::HashMap;
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Mutex, RwLock};
use std::thread;
use std::time::Instant;

const KEYS: u64 = 10_000;
const THREADS: u64 = 4;
const SHARDS: usize = 16;

/// A deterministic op stream per thread: (key, is_write).
fn ops(thread: u64, n: u64) -> impl Iterator<Item = (u64, bool)> {
    let mut x = 0x9E37_79B9_7F4A_7C15u64 ^ (thread + 1).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    (0..n).map(move |_| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x % KEYS, x % 100 < 5)
    })
}

fn bench(label: &str, n: u64, op: impl Fn(u64, bool) + Sync) {
    let t = Instant::now();
    thread::scope(|s| {
        for th in 0..THREADS {
            let op = &op;
            s.spawn(move || ops(th, n).for_each(|(k, w)| op(k, w)));
        }
    });
    println!("{label:<44} {:>8.1} ns per op", t.elapsed().as_nanos() as f64 / n as f64);
}

fn seed() -> HashMap<u64, u64> {
    (0..KEYS).map(|k| (k, 0)).collect()
}

fn main() {
    const N: u64 = 1_000_000;

    let m = Mutex::new(seed());
    bench("Mutex<HashMap>", N, |k, w| {
        let mut g = m.lock().unwrap();
        if w { *g.get_mut(&k).unwrap() += 1 } else { std::hint::black_box(g.get(&k)); }
    });

    let rw = RwLock::new(seed());
    bench("RwLock<HashMap>", N, |k, w| {
        if w { *rw.write().unwrap().get_mut(&k).unwrap() += 1 } else { std::hint::black_box(rw.read().unwrap().get(&k).copied()); }
    });

    let shards_m: Vec<Mutex<HashMap<u64, u64>>> = (0..SHARDS)
        .map(|i| Mutex::new(seed().into_iter().filter(|(k, _)| *k as usize % SHARDS == i).collect()))
        .collect();
    bench("16 shards x Mutex<HashMap>", N, |k, w| {
        let mut g = shards_m[k as usize % SHARDS].lock().unwrap();
        if w { *g.get_mut(&k).unwrap() += 1 } else { std::hint::black_box(g.get(&k)); }
    });

    let shards_rw: Vec<RwLock<HashMap<u64, u64>>> = (0..SHARDS)
        .map(|i| RwLock::new(seed().into_iter().filter(|(k, _)| *k as usize % SHARDS == i).collect()))
        .collect();
    bench("16 shards x RwLock<HashMap>", N, |k, w| {
        let shard = &shards_rw[k as usize % SHARDS];
        if w { *shard.write().unwrap().get_mut(&k).unwrap() += 1 } else { std::hint::black_box(shard.read().unwrap().get(&k).copied()); }
    });

    // Owner thread: every op is a message; reads need a reply (a round trip), writes are fire-and-forget.
    enum Op {
        Get(u64, SyncSender<Option<u64>>),
        Add(u64),
    }
    let (tx, rx) = mpsc::sync_channel::<Op>(1024);
    let owner = thread::spawn(move || {
        let mut map = seed();
        for op in rx {
            match op {
                Op::Get(k, reply) => drop(reply.send(map.get(&k).copied())),
                Op::Add(k) => *map.get_mut(&k).unwrap() += 1,
            }
        }
        map.values().sum::<u64>()
    });
    const N_OWNER: u64 = 50_000; // round trips are slow: fewer ops, same per-op metric
    bench("owner thread + channels (reads round-trip)", N_OWNER, |k, w| {
        if w {
            tx.send(Op::Add(k)).unwrap();
        } else {
            let (reply, answer) = mpsc::sync_channel(1);
            tx.send(Op::Get(k, reply)).unwrap();
            std::hint::black_box(answer.recv().unwrap());
        }
    });
    drop(tx);
    let owner_writes = owner.join().unwrap();

    let single = m.into_inner().unwrap().values().sum::<u64>();
    let sharded = shards_m.into_iter().map(|s| s.into_inner().unwrap().values().sum::<u64>()).sum::<u64>();
    println!(
        "writes applied: Mutex<HashMap> {single}, 16 x Mutex shards {sharded} (same op streams), owner {owner_writes} (shorter streams)"
    );
}
