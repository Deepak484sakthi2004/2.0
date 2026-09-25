// verify: debug ok
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
enum CallError {
    Declined,
    RateLimited { retry_after: Duration },
    Unavailable,
    Timeout, // ambiguous: the processor may have charged the card
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Class {
    Rejected,
    Transient,
    Ambiguous,
}

impl CallError {
    fn class(self) -> Class {
        match self {
            CallError::Declined => Class::Rejected,
            CallError::RateLimited { .. } | CallError::Unavailable => Class::Transient,
            CallError::Timeout => Class::Ambiguous,
        }
    }
}

struct Policy {
    max_attempts: u32,
    base: Duration,
    cap: Duration,
    budget: Duration, // the caller's remaining deadline: never sleep past it
}

/// xorshift64: deterministic "randomness" so the output is reproducible.
struct Rng(u64);
impl Rng {
    fn next_f64(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Exponential backoff with full jitter: sleep a random time in [0, min(cap, base * 2^attempt)].
fn backoff(p: &Policy, attempt: u32, rng: &mut Rng) -> Duration {
    let ceiling = p.base.saturating_mul(1 << attempt.min(16)).min(p.cap);
    ceiling.mul_f64(rng.next_f64())
}

/// Retry transient failures; retry ambiguous ones only when the operation is idempotent.
/// Time is simulated: `elapsed` stands in for the clock, so nothing actually sleeps.
fn call_with_retry(
    p: &Policy,
    idempotent: bool,
    mut op: impl FnMut(u32) -> Result<&'static str, CallError>,
) -> (Result<&'static str, CallError>, Vec<String>) {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut elapsed = Duration::ZERO;
    let mut log = Vec::new();
    for attempt in 0..p.max_attempts {
        let err = match op(attempt) {
            Ok(v) => return (Ok(v), log),
            Err(e) => e,
        };
        let retryable = match err.class() {
            Class::Rejected => false,
            Class::Transient => true,
            Class::Ambiguous => idempotent,
        };
        if !retryable || attempt + 1 == p.max_attempts {
            log.push(format!("attempt {attempt}: {err:?} → give up ({:?})", err.class()));
            return (Err(err), log);
        }
        let mut wait = backoff(p, attempt, &mut rng);
        if let CallError::RateLimited { retry_after } = err {
            wait = wait.max(retry_after); // the server told us when: listen
        }
        if elapsed + wait > p.budget {
            log.push(format!("attempt {attempt}: {err:?} → would wait {wait:?}, past the deadline budget: give up"));
            return (Err(err), log);
        }
        elapsed += wait;
        log.push(format!("attempt {attempt}: {err:?} → sleep {:?} (t={:?})", round(wait), round(elapsed)));
    }
    unreachable!("max_attempts is at least 1")
}

fn round(d: Duration) -> Duration {
    Duration::from_millis(d.as_millis() as u64)
}

fn main() {
    let p = Policy { max_attempts: 4, base: Duration::from_millis(50), cap: Duration::from_secs(1), budget: Duration::from_millis(800) };
    let limited = |ms| Err(CallError::RateLimited { retry_after: Duration::from_millis(ms) });
    let scenarios = [
        ("flaky processor", true, vec![Err(CallError::Unavailable), Err(CallError::Unavailable), Ok("ch_123")]),
        ("declined", true, vec![Err(CallError::Declined), Ok("never reached")]),
        ("timeout, no idempotency key", false, vec![Err(CallError::Timeout), Ok("ch_124")]),
        ("timeout, with idempotency key", true, vec![Err(CallError::Timeout), Ok("ch_124")]),
        ("rate limited", true, vec![limited(300), limited(600), Ok("ch_125")]),
    ];
    for (name, idempotent, script) in scenarios {
        let (result, log) = call_with_retry(&p, idempotent, |attempt| script[attempt as usize]);
        println!("{name}: {result:?}");
        for line in log {
            println!("    {line}");
        }
    }
}
