// verify: debug ok
/// A limiter that rejects must tell the client when to retry.
trait RateLimiter {
    fn try_acquire(&mut self) -> bool;

    /// DEFAULT: "retry immediately". Convenient, and wrong for most implementations.
    fn retry_after_ms(&self) -> u64 {
        0
    }
}

struct PerKeyWindow {
    used: u32,
    limit: u32,
    ms_until_reset: u64,
}

impl RateLimiter for PerKeyWindow {
    fn try_acquire(&mut self) -> bool {
        self.used += 1;
        self.used <= self.limit
    }
    fn retry_after_ms(&self) -> u64 {
        self.ms_until_reset
    }
}

/// Added months later by another team. It compiles: the default fills the gap silently.
struct GlobalConcurrency {
    in_flight: u32,
    max: u32,
}

impl RateLimiter for GlobalConcurrency {
    fn try_acquire(&mut self) -> bool {
        if self.in_flight < self.max {
            self.in_flight += 1;
            true
        } else {
            false
        }
    }
    // forgot retry_after_ms: rejected clients are told to retry after 0 ms
}

fn reject_header(l: &dyn RateLimiter) -> String {
    format!("429 Too Many Requests; Retry-After-Ms: {}", l.retry_after_ms())
}

fn main() {
    let mut per_key = PerKeyWindow { used: 0, limit: 1, ms_until_reset: 750 };
    let mut global = GlobalConcurrency { in_flight: 1, max: 1 };
    for limiter in [&mut per_key as &mut dyn RateLimiter, &mut global] {
        limiter.try_acquire();
        if !limiter.try_acquire() {
            println!("{}", reject_header(limiter));
        }
    }
}
