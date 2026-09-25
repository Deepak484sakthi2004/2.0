// verify: debug ok
//! Chapter 1.3 deadlocked two threads by locking two accounts in opposite orders.
//! The standard cure: a global lock order (here: by account id). 8 threads, opposite directions, no deadlock.
use std::sync::Mutex;
use std::thread;

struct Account {
    id: u32,
    balance: Mutex<i64>,
}

/// Locks both accounts in id order, whatever the transfer direction.
fn transfer(from: &Account, to: &Account, amount: i64) -> bool {
    if from.id == to.id {
        return false; // locking the same Mutex twice on one thread would deadlock (it isn't reentrant)
    }
    let (first, second) = if from.id < to.id { (from, to) } else { (to, from) };
    let mut a = first.balance.lock().unwrap();
    let mut b = second.balance.lock().unwrap();
    let (src, dst) = if from.id < to.id { (&mut *a, &mut *b) } else { (&mut *b, &mut *a) };
    if *src < amount {
        return false; // check and act under the same two guards: no TOCTOU (Chapter 1.3)
    }
    *src -= amount;
    *dst += amount;
    true
}

fn main() {
    let x = Account { id: 1, balance: Mutex::new(1_000) };
    let y = Account { id: 2, balance: Mutex::new(1_000) };
    let moved = std::sync::atomic::AtomicU32::new(0);
    thread::scope(|s| {
        for t in 0..8 {
            let (x, y, moved) = (&x, &y, &moved);
            s.spawn(move || {
                for _ in 0..10_000 {
                    // Half the threads go x -> y, half y -> x: the pattern that deadlocks without an order.
                    let ok = if t % 2 == 0 { transfer(x, y, 7) } else { transfer(y, x, 7) };
                    if ok {
                        moved.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            });
        }
    });
    let (bx, by) = (*x.balance.lock().unwrap(), *y.balance.lock().unwrap());
    println!("finished without deadlock; transfers succeeded: {}; balances {bx} + {by} = {}", moved.into_inner() > 0, bx + by);
    println!("self-transfer refused: {}", !transfer(&x, &x, 1));
}
