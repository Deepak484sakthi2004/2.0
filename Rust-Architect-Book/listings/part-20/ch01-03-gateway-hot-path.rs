// verify: release ok
// A MODEL of the Meridian gateway's per-request application work, measured on one thread:
// parse the request head, verify an HS256 bearer token, route, rate-limit, build the upstream request head and an
// access-log line. Not modeled: TLS, socket syscalls, the upstream call, the upstream response body.

// --- instrumentation: count heap allocations (the Part III counting allocator) ---
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    static ALLOCS: AtomicUsize = AtomicUsize::new(0);

    pub struct Counting;

    // SAFETY: both methods forward their exact arguments to `System`, which upholds the
    // GlobalAlloc contract; the counter is a plain atomic, so counting never allocates.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static GLOBAL: Counting = Counting;

    pub fn allocs() -> usize {
        ALLOCS.load(Relaxed)
    }
}

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use ring::hmac;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Instant;

#[derive(Deserialize)]
struct Claims<'a> {
    sub: &'a str,
    exp: u64,
}

struct TokenBucket {
    tokens: f64,
    last_ns: u64,
}

impl TokenBucket {
    fn try_acquire(&mut self, now_ns: u64, rate_per_s: f64, burst: f64) -> bool {
        let dt = now_ns.saturating_sub(self.last_ns) as f64 / 1e9;
        self.tokens = (self.tokens + dt * rate_per_s).min(burst);
        self.last_ns = now_ns;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Per-worker state, reused across requests (the fix Chapter 9.2 described for access logs).
struct Worker {
    key: hmac::Key,
    routes: HashMap<(&'static str, &'static str), u32>,
    buckets: HashMap<u64, TokenBucket>,
    json_buf: Vec<u8>,
    sig_buf: Vec<u8>,
    upstream_head: String,
    log: String,
}

#[derive(Debug, PartialEq)]
enum Outcome {
    Forward { route: u32 },
    Unauthorized,
    NotFound,
    Limited,
    BadRequest,
}

impl Worker {
    fn handle(&mut self, raw: &[u8], now_ns: u64) -> Outcome {
        let mut headers = [httparse::EMPTY_HEADER; 32];
        let mut req = httparse::Request::new(&mut headers);
        let Ok(httparse::Status::Complete(_)) = req.parse(raw) else { return Outcome::BadRequest };
        let (method, path) = (req.method.unwrap_or(""), req.path.unwrap_or(""));

        // 1. Authenticate: "Authorization: Bearer <header>.<payload>.<signature>", HMAC-SHA256.
        let Some(auth) = req.headers.iter().find(|h| h.name.eq_ignore_ascii_case("authorization")) else {
            return Outcome::Unauthorized;
        };
        let Some(token) = auth.value.strip_prefix(b"Bearer ") else { return Outcome::Unauthorized };
        let Some(dot2) = token.iter().rposition(|&b| b == b'.') else { return Outcome::Unauthorized };
        let (signed, sig_b64) = (&token[..dot2], &token[dot2 + 1..]);
        self.sig_buf.clear();
        if B64.decode_vec(sig_b64, &mut self.sig_buf).is_err() || hmac::verify(&self.key, signed, &self.sig_buf).is_err() {
            return Outcome::Unauthorized;
        }
        let Some(dot1) = signed.iter().position(|&b| b == b'.') else { return Outcome::Unauthorized };
        self.json_buf.clear();
        if B64.decode_vec(&signed[dot1 + 1..], &mut self.json_buf).is_err() {
            return Outcome::Unauthorized;
        }
        let Ok(claims) = serde_json::from_slice::<Claims>(&self.json_buf) else { return Outcome::Unauthorized };
        if claims.exp.saturating_mul(1_000_000_000) < now_ns {
            return Outcome::Unauthorized;
        }

        // 2. Route on method + first two path segments ("/v1/payments/pay_123/refunds" -> "/v1/payments").
        let prefix_end = path.match_indices('/').nth(2).map_or(path.len(), |(i, _)| i);
        let Some(&route) = self.routes.get(&(method, &path[..prefix_end])) else { return Outcome::NotFound };

        // 3. Rate-limit per API key (the subject), 500 req/s with a burst of 1000.
        let api_key = fxhash::hash64(claims.sub);
        let bucket = self.buckets.entry(api_key).or_insert(TokenBucket { tokens: 1000.0, last_ns: now_ns });
        if !bucket.try_acquire(now_ns, 500.0, 1000.0) {
            return Outcome::Limited;
        }

        // 4. Upstream request head and one access-log line, written into reused buffers.
        self.upstream_head.clear();
        let _ = write!(
            self.upstream_head,
            "{method} {path} HTTP/1.1\r\nx-meridian-route: {route}\r\nx-meridian-sub: {}\r\n",
            claims.sub
        );
        self.log.clear();
        let _ = write!(self.log, "{now_ns} {method} {path} route={route} sub={} status=200", claims.sub);
        Outcome::Forward { route }
    }
}

fn median_ns(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn main() {
    let key = hmac::Key::new(hmac::HMAC_SHA256, b"meridian-demo-signing-key-32-bytes!!");
    let header = B64.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
    let payload = B64.encode(br#"{"sub":"merchant_48213","exp":4102444800,"scope":"payments:write refunds:write"}"#);
    let signed = format!("{header}.{payload}");
    let sig = B64.encode(hmac::sign(&key, signed.as_bytes()).as_ref());
    let token = format!("{signed}.{sig}");
    // 13 headers: Chapter 9.1's "11-14 headers typical".
    let raw = format!(
        "POST /v1/payments/pay_123/refunds?limit=20 HTTP/1.1\r\nHost: api.meridian.example\r\n\
         Authorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: 64\r\n\
         Accept: application/json\r\nUser-Agent: partner-sdk/4.2.1\r\nX-Request-Id: 7f9c2ba4e88f827d\r\n\
         Idempotency-Key: 3b241101-e2bb-4255-8caf-4136c566a962\r\nAccept-Encoding: gzip\r\n\
         X-Forwarded-For: 203.0.113.7\r\nTraceparent: 00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01\r\n\
         Connection: keep-alive\r\n\r\n"
    );

    // ~1,000 routes, as in the gateway config of Chapter 3.2.
    let mut routes = HashMap::new();
    let mut id = 0;
    for m in ["GET", "POST", "PUT", "DELETE"] {
        for i in 0..250 {
            let p: &'static str = if i == 0 { "/v1/payments" } else { Box::leak(format!("/v1/svc{i}").into_boxed_str()) };
            routes.insert((m, p), id);
            id += 1;
        }
    }
    let mut w = Worker {
        key,
        routes,
        buckets: HashMap::new(),
        json_buf: Vec::with_capacity(256),
        sig_buf: Vec::with_capacity(64),
        upstream_head: String::with_capacity(512),
        log: String::with_capacity(512),
    };

    let mut now = 1_700_000_000_000_000_000u64;
    for _ in 0..10_000 {
        now += 2_000_000; // 2 ms apart: under 500 req/s, so every request is forwarded
        black_box(w.handle(black_box(raw.as_bytes()), now));
    }
    now += 2_000_000;
    println!("outcome: {:?}; request head {} bytes", w.handle(raw.as_bytes(), now), raw.len());

    let (samples, per) = (200usize, 1_000u64);
    let mut per_req: Vec<f64> = Vec::with_capacity(samples); // allocated before counting starts
    let a0 = counting::allocs();
    for _ in 0..samples {
        let t = Instant::now();
        for _ in 0..per {
            now += 2_000_000;
            black_box(w.handle(black_box(raw.as_bytes()), now));
        }
        per_req.push(t.elapsed().as_nanos() as f64 / per as f64);
    }
    let allocs = counting::allocs() - a0;
    let mut sorted = per_req.clone();
    sorted.sort_by(f64::total_cmp);
    println!(
        "per request ({samples} samples x {per}): min {:.2} us, median {:.2} us, p99 of samples {:.2} us",
        sorted[0] / 1e3,
        sorted[samples / 2] / 1e3,
        sorted[samples * 99 / 100] / 1e3
    );
    println!("heap allocations in {} requests after warm-up: {allocs}", samples as u64 * per);

    // Where does the time go? Time the two biggest pieces on their own.
    let hmac_ns = median_ns(
        (0..100)
            .map(|_| {
                let t = Instant::now();
                for _ in 0..1_000 {
                    black_box(hmac::verify(&w.key, black_box(signed.as_bytes()), black_box(&w.sig_buf)).is_ok());
                }
                t.elapsed().as_nanos() as f64 / 1_000.0
            })
            .collect(),
    );
    let parse_ns = median_ns(
        (0..100)
            .map(|_| {
                let t = Instant::now();
                for _ in 0..1_000 {
                    let mut headers = [httparse::EMPTY_HEADER; 32];
                    let mut req = httparse::Request::new(&mut headers);
                    black_box(req.parse(black_box(raw.as_bytes())).is_ok());
                }
                t.elapsed().as_nanos() as f64 / 1_000.0
            })
            .collect(),
    );
    println!("  of which HMAC-SHA256 verify ({} bytes): median {:.2} us", signed.len(), hmac_ns / 1e3);
    println!("  of which httparse of the head:           median {:.2} us", parse_ns / 1e3);

    let cores = |cpu_us: f64| 400_000.0 * cpu_us / 1e6 / 0.6;
    println!("cores for 400K req/s at 60% utilization, this model only: {:.1}", cores(sorted[samples / 2] / 1e3));
    println!("same arithmetic with the Java gateway's ~500 us CPU per request: {:.0}", cores(500.0));
}
