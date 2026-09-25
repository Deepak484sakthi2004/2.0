# Project Level 1 — `logstat`: A Production-Grade CLI Tool

> **Where this sits:** Part II · the first rung of the project ladder
> **Uses:** everything in Part II: the toolchain and profiles (2.1–2.2), scalars and conversions (2.3), expressions and
> slices (2.4), patterns and let-else (2.5), structs, enums, and methods (2.6), modules and privacy (2.7).
> **Full source:** `listings/part-02/project-01-logstat.rs`, verified with rustc 1.98.1: runs in debug and release,
> and all six unit tests pass.

A "hello world" CLI teaches nothing about production. This project builds a small tool the way you'd build one that
other engineers depend on in on-call situations: bounded memory, streaming input, correct exit codes, graceful handling
of bad data and closed pipes, and tests that pin its behavior.

---

## 1. Requirements

`logstat` summarizes HTTP access logs, one request per line:

```text
<timestamp> <method> <path> <status> <latency_ms>
2026-09-24T10:00:01Z GET /api/orders 200 37
```

```text
usage: logstat [--top N] [FILE ...]
Summarizes access logs: status classes, latency percentiles, busiest paths.
Reads standard input when no FILE is given.
```

**Functional:** count lines; count malformed lines and invalid UTF-8 separately; count responses per status class
(1xx–5xx); report p50, p95, and p99 latency; list the N busiest paths.

**Non-functional**, which is where the engineering is:

| Requirement | Why it matters in production |
|---|---|
| **Memory bounded regardless of input size** (for a fixed set of paths) | People will run it on a 50 GB log on a bastion host with 2 GB of RAM |
| **Streaming**: one pass, stdin or files | `zcat *.gz \| logstat` and `tail -n 1000000 x.log \| logstat` must work |
| **Bad data is counted, never fatal** | Real logs contain truncated lines, binary garbage, and CRLF endings |
| **Correct exit codes**: 0 ok, 1 runtime error, 2 usage error | Scripts and CI branch on them |
| **Closed pipes aren't errors** | `logstat huge.log \| head -3` must not print an error or exit non-zero |
| **Output is buffered** | Thousands of `println!` calls, each locking stdout, would dominate run time for large reports |
| **Deterministic output** | Ties sorted, so outputs can be diffed and tested |

## 2. Design

```text
                 ┌────────────┐
 argv ─────────► │ args       │  Result<Command, String>   (usage errors → exit 2)
                 └─────┬──────┘
                       ▼
 stdin / files ─► ingest(): one reusable Vec<u8> line buffer
                       │   bytes ──UTF-8?──► &str ──► parse::line(&str) ──► Record<'_> (BORROWS the line)
                       │                                                      │
                       │                      invalid UTF-8 / malformed: counted, never fatal
                       ▼                                                      ▼
                 ┌──────────────────────────────────────────────────────────────────┐
                 │ stats::Summary  (OWNS everything it keeps)                        │
                 │   by_class: [u64; 5]                            fixed size        │
                 │   latency_buckets: Vec<u64> × 10,002 (1 ms res.) fixed size, ~80 KB│
                 │   path_hits: HashMap<String, u64>                grows with DISTINCT paths │
                 └───────────────────────────────┬──────────────────────────────────┘
                                                 ▼
                 report::write(&mut impl Write) ──► BufWriter<StdoutLock> ──► stdout
                                                    (BrokenPipe → exit 0)
```

**Ownership boundaries**, the first real ownership design in the book (Part III formalizes it):

- `ingest` **owns** one line buffer and reuses it for every line.
- `parse::line` returns a `Record<'_>` that **borrows** from that buffer. Parsing allocates nothing.
- `Summary::record` copies out only what must outlive the line: a path string, and **only the first time that path is
  seen**.
- `report::write` **borrows** the summary and writes into any `Write`: stdout in production, a `Vec<u8>` in tests.

**Module layout** for a real Cargo project (the verified listing uses the same modules inline, because the Playground
takes one file):

```text
logstat/
├── Cargo.toml
└── src/
    ├── main.rs     main(), run(), ingest(): I/O and process concerns (exit codes, stdout, stdin)
    ├── args.rs     Options, Command, parse()
    ├── parse.rs    Record<'a>, LineError, line()
    ├── stats.rs    Summary, MAX_TRACKED_MS, percentile(), top_paths()
    └── report.rs   write()
```

```toml
[package]
name = "logstat"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"

[dependencies]            # none: std only, deliberately (see §6)

[profile.release]
panic = "abort"           # a CLI: a panic is a bug; exit immediately (Chapter 2.2)
lto = true
codegen-units = 1
strip = true
```

## 3. Implementation walkthrough

### Arguments: an enum for what the user asked for

```rust,ignore
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
```

`Command` makes "help was requested" a *value*, not a side effect buried in the parser, so `main` decides what to print
and which exit code to use, and the parser stays testable. `match` on `argv[i].as_str()` uses literal patterns, a
guard (`flag if flag.starts_with('-') && flag != "-"`, since `-` conventionally means stdin), and a catch-all binding
`file`. The `?` after `map_err` returns early with the error (Part VIII covers `?` properly). For a real tool with
subcommands, reach for `clap` (Part XXII). Hand-rolling here keeps the project std-only and shows what such a library
does for you.

### Parsing: zero allocation per line

```rust,ignore
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
```

The interesting line is the **let-else with a tuple of `Option`s**. It asks for exactly six calls to `next()` and
requires the first five to be `Some` and the sixth to be `None`, so "exactly five fields" is one pattern. Tuple
operands are evaluated **left to right** [LANG], so the fields arrive in order. `Record<'a>` holds `&'a str`, a slice of
the input line, so no `String` is created. The `'a` says "this record can't outlive the line it came from." Part IV
explains exactly how the compiler enforces that, and Chapter 1.4 explained why this zero-copy choice is worth its
lifetime cost on a hot path. Status is validated to 100–599 because `Summary` uses `status / 100 - 1` as an array index:
**validate at the boundary, then index without fear.**

### Statistics: bounded memory by design

```rust,ignore
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
        // new() and top_paths() in the full listing
    }
}
```

Three design decisions to defend in review:

1. **A fixed histogram instead of storing every latency.** Exact percentiles over *n* requests need *O(n)* memory (store
   everything, then select). A histogram with 1 ms buckets up to 10 s is 10,002 `u64`s, about **80 KB whatever the
   input size**, and exact to 1 ms within the tracked range. Everything above 10 s lands in one overflow bucket and
   reports as `>10000 ms`. The trade is *bounded memory and one pass* against *resolution*. (The review asks what to
   change if you need both.)
2. **`get_mut`, then `insert`, instead of `entry(r.path.to_string())`.** The `entry` API needs an *owned* key up front,
   which means one `String` allocation per line even for paths already in the map. The two-step version allocates only
   for new paths and hashes a repeated path twice only on its first occurrence. At millions of lines, allocations are
   the cost that shows up first in a profile (Part XX).
3. **Private fields with public counters.** `lines`, `malformed`, and `by_class` are plain counters that `ingest`
   updates. The buckets and the path map have invariants (the buckets sum to `latency_count`), so they're private, and
   the `unreachable!` in `percentile` states that invariant explicitly.

### Ingest: one buffer, never fatal on data

```rust,ignore
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
```

- **Bytes first, then UTF-8.** `BufRead::lines()` would return an *error* on invalid UTF-8 and stop the whole run. Reading
  bytes with `read_until` and validating each line separately means one corrupt line costs one counter increment.
- **One buffer for the whole input.** `buf.clear()` keeps the capacity, so after the first long line there are no
  further allocations for line storage.
- **CRLF tolerance.** `trim_end_matches('\r')` handles logs copied from Windows.
- `impl BufRead` makes the function accept stdin, a `BufReader<File>`, or a `&[u8]` in tests. (That's a generic
  parameter written with `impl Trait`, covered in Parts VI and VII.)

### Main: process concerns in one place

```rust,ignore
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
```

`main` returns `std::process::ExitCode`, so every exit path has an explicit code and destructors still run (unlike
`std::process::exit`, which skips them, Chapter 1.3). Errors go to **stderr**, results to **stdout**, so
`logstat x.log > report.txt` never mixes the two.

## 4. Production concerns, explained

**Buffered, locked stdout.** [LIB] `println!` locks stdout on *every* call, and stdout is **line-buffered**, so each
line is a separate `write` syscall when it's connected to a pipe or file. For a report of a few lines it doesn't matter.
For a tool that prints a million lines, it's often the bottleneck. `BufWriter::new(io::stdout().lock())` takes the lock
once and batches writes into large syscalls. The explicit `flush()` matters too: `BufWriter` flushes on drop, but it
**ignores errors** when it does, so the only way to *see* a write error (a full disk, a closed pipe) is to flush
yourself.

**SIGPIPE and broken pipes.** [OS] When a reader such as `head -3` exits, the next write to the pipe raises `SIGPIPE`,
whose default action kills the process silently. That's how C tools behave. [RUNTIME] Rust's standard library
**ignores `SIGPIPE`** on Unix before `main` runs, so the write fails with an `EPIPE` error instead, which Rust reports as
`ErrorKind::BrokenPipe`. `println!` treats any write error as fatal and **panics** ("failed printing to stdout: Broken
pipe"), which is why a naive Rust CLI prints a panic message when piped into `head`. `logstat` writes through
`writeln!`, which returns the error, and treats `BrokenPipe` as success.

**Exit codes** follow the common Unix convention: `0` success, `1` runtime failure (unreadable file, write error), `2`
usage error. Scripts can tell "you called me wrong" apart from "the job failed."

**Memory profile.** Constant: the 80 KB histogram, the reusable line buffer (as large as the longest line), and the
report. Variable: the path map, one `String` plus a `u64` plus hash-table overhead **per distinct path**. For a log whose
paths contain IDs (`/api/orders/8a7f...`), "distinct paths" means "every line," and the bounded-memory promise breaks.
That's the central question of the architecture review.

**Performance profile** (a hypothesis until measured; Part XX measures it): per line, the work is UTF-8 validation,
whitespace splitting, two integer parses, one hash of the path (SipHash by default, [LIB] a DoS-resistant but not the
fastest hasher, Part IX), and a few counter increments. There's no allocation for known paths. For logs on local disk,
this kind of tool is typically bound by parsing and hashing rather than by I/O. Confirm that with a profiler before
changing anything.

## 5. Testing

The listing's six unit tests pass under `cargo test` (verified on the Playground):

```text
running 6 tests
test tests::parses_a_valid_line ... ok
test tests::parses_arguments ... ok
test tests::counts_invalid_utf8_instead_of_crashing ... ok
test tests::full_report_matches_expected_output ... ok
test tests::percentiles_use_nearest_rank ... ok
test tests::rejects_malformed_lines ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

The end-to-end **golden test** feeds this sample, which includes a blank line and a garbage line:

```text
2026-09-24T10:00:01Z GET /api/orders 200 37
2026-09-24T10:00:01Z GET /api/orders 200 41
2026-09-24T10:00:02Z POST /api/payments 201 180

2026-09-24T10:00:02Z GET /api/orders 500 1200
2026-09-24T10:00:03Z GET /health 200 1
this line is garbage
2026-09-24T10:00:04Z GET /api/users 404 12
2026-09-24T10:00:05Z GET /api/payments 429 25000
```

and asserts that the report is exactly:

```text
lines:      8 (1 malformed, 0 invalid UTF-8)
status:     1xx=0 2xx=4 3xx=0 4xx=2 5xx=1
p50:        41 ms
p95:        >10000 ms
p99:        >10000 ms
top paths:
         3  /api/orders
         2  /api/payments
         1  /api/users
```

Check it by hand; it's the best way to understand the percentile logic. Seven valid latencies sorted are
1, 12, 37, 41, 180, 1200, and 25000 (overflow bucket). p50's rank is ⌈0.5 × 7⌉ = 4, which gives 41 ms. p95's rank is
⌈6.65⌉ = 7, the overflow bucket. The blank line isn't counted, the garbage line is `malformed`, and the `/api/users` vs
`/health` tie at one hit each is broken alphabetically. Because `report::write` takes `&mut impl Write`, the test writes
into a `Vec<u8>` with no process, no stdout, and no files involved.

Run with an empty stdin (as the Playground does), the binary prints the empty report and exits 0 in both debug and
release:

```text
lines:      0 (0 malformed, 0 invalid UTF-8)
status:     1xx=0 2xx=0 3xx=0 4xx=0 5xx=0
p50:        n/a
p95:        n/a
p99:        n/a
top paths:
```

## 6. Deliberate omissions

| Omitted | Why here | Where it comes back |
|---|---|---|
| `clap` for arguments | Keep it std-only, and show what the library does | Part XXII |
| `anyhow`/`thiserror` for errors | `String` and `io::Error` are enough for now | Part VIII |
| Multithreading | One core is plenty until measured otherwise | Part XI (parallel ingest, Chapter 1.3's "no sharing" design) |
| `mmap` input | Streaming works for pipes; mmap only works for files | Part XIX |
| Fuzzing the parser | Needs tooling | Part XXII |

## 7. Architecture review

*You are the principal engineer reviewing `logstat` before it becomes the team's standard on-call tool. Answer each
question in writing. Model answers are in Appendix A (Part II).*

1. **Bottlenecks.** For a 20 GB log on a laptop SSD, what limits throughput? How would you find out rather than guess?
2. **Allocations.** List every allocation site in the steady state (after the first thousand lines). Which one can
   grow without bound?
3. **Unbounded cardinality.** Paths like `/api/orders/8a7f3c...` make the path map grow with every line. Propose two
   fixes with different trade-offs (hint: normalization, and a fixed-capacity heavy-hitters algorithm).
4. **Accuracy vs memory.** Product wants microsecond resolution *and* latencies up to 5 minutes. What happens to the
   histogram? Propose a design that keeps memory bounded (hint: log-linear buckets).
5. **Failure model.** A file becomes unreadable halfway through (NFS hiccup). What does `logstat` do now? Is a partial
   report with exit code 1 better or worse than no report? What do scripts expect?
6. **10x and 100x.** The team wants to run it continuously over a 200 MB/s log stream. What breaks first?
7. **Parallelism.** Sketch a multithreaded version using the "don't share, merge at the end" pattern from Chapter 1.3.
   What's the merge step for each field of `Summary`? Which statistics would be hard to merge if the histogram were
   replaced by exact percentiles?
8. **Ownership boundaries.** Where does data move from borrowed to owned, and why exactly there?
9. **`unsafe` and zero-copy.** Is any `unsafe` needed? Could the path strings be zero-copy too, and what would that
   require the input buffer to become?
10. **Priorities.** How would the design change if **time-to-first-result** mattered most (people want numbers within a
    second on huge files)? If **throughput** mattered most? If **developer velocity** mattered most?

## 8. Extensions

- **Beginner:** `--json` output (a hand-written JSON writer for now; Serde in Part XXII).
- **Intermediate:** `--since <timestamp>` filtering. Compare timestamps as strings when the format guarantees lexicographic
  ordering (RFC 3339 in UTC does), and explain why that's valid.
- **Advanced:** Replace the linear histogram with log-linear buckets (as in HdrHistogram) and bound the relative error
  at 1% from 1 µs to 1 hour. Compute the memory.
- **Systems:** Profile `logstat` on a generated 5 GB file with `perf` (on Linux) and produce a flame graph. Is the top of
  the profile what §4 predicted?
- **Architecture:** Make `stats` and `parse` a library crate and `logstat` a thin binary. Then sketch the Tokio service
  that reuses the library to tail logs over the network (Part XIII).
