// verify: debug test
// verify: release ok
//! Parallel logstat (Project L1, review question 7): split the input at newline boundaries, build one
//! Summary per thread with NO shared state, then merge. Histograms and counters merge exactly.
use rayon::prelude::*;
use std::thread;
use std::time::Instant;

mod parse {
    /// One parsed log line, borrowed from the input (as in Project L1).
    #[derive(Debug, PartialEq)]
    pub struct Record<'a> {
        pub path: &'a str,
        pub status: u16,
        pub latency_ms: u32,
    }

    pub fn line(line: &str) -> Option<Record<'_>> {
        let mut f = line.split_ascii_whitespace();
        let (Some(_ts), Some(_method), Some(path), Some(status), Some(latency), None) =
            (f.next(), f.next(), f.next(), f.next(), f.next(), f.next())
        else {
            return None;
        };
        let status: u16 = status.parse().ok().filter(|s| (100..=599).contains(s))?;
        Some(Record { path, status, latency_ms: latency.parse().ok()? })
    }
}

mod stats {
    use crate::parse::Record;
    use std::collections::HashMap;

    pub const MAX_TRACKED_MS: u32 = 10_000;

    #[derive(Debug, PartialEq)]
    pub struct Summary {
        pub lines: u64,
        pub malformed: u64,
        pub invalid_utf8: u64,
        pub by_class: [u64; 5],
        latency_buckets: Vec<u64>,
        latency_count: u64,
        path_hits: HashMap<String, u64>,
    }

    impl Summary {
        pub fn new() -> Summary {
            Summary {
                lines: 0,
                malformed: 0,
                invalid_utf8: 0,
                by_class: [0; 5],
                latency_buckets: vec![0; MAX_TRACKED_MS as usize + 2],
                latency_count: 0,
                path_hits: HashMap::new(),
            }
        }

        pub fn record(&mut self, r: &Record) {
            self.by_class[(r.status / 100 - 1) as usize] += 1;
            self.latency_buckets[r.latency_ms.min(MAX_TRACKED_MS + 1) as usize] += 1;
            self.latency_count += 1;
            if let Some(hits) = self.path_hits.get_mut(r.path) {
                *hits += 1;
            } else {
                self.path_hits.insert(r.path.to_string(), 1);
            }
        }

        /// Exact merge: every field is a sum (element-wise for arrays, per key for the map).
        pub fn merge(mut self, mut other: Summary) -> Summary {
            if other.path_hits.len() > self.path_hits.len() {
                std::mem::swap(&mut self.path_hits, &mut other.path_hits); // fold the smaller map into the larger
            }
            self.lines += other.lines;
            self.malformed += other.malformed;
            self.invalid_utf8 += other.invalid_utf8;
            for (a, b) in self.by_class.iter_mut().zip(other.by_class) {
                *a += b;
            }
            for (a, b) in self.latency_buckets.iter_mut().zip(&other.latency_buckets) {
                *a += b;
            }
            self.latency_count += other.latency_count;
            for (path, hits) in other.path_hits {
                *self.path_hits.entry(path).or_insert(0) += hits;
            }
            self
        }

        pub fn percentile(&self, p: f64) -> Option<u32> {
            if self.latency_count == 0 {
                return None;
            }
            let rank = ((p / 100.0) * self.latency_count as f64).ceil().max(1.0) as u64;
            let mut seen = 0;
            for (ms, &count) in self.latency_buckets.iter().enumerate() {
                seen += count;
                if seen >= rank {
                    return Some(ms as u32);
                }
            }
            unreachable!()
        }

        pub fn top_paths(&self, n: usize) -> Vec<(&str, u64)> {
            let mut all: Vec<(&str, u64)> = self.path_hits.iter().map(|(p, &c)| (p.as_str(), c)).collect();
            all.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            all.truncate(n);
            all
        }
    }
}

use stats::Summary;

/// One chunk, one Summary. Bad data is counted, never fatal (as in L1).
fn ingest(chunk: &[u8]) -> Summary {
    let mut s = Summary::new();
    for raw in chunk.split(|&b| b == b'\n') {
        let Ok(text) = std::str::from_utf8(raw) else {
            s.lines += 1;
            s.invalid_utf8 += 1;
            continue;
        };
        let text = text.trim_end_matches('\r');
        if text.trim().is_empty() {
            continue;
        }
        s.lines += 1;
        match parse::line(text) {
            Some(r) => s.record(&r),
            None => s.malformed += 1,
        }
    }
    s
}

/// Up to `parts` chunks, each ending just after a b'\n' (or at the end). A newline byte never occurs inside
/// a multi-byte UTF-8 sequence, so no line, and no character, is ever cut in two.
fn split_lines(input: &[u8], parts: usize) -> Vec<&[u8]> {
    let target = input.len().div_ceil(parts.max(1)).max(1);
    let mut chunks = Vec::with_capacity(parts);
    let mut rest = input;
    while !rest.is_empty() {
        let cut = if rest.len() <= target {
            rest.len()
        } else {
            match rest[target..].iter().position(|&b| b == b'\n') {
                Some(i) => target + i + 1,
                None => rest.len(),
            }
        };
        let (head, tail) = rest.split_at(cut);
        chunks.push(head);
        rest = tail;
    }
    chunks
}

fn parallel_scoped(input: &[u8], threads: usize) -> Summary {
    thread::scope(|s| {
        let handles: Vec<_> = split_lines(input, threads).into_iter().map(|c| s.spawn(move || ingest(c))).collect();
        handles.into_iter().map(|h| h.join().unwrap()).fold(Summary::new(), Summary::merge)
    })
}

fn parallel_rayon(input: &[u8]) -> Summary {
    // More chunks than threads: work stealing evens out chunks that happen to be slower.
    split_lines(input, rayon::current_num_threads() * 8).into_par_iter().map(ingest).reduce(Summary::new, Summary::merge)
}

/// A deterministic synthetic access log: 1 in 5,000 lines not UTF-8, 4 in 5,000 malformed.
fn synthetic_log(lines: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(lines * 48);
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    for i in 0..lines {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        if i % 5_000 == 4_999 {
            out.extend_from_slice(b"2026-09-25T10:00:00Z GET /caf\xFF 200 12\n");
        } else if i % 1_000 == 999 {
            out.extend_from_slice(b"garbage line\n");
        } else {
            let status = [200, 200, 200, 201, 204, 301, 404, 429, 500, 503][(x % 10) as usize];
            let latency = (x >> 8) % 400 + if x % 97 == 0 { 3_000 } else { 1 };
            let path = (x >> 20) % 50;
            out.extend_from_slice(format!("2026-09-25T10:00:00Z GET /api/items/{path} {status} {latency}\n").as_bytes());
        }
    }
    out
}

fn main() {
    let input = synthetic_log(1_000_000);
    let threads = thread::available_parallelism().unwrap().get();

    let t = Instant::now();
    let seq = ingest(&input);
    let t_seq = t.elapsed();

    let t = Instant::now();
    let scoped = parallel_scoped(&input, threads);
    let t_scoped = t.elapsed();

    let t = Instant::now();
    let ray = parallel_rayon(&input);
    let t_rayon = t.elapsed();

    assert!(seq == scoped && seq == ray, "a merged summary must equal the sequential one exactly");
    println!("identical summaries from 3 strategies: true");
    println!("lines: {} ({} malformed, {} invalid UTF-8)", seq.lines, seq.malformed, seq.invalid_utf8);
    let [c1, c2, c3, c4, c5] = seq.by_class;
    println!("status: 1xx={c1} 2xx={c2} 3xx={c3} 4xx={c4} 5xx={c5}");
    println!("p50={:?} p99={:?} ms; top 3 paths {:?}", seq.percentile(50.0), seq.percentile(99.0), seq.top_paths(3));
    println!("input {} MB; sequential {t_seq:.1?}, scoped x{threads} {t_scoped:.1?}, rayon {t_rayon:.1?} (one run)", input.len() / 1_000_000);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_reassemble_and_end_at_newlines() {
        let input = synthetic_log(10_003);
        for parts in [1, 2, 3, 7, 64] {
            let chunks = split_lines(&input, parts);
            assert_eq!(chunks.concat(), input);
            for c in &chunks[..chunks.len() - 1] {
                assert_eq!(c.last(), Some(&b'\n'));
            }
        }
        assert!(split_lines(b"", 4).is_empty());
        assert_eq!(split_lines(b"no newline at all", 4), vec![&b"no newline at all"[..]]);
    }

    #[test]
    fn merge_is_exact() {
        let input = synthetic_log(20_000);
        let whole = ingest(&input);
        let parts = split_lines(&input, 5);
        let merged = parts.iter().map(|c| ingest(c)).fold(Summary::new(), Summary::merge);
        assert!(merged == whole);
        assert!(parallel_scoped(&input, 3) == whole);
        assert!(parallel_rayon(&input) == whole);
    }
}
