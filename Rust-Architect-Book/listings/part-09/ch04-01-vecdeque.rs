// verify: debug ok
use std::collections::VecDeque;

/// Sliding-window rate limiter: at most `limit` events in any `window_ms`.
struct SlidingWindow {
    window_ms: u64,
    limit: usize,
    events: VecDeque<u64>, // timestamps, oldest at the front
}

impl SlidingWindow {
    fn new(window_ms: u64, limit: usize) -> Self {
        SlidingWindow { window_ms, limit, events: VecDeque::with_capacity(limit) }
    }
    fn allow(&mut self, now_ms: u64) -> bool {
        while let Some(&oldest) = self.events.front() {
            if now_ms - oldest >= self.window_ms {
                self.events.pop_front();
            } else {
                break;
            }
        }
        if self.events.len() < self.limit {
            self.events.push_back(now_ms);
            true
        } else {
            false
        }
    }
}

fn main() {
    let mut q: VecDeque<u32> = VecDeque::with_capacity(8);
    for i in 1..=6 {
        q.push_back(i);
    }
    q.pop_front();
    q.pop_front();
    for i in 7..=9 {
        q.push_back(i);
    }
    println!("capacity {}, len {}", q.capacity(), q.len());
    let (front, back) = q.as_slices();
    println!("as_slices: {front:?} + {back:?}   <- the ring buffer wrapped");
    q.make_contiguous();
    let (front, back) = q.as_slices();
    println!("after make_contiguous: {front:?} + {back:?}");

    let mut rl = SlidingWindow::new(1000, 3);
    let decisions: Vec<(u64, bool)> = [0, 100, 200, 300, 999, 1000, 1150, 1250].iter().map(|&t| (t, rl.allow(t))).collect();
    println!("rate limiter (3 per 1000 ms): {decisions:?}");
    println!("limiter buffer capacity stayed {}", rl.events.capacity());
}
