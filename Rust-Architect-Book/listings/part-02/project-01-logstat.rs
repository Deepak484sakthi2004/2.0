// verify: debug ok
// verify: debug test
// verify: release ok
//! logstat: summarize HTTP access logs.
//!
//! Input: one request per line, whitespace-separated:
//!     <timestamp> <method> <path> <status> <latency_ms>
//!     2026-09-24T10:00:01Z GET /api/orders 200 37
//!
//! Usage: logstat [--top N] [FILE ...]     (reads stdin when no FILE is given; "-" also means stdin)

use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::process::ExitCode;

const USAGE: &str = "usage: logstat [--top N] [FILE ...]\n\
                     Summarizes access logs: status classes, latency percentiles, busiest paths.\n\
                     Reads standard input when no FILE is given.\n";

mod args {
    #[derive(Debug, PartialEq)]
    pub struct Options {
        pub top: usize,
        pub files: Vec<String>,
    }

    #[derive(Debug, PartialEq)]
    pub enum Command {
        Run(Options),
        Help,
    }

    pub fn parse(argv: &[String]) -> Result<Command, String> {
        let mut opts = Options { top: 5, files: Vec::new() };
        let mut i = 0;
        while i < argv.len() {
            match argv[i].as_str() {
                "-h" | "--help" => return Ok(Command::Help),
                "--top" => {
                    let Some(value) = argv.get(i + 1) else {
                        return Err("--top needs a value".to_string());
                    };
                    opts.top = value
                        .parse()
                        .map_err(|_| format!("--top expects a number, got {value:?}"))?;
                    i += 1;
                }
                flag if flag.starts_with('-') && flag != "-" => {
                    return Err(format!("unknown option {flag:?}"));
                }
                file => opts.files.push(file.to_string()),
            }
            i += 1;
        }
        Ok(Command::Run(opts))
    }
}

mod parse {
    /// One parsed log line. It borrows from the line: parsing allocates nothing.
    #[derive(Debug, PartialEq)]
    pub struct Record<'a> {
        pub path: &'a str,
        pub status: u16,
        pub latency_ms: u32,
    }

    #[derive(Debug, PartialEq)]
    pub enum LineError {
        FieldCount,
        Status,
        Latency,
    }

    pub fn line(line: &str) -> Result<Record<'_>, LineError> {
        let mut fields = line.split_ascii_whitespace();
        let mut next = || fields.next();
        // Exactly five fields: the sixth `next()` must be None.
        let (Some(_ts), Some(_method), Some(path), Some(status), Some(latency), None) =
            (next(), next(), next(), next(), next(), next())
        else {
            return Err(LineError::FieldCount);
        };
        let status: u16 = status.parse().map_err(|_| LineError::Status)?;
        if !(100..=599).contains(&status) {
            return Err(LineError::Status);
        }
        let latency_ms = latency.parse().map_err(|_| LineError::Latency)?;
        Ok(Record { path, status, latency_ms })
    }
}

mod stats {
    use crate::parse::Record;
    use std::collections::HashMap;

    /// Latencies are counted in 1 ms buckets up to this bound; slower requests share one overflow bucket.
    pub const MAX_TRACKED_MS: u32 = 10_000;

    pub struct Summary {
        pub lines: u64,
        pub malformed: u64,
        pub invalid_utf8: u64,
        pub by_class: [u64; 5], // 1xx, 2xx, 3xx, 4xx, 5xx
        latency_buckets: Vec<u64>, // index = ms; the last bucket = "over MAX_TRACKED_MS"
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
            let bucket = r.latency_ms.min(MAX_TRACKED_MS + 1) as usize;
            self.latency_buckets[bucket] += 1;
            self.latency_count += 1;
            // Allocate a key only the first time a path is seen.
            if let Some(hits) = self.path_hits.get_mut(r.path) {
                *hits += 1;
            } else {
                self.path_hits.insert(r.path.to_string(), 1);
            }
        }

        /// Nearest-rank percentile in ms (MAX_TRACKED_MS + 1 means "slower than tracked").
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
            unreachable!("rank <= latency_count, and the buckets sum to latency_count")
        }

        /// The `n` busiest paths, most hits first; ties broken alphabetically for stable output.
        pub fn top_paths(&self, n: usize) -> Vec<(&str, u64)> {
            let mut all: Vec<(&str, u64)> = self.path_hits.iter().map(|(p, &c)| (p.as_str(), c)).collect();
            all.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            all.truncate(n);
            all
        }
    }
}

mod report {
    use crate::stats::{MAX_TRACKED_MS, Summary};
    use std::io::{self, Write};

    pub fn write(out: &mut impl Write, s: &Summary, top: usize) -> io::Result<()> {
        writeln!(out, "lines:      {} ({} malformed, {} invalid UTF-8)", s.lines, s.malformed, s.invalid_utf8)?;
        let [c1, c2, c3, c4, c5] = s.by_class;
        writeln!(out, "status:     1xx={c1} 2xx={c2} 3xx={c3} 4xx={c4} 5xx={c5}")?;
        for (label, p) in [("p50", 50.0), ("p95", 95.0), ("p99", 99.0)] {
            match s.percentile(p) {
                None => writeln!(out, "{label}:        n/a")?,
                Some(ms) if ms > MAX_TRACKED_MS => writeln!(out, "{label}:        >{MAX_TRACKED_MS} ms")?,
                Some(ms) => writeln!(out, "{label}:        {ms} ms")?,
            }
        }
        writeln!(out, "top paths:")?;
        for (path, hits) in s.top_paths(top) {
            writeln!(out, "  {hits:>8}  {path}")?;
        }
        Ok(())
    }
}

/// Feeds every line of `input` into `summary`. Bad data is counted, never fatal; only I/O errors fail.
fn ingest(mut input: impl BufRead, summary: &mut stats::Summary) -> io::Result<()> {
    let mut buf = Vec::with_capacity(256); // reused for every line: no allocation per line
    loop {
        buf.clear();
        if input.read_until(b'\n', &mut buf)? == 0 {
            return Ok(()); // EOF
        }
        let raw = buf.strip_suffix(b"\n").unwrap_or(&buf);
        let Ok(text) = std::str::from_utf8(raw) else {
            summary.lines += 1;
            summary.invalid_utf8 += 1;
            continue;
        };
        let text = text.trim_end_matches('\r');
        if text.trim().is_empty() {
            continue; // blank lines are not records
        }
        summary.lines += 1;
        match parse::line(text) {
            Ok(record) => summary.record(&record),
            Err(_) => summary.malformed += 1,
        }
    }
}

fn run(opts: &args::Options) -> io::Result<stats::Summary> {
    let mut summary = stats::Summary::new();
    if opts.files.is_empty() {
        ingest(io::stdin().lock(), &mut summary)?;
    }
    for path in &opts.files {
        if path == "-" {
            ingest(io::stdin().lock(), &mut summary)?;
        } else {
            let file = File::open(path).map_err(|e| io::Error::new(e.kind(), format!("{path}: {e}")))?;
            ingest(BufReader::new(file), &mut summary)?;
        }
    }
    Ok(summary)
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let opts = match args::parse(&argv) {
        Ok(args::Command::Run(opts)) => opts,
        Ok(args::Command::Help) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(msg) => {
            eprint!("logstat: {msg}\n{USAGE}");
            return ExitCode::from(2); // usage error
        }
    };

    let summary = match run(&opts) {
        Ok(summary) => summary,
        Err(e) => {
            eprintln!("logstat: {e}");
            return ExitCode::from(1); // runtime error
        }
    };

    let mut out = BufWriter::new(io::stdout().lock());
    match report::write(&mut out, &summary, opts.top).and_then(|()| out.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        // `logstat huge.log | head -3` closes the pipe early; that is not an error.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("logstat: writing output: {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use args::{Command, Options};
    use parse::{LineError, Record};

    const SAMPLE: &str = "\
2026-09-24T10:00:01Z GET /api/orders 200 37
2026-09-24T10:00:01Z GET /api/orders 200 41
2026-09-24T10:00:02Z POST /api/payments 201 180

2026-09-24T10:00:02Z GET /api/orders 500 1200
2026-09-24T10:00:03Z GET /health 200 1
this line is garbage
2026-09-24T10:00:04Z GET /api/users 404 12
2026-09-24T10:00:05Z GET /api/payments 429 25000
";

    fn summarize(input: &[u8]) -> stats::Summary {
        let mut s = stats::Summary::new();
        ingest(input, &mut s).unwrap();
        s
    }

    #[test]
    fn parses_a_valid_line() {
        let line = "2026-09-24T10:00:01Z GET /api/orders 200 37";
        assert_eq!(parse::line(line), Ok(Record { path: "/api/orders", status: 200, latency_ms: 37 }));
    }

    #[test]
    fn rejects_malformed_lines() {
        assert_eq!(parse::line("t GET /x 200"), Err(LineError::FieldCount));
        assert_eq!(parse::line("t GET /x 200 5 extra"), Err(LineError::FieldCount));
        assert_eq!(parse::line("t GET /x abc 5"), Err(LineError::Status));
        assert_eq!(parse::line("t GET /x 700 5"), Err(LineError::Status));
        assert_eq!(parse::line("t GET /x 200 -5"), Err(LineError::Latency));
    }

    #[test]
    fn counts_invalid_utf8_instead_of_crashing() {
        let s = summarize(b"t GET /a 200 5\n\xff\xfe broken\r\nt GET /b 200 7");
        assert_eq!((s.lines, s.invalid_utf8, s.malformed), (3, 1, 0));
    }

    #[test]
    fn percentiles_use_nearest_rank() {
        let mut s = stats::Summary::new();
        for ms in 1..=100 {
            s.record(&Record { path: "/", status: 200, latency_ms: ms });
        }
        assert_eq!(s.percentile(50.0), Some(50));
        assert_eq!(s.percentile(99.0), Some(99));
        assert_eq!(stats::Summary::new().percentile(50.0), None);
    }

    #[test]
    fn parses_arguments() {
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            args::parse(&argv(&["--top", "3", "a.log", "-"])),
            Ok(Command::Run(Options { top: 3, files: vec!["a.log".into(), "-".into()] }))
        );
        assert_eq!(args::parse(&argv(&["--help"])), Ok(Command::Help));
        assert!(args::parse(&argv(&["--top"])).is_err());
        assert!(args::parse(&argv(&["--top", "many"])).is_err());
        assert!(args::parse(&argv(&["--verbose"])).is_err());
    }

    #[test]
    fn full_report_matches_expected_output() {
        let s = summarize(SAMPLE.as_bytes());
        let mut out = Vec::new();
        report::write(&mut out, &s, 3).unwrap();
        let expected = "\
lines:      8 (1 malformed, 0 invalid UTF-8)
status:     1xx=0 2xx=4 3xx=0 4xx=2 5xx=1
p50:        41 ms
p95:        >10000 ms
p99:        >10000 ms
top paths:
         3  /api/orders
         2  /api/payments
         1  /api/users
";
        assert_eq!(String::from_utf8(out).unwrap(), expected);
    }
}
