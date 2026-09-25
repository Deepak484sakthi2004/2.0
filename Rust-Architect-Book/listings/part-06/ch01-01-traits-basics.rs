// verify: debug ok
/// A rate-limiting algorithm. Implementors provide `try_acquire`; everything else has a default.
trait RateLimiter {
    /// Try to admit one request at time `now_ms`. Returns true if admitted.
    fn try_acquire(&mut self, now_ms: u64) -> bool;

    /// A human-readable name for logs. Default: a generic label.
    fn name(&self) -> &str {
        "limiter"
    }

    /// Admit up to `n` requests; a DEFAULT METHOD built on the required one (template method).
    fn admit_batch(&mut self, n: u32, now_ms: u64) -> u32 {
        (0..n).filter(|_| self.try_acquire(now_ms)).count() as u32
    }
}

/// Token bucket: `capacity` tokens, refilled at `per_sec` tokens per second.
struct TokenBucket {
    capacity: f64,
    tokens: f64,
    per_sec: f64,
    last_ms: u64,
}

impl RateLimiter for TokenBucket {
    fn try_acquire(&mut self, now_ms: u64) -> bool {
        let elapsed = (now_ms - self.last_ms) as f64 / 1000.0;
        self.tokens = (self.tokens + elapsed * self.per_sec).min(self.capacity);
        self.last_ms = now_ms;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn name(&self) -> &str {
        "token-bucket" // overrides the default
    }
}

/// Fixed window: at most `limit` requests per `window_ms`.
struct FixedWindow {
    limit: u32,
    window_ms: u64,
    window_start: u64,
    used: u32,
}

impl RateLimiter for FixedWindow {
    fn try_acquire(&mut self, now_ms: u64) -> bool {
        if now_ms - self.window_start >= self.window_ms {
            self.window_start = now_ms;
            self.used = 0;
        }
        if self.used < self.limit {
            self.used += 1;
            true
        } else {
            false
        }
    }
    // `name` and `admit_batch` use the defaults
}

/// A trait bound: works for ANY RateLimiter, resolved at compile time.
fn simulate<L: RateLimiter>(limiter: &mut L, requests_per_tick: u32) -> Vec<u32> {
    (0..5).map(|tick| limiter.admit_batch(requests_per_tick, tick * 250)).collect()
}

/// The same bound written with `impl Trait` in argument position.
fn describe(limiter: &impl RateLimiter) -> String {
    format!("[{}]", limiter.name())
}

fn main() {
    let mut bucket = TokenBucket { capacity: 4.0, tokens: 4.0, per_sec: 4.0, last_ms: 0 };
    let mut window = FixedWindow { limit: 4, window_ms: 1000, window_start: 0, used: 0 };
    println!("{} admitted per 250 ms tick: {:?}", describe(&bucket), simulate(&mut bucket, 3));
    println!("{} admitted per 250 ms tick: {:?}", describe(&window), simulate(&mut window, 3));
}
