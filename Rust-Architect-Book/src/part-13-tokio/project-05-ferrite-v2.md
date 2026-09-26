# Project Level 5 — An Async TCP Server (Ferrite v2)

> **Where this sits:** Part XIII · the fifth rung of the project ladder, and the second version of **Ferrite**
> **Uses:** Ferrite v1's `KvStore` and `ShardedStore` (Project L4; values now shared, per Chapter 20.7's measurement),
> the runtime model (13.1), task placement (13.2), `Semaphore` (13.3), `JoinSet` / `CancellationToken` and cancel
> safety (13.4), and every bound of Chapter 13.5.
> **Full source:** `listings/part-13/project-05-ferrite-v2.rs` (about 810 lines), verified with rustc 1.98.1 and Tokio
> 1.53.1: it **runs in debug and release, and all 11 tests pass**. `main` starts two servers on `127.0.0.1` and drives
> them with in-process clients over real TCP sockets, including 1,000 concurrent connections.

Ferrite v1 (Project L4) served its `ShardedStore` with the L3 worker pool: one thread per connection. Its golden output
said so plainly: "peak connections served at once: 4 (= workers)". Eight clients connected, four were served, and four
waited for a whole connection to finish. A cache whose clients keep pools of long-lived connections can't work that way.

v2 keeps everything that was right about v1 (the storage contract, the protocol grammar, the poisoning policy) and
replaces the server with an async one. It also takes on the production concerns v1 listed and deferred: a write
timeout, a cap on unflushed replies, a connection limit that refuses instead of queueing, graceful shutdown with a drain
deadline, per-server metrics, and requests that arrive split across TCP reads.

```text
 v1  Part XI     Project L4      in-memory ShardedStore, threaded TCP server, line protocol
 v2  Part XIII   (this project)  same store, same grammar, on Tokio: limits, backpressure, timeouts, graceful shutdown
 v3  Part XXIII  Project L7      WAL + memtable + sorted segments on disk, recovery, fsync policy
 v4  Part XXIV   Project L10     range scans, a small query layer, MVCC snapshot reads
 v5  Part XXIV   Project L11     leader/follower replication, Raft-style election, key-range partitioning
```

---

## 1. Requirements

| Area | Requirement |
|---|---|
| Storage | v1's `KvStore` trait (same three required methods, plus a defaulted `get_shared`) and `ShardedStore`, now storing `Arc<[u8]>` values (in a workspace, the `ferrite-store` crate) |
| Protocol grammar | v1's: `PING`, `GET <key>`, `SET <key> <value>`, `DEL <key>`, one per `\n`-terminated line, case-insensitive commands |
| Replies | v1's: `+PONG`, `+OK`, `$<value>`, `_`, `-ERR ...`. **New in v2:** every error is `-ERR <CODE> <detail>`, with a stable code |
| Error codes | `BUSY`, `SHUTTING_DOWN`, `LINE_TOO_LONG`, `INVALID_UTF8`, `EMPTY`, `UNKNOWN_COMMAND`, `WRONG_ARITY` |
| Framing | a request may arrive in pieces across reads, or several in one read; `\r\n` accepted |
| Concurrency | one Tokio task per connection; no worker-per-connection limit |
| Connection limit | `max_connections`; beyond it, reply `-ERR BUSY connection limit reached` and close (never queue) |
| Timeouts | idle (no complete request for `idle_timeout`: close); write (a reply can't be handed to the kernel within `write_timeout`: close) |
| Backpressure | at most `max_unflushed` reply bytes buffered per connection before the server waits for the socket |
| Limits | request line ≤ `max_line` (`-ERR LINE_TOO_LONG`, then close without losing that reply) |
| Accept queue | `backlog` configurable (Tokio's `bind` would use 128, Chapter 13.5 §6) |
| Shutdown | stop accepting; each connection finishes its current request, replies `-ERR SHUTTING_DOWN`, closes; abort what remains at `drain_timeout` |
| Robustness | a panic kills one connection, not the server; v1's shard poisoning policy still holds |
| Metrics | per-server counters for every way a connection ends, refusals, requests, peak connections |

**Non-goals** (§6): persistence, binary-safe values, authentication and TLS, replication.

---

## 2. Design

```text
            ┌──────────────── accept task: Server::run ─────────────────────────────────────────────┐
 clients ──►│ select! { biased;                                                                       │
            │   shutdown.cancelled()      → break, then drain                                         │
            │   conns.join_next()         → reap a finished connection (count panics)                 │
            │   listener.accept()         → try_acquire_owned() on Semaphore(max_connections)          │
            │ }                                  │ Ok(permit)                    │ Err: full            │
            └────────────────────────────────────┼───────────────────────────────┼────────────────────┘
                                                 ▼                               ▼
               JoinSet<()>: one task per connection, owned by run()     refuse(): "-ERR BUSY ...", FIN, linger
               ┌──────────────────── serve(stream, &*store, &config, &stats, &stop) ────────────────────┐
               │ FramedRead<OwnedReadHalf, LineCodec>       FramedWrite<OwnedWriteHalf, ReplyCodec>      │
               │ loop {                                                                                  │
               │   select! { stop.cancelled() → SHUTTING_DOWN; timeout(idle, requests.next()) }          │
               │   handle_line(store, &line)          ◄── synchronous: no lock survives this call        │
               │   timeout(write, feed(reply) + flush unless another request is already buffered)       │
               │ }                                                                                       │
               └────────────────────────────────────────┬────────────────────────────────────────────────┘
                                                        │ &S where S: KvStore + Send + Sync (an Arc<S> in the task)
                                                        ▼
                                    ShardedStore (v1 + shared values): 16 × RwLock<HashMap<Vec<u8>, Arc<[u8]>>>
```

**Ownership decisions:**

| Value | Owner | Shared how | Why |
|---|---|---|---|
| The store | `Arc<S>`, cloned into each connection task | `&self` methods (v1's contract) | as in v1: the store synchronizes internally |
| Values | the shard maps, as `Arc<[u8]>` | a GET clones the `Arc` into `Response::Value` | bytes are copied once, into the write buffer, outside any lock (Chapter 20.7) |
| The listener | `Server`, then `run()` | not shared | dropped at shutdown, which makes the kernel refuse new connections |
| Each connection's socket | its task, split into owned read and write halves | moved, never shared | the reader and the writer are two fields of one task: no lock |
| Partial requests | the `FramedRead`'s buffer | owned by the reader | survives a cancelled `next()`: that's what makes the read cancel safe (13.4) |
| Unflushed replies | the `FramedWrite`'s buffer | owned by the writer | capped by `max_unflushed` |
| Connection tasks | a `JoinSet` inside `run()` | owned | none can outlive the server (structured concurrency, 13.4) |
| Connection permits | `OwnedSemaphorePermit`, moved into each task | RAII | released however the task ends, including a panic |
| `Config`, `Stats` | `Arc`s cloned into tasks | read-only / atomics | per server, not process-global (v1's statics are gone) |
| Shutdown signal | a `CancellationToken`; each task holds a clone | cooperative | connections stop *between* requests, never mid-request |

Six decisions carry the design.

**The store stays synchronous, and that's safe in async code.** `ShardedStore`'s methods take a `std::sync::RwLock`,
do one `HashMap` operation, and return. They take microseconds and never wait on I/O, so calling them on a worker is
ordinary CPU work between `.await`s, well inside Chapter 13.2's budget. And because every guard dies inside the method
call, **no lock ever crosses an `.await`**, whatever the value type. If a future store's methods did block (v3's disk
reads can), the server would call them through `spawn_blocking` or the trait would gain an async variant. That's v3's
decision, with v3's measurements.

**Values are shared, not copied under the lock.** Project L4's review asked whether `get` should return shared
immutable values (`Arc<[u8]>`, `Bytes`) instead of copying under the shard lock. Chapter 20.7 measured exactly that
question on Ferrite's shard layout (listing `part-20/ch07-05-ferrite-shards.rs`: 16 shards, 4 KiB values, 95% GETs):
storing `Arc<[u8]>` made a GET 1.9× faster on four threads and 2.8× on one, while switching the lock from `Mutex` to
`RwLock` bought only 10%. The copy, not the lock, was the cost. So v2 stores `Arc<[u8]>` values. A GET clones a pointer
under the read lock, and the bytes are copied once, into the reply buffer, after the lock is released. The `KvStore`
contract keeps its three required methods unchanged and gains one **defaulted** method, `get_shared`, so every v1
implementation (and the test store in §4) still compiles. Adding a defaulted method is the backward-compatible way to
evolve a trait (Chapter 6.1).

**One task per connection, bounded by a semaphore that refuses.** A task costs 88 bytes plus its future (13.1 §5), so
connections are cheap to hold. What must be bounded is the number of them (memory, descriptors, kernel socket buffers).
The accept loop takes a permit with `try_acquire_owned()`, and a full server replies `-ERR BUSY` at once. Queueing
connections would hide overload behind latency, which is Chapter 13.5's unbounded row.

**Errors gain codes, and the grammar doesn't change.** v1's error replies were free text. Chapter 8.2's rule "never
match on messages" means clients need something stable to switch on, and Chapter 8.4's design exercise asked for busy and
shutting-down codes. So every error reply is now `-ERR <CODE> <detail>`: the code is a stable token and the detail is for
people. A v1 client that treats everything after `-ERR` as text keeps working, and a v2 client switches on the first
token.

**Framing is a codec.** Chapter 2.4's design exercise predicted that a frame would arrive split across two reads, and
Chapter 13.4 showed what a non-cancel-safe reader does in a `select!`. v2 uses `tokio_util::codec::FramedRead` with a small
decoder. Partial lines live in the reader's buffer, `next()` is cancel safe, and a request split into three TCP writes 20
ms apart is reassembled (the golden output shows it).

**Every wait has a bound.** Waiting for the next request: `idle_timeout`. Handing a reply to the kernel:
`write_timeout`. Buffering replies: `max_unflushed`. The line: `max_line`. The accept queue: `backlog`. Shutdown:
`drain_timeout`. That's Chapter 13.5's rule ("every queue needs a bound, and every bound needs a policy") applied to one
server.

---

## 3. Implementation walkthrough

### The one store change: shared values

```rust,ignore
/// v2: the value as a shared, immutable buffer. Defaulted, so every v1 implementation still compiles;
/// stores that keep `Arc<[u8]>` values override it to avoid the copy.
fn get_shared(&self, key: &[u8]) -> Option<Arc<[u8]>> {
    self.get(key).map(Arc::from)
}

// ... and ShardedStore's implementation:
fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Option<Vec<u8>> {
    let value: Arc<[u8]> = Arc::from(value); // allocate and copy before taking the lock
    let shard = self.shard(&key);
    let previous = self.write(shard).insert(key, value);
    previous.map(|v| v.to_vec()) // after the guard is gone
}

fn get_shared(&self, key: &[u8]) -> Option<Arc<[u8]>> {
    self.read(self.shard(key)).get(key).cloned() // under the lock: a reference-count increment
}
```

(All excerpts are from `project-05-ferrite-v2.rs`.) Every copy of value bytes now happens **outside** a shard lock:
`put` builds the `Arc<[u8]>` before taking the write lock, `get_shared` clones a pointer under the read lock, and
`Response::Value` holds the `Arc<[u8]>` until `encode` copies the bytes into the connection's write buffer. The trait's
`get` (still returning `Vec<u8>` for v1 callers) is `get_shared` plus a copy after the lock is released, and
`store_semantics`, v1's conformance test, passes unchanged. The poisoning policy is untouched: the critical sections are
still single `HashMap` operations.

### The decoder: lines out of arbitrary pieces

```rust,ignore
impl Decoder for LineCodec {
    type Item = BytesMut;
    type Error = FrameError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<BytesMut>, FrameError> {
        // Resume the search where the last call stopped: each byte is scanned once, however it arrives.
        match memchr::memchr(b'\n', &src[self.scanned..]) {
            Some(i) => {
                let end = self.scanned + i; // index of the '\n'
                self.scanned = 0;
                if end > self.max_line {
                    return Err(FrameError::TooLong);
                }
                let mut line = src.split_to(end + 1); // the line and its '\n' leave the buffer, no copy
                line.truncate(end);
                if line.last() == Some(&b'\r') {
                    line.truncate(end - 1);
                }
                Ok(Some(line))
            }
            None if src.len() > self.max_line => Err(FrameError::TooLong),
            None => {
                self.scanned = src.len();
                Ok(None)
            }
        }
    }
}
```

`FramedRead` calls `decode` whenever new bytes arrive. Returning
`Ok(None)` means "not a whole line yet, read more". Three details:

- **`scanned` makes the scan linear.** Without it, a 60 KiB line arriving 1 KiB at a time would be searched from the
  start 60 times (quadratic). With it, every byte is examined once. `memchr` does the search with SIMD (Chapter 9.2).
- **`split_to` doesn't copy.** It splits the `BytesMut` in two at the newline and hands the front half out as the
  frame, sharing the allocation (Chapter 9's `Bytes` model).
- **The length check happens before a newline is found**, so a client that sends 10 MB without a `\n` is stopped at
  `max_line` bytes, not after the whole thing is buffered.

### The connection loop

```rust,ignore
let why = loop {
    // Between requests is the only place where shutdown or idleness may stop a connection.
    let next = tokio::select! {
        biased;
        _ = stop.cancelled() => {
            let _ = send_now(&mut replies, config, Response::Error(Code::ShuttingDown, "server is shutting down".into())).await;
            break Closed::Shutdown;
        }
        next = timeout(config.idle_timeout, requests.next()) => next, // FramedRead::next is cancel safe
    };
    let line = match next {
        Err(_elapsed) => break Closed::Idle,
        Ok(None) => break Closed::ByClient,
        // ... LINE_TOO_LONG: reply, FIN, linger (below); an I/O error: the client is gone
        Ok(Some(Ok(line))) => line,
    };
    stats.requests.fetch_add(1, Relaxed);
    let reply = protocol::handle_line(store, &line); // no lock outlives this call: nothing to hold across .await
    // Flush before we might wait for input: that is, unless another complete request is already buffered.
    let more_waiting = memchr::memchr(b'\n', requests.read_buffer()).is_some();
    let wrote = timeout(config.write_timeout, async {
        replies.feed(reply).await?; // buffers; writes (and waits) first if max_unflushed is reached
        if !more_waiting {
            replies.flush().await?;
        }
        Ok::<(), io::Error>(())
    })
    .await;
    match wrote {
        Ok(Ok(())) => {}
        Ok(Err(_)) => break Closed::ByClient,
        Err(_elapsed) => break Closed::SlowReader, // the client isn't reading its replies
    }
};
```

Read it with Chapter 13.4's question: *what happens if this is cancelled at each `.await`?*

- **The `select!` races only the wait for the next request.** Shutdown and idleness can stop a connection there and
  nowhere else, and `FramedRead::next` is cancel safe, so a half-received request left in the buffer is simply dropped
  with the connection. Serving a request (`handle_line`) and writing its reply are never raced with shutdown: a client
  never gets a request applied without its reply being attempted.
- **`biased;` puts shutdown first.** Without it, `select!` picks a random branch first, and a client streaming requests
  could delay noticing shutdown for a while. With it, a connection sees the token at its next request boundary.
- **The flush rule is v1's rule, restated for a codec.** v1 flushed when its `BufReader` was empty. v2 flushes unless
  the read buffer already holds a complete request (a `\n`), because then the next loop iteration doesn't wait for the
  network. A pipelining client that sends 100 requests in one packet gets 100 replies in (usually) one write. A client
  that sends a request and waits gets its reply immediately. Flushing "unless the buffer is empty" would be wrong for a
  codec: a partial next request would delay the current reply until the rest arrived.
- **Backpressure is `max_unflushed`.** `FramedWrite::set_backpressure_boundary` makes `feed` write out (and await) the
  buffer once it holds that many bytes. A client that pipelines and never reads fills its socket buffers (Chapter 13.5
  §3: ~2.5 MiB on loopback), the write stops completing, and the `write_timeout` around `feed`+`flush` ends the
  connection. That fixes v1's documented gap ("a client that pipelines requests and never reads replies ... the worker
  blocks in `write` forever"). v2's task would have waited forever too, without a thread, which is cheaper but not
  better.
- **`set_nodelay(true)`.** Replies are small, and Nagle's algorithm would hold a small reply for up to one round trip
  waiting to coalesce (Chapter 21.1 covers the mechanism). The flush rule already batches pipelined replies, so Nagle
  adds only latency.

### The accept loop and the connection limit

```rust,ignore
match Arc::clone(&limit).try_acquire_owned() {
    Ok(permit) => {
        self.stats.accepted.fetch_add(1, Relaxed);
        let (store, config) = (Arc::clone(&self.store), Arc::clone(&self.config));
        let (stats, stop) = (Arc::clone(&self.stats), shutdown.clone());
        conns.spawn(async move {
            let _permit = permit; // released when the connection ends, however it ends
            let _active = ActiveGuard::new(&stats);
            serve(stream, &*store, &config, &stats, &stop).await;
        });
    }
    Err(_) => {
        self.stats.refused_busy.fetch_add(1, Relaxed);
        // Refuse in a task of its own: a refused client that doesn't read can't stall accept.
        conns.spawn(refuse(stream));
    }
}
```

The permit and the `ActiveGuard` are both RAII values inside the task's future, so a connection that ends normally, times
out, is aborted at shutdown, or **panics** returns its permit and decrements the active count (the test
`a_panic_costs_one_connection_not_the_server` checks exactly that). The loop's other `select!` branch,
`Some(done) = conns.join_next()`, reaps finished connections as they end, so the `JoinSet` doesn't accumulate results
(Chapter 13.4 §5), and it records panics from the `JoinError`.

An `accept` error (`EMFILE` when descriptors run out, `ECONNABORTED` when a client gave up in the queue) sleeps 10 ms and
continues. Without the sleep, a persistent `EMFILE` turns the accept loop into a 100% CPU spin.

### Closing without losing the last reply

```rust,ignore
/// Closing a socket with unread input makes the kernel send RST, which can destroy a reply still in flight.
/// So after the last reply and our FIN, read and discard what the client is still sending, briefly.
async fn linger(mut rd: impl AsyncRead + Unpin) {
    let mut discard = tokio::io::sink();
    let _ = timeout(Duration::from_millis(100), tokio::io::copy(&mut (&mut rd).take(1 << 20), &mut discard)).await;
}
```

When the server gives up on a connection that is **still sending** (a line that's too long, a refusal while the
client's first request is already in flight), closing the socket with unread bytes in its receive buffer makes Linux
send an RST instead of a FIN [OS]. An RST can make the client's kernel throw away data it has received but the client
hasn't read yet, including the error reply that explains the close. So v2 sends the reply, closes its write side (FIN),
and reads and discards the client's remaining input for up to 100 ms and 1 MiB before dropping the socket. Web servers
call this a *lingering close*. The golden output's `-ERR LINE_TOO_LONG` line for a 70,000-byte request is what it
protects.

### Shutdown

`run` breaks out of the accept loop when the token is cancelled, **drops the listener** (the kernel now refuses new
connections: the test `shutdown_says_goodbye_between_requests_and_drains` checks that a new `connect` fails), and gives the
connections `drain_timeout` to finish. Each connection sees the token at its next request boundary, replies
`-ERR SHUTTING_DOWN server is shutting down`, and closes. After the deadline, `JoinSet::shutdown()` aborts the rest: the
test `the_drain_deadline_aborts_connections_stuck_writing` has a connection blocked writing to a client that doesn't read
(with a 30 s write timeout), and shutdown aborts it after the 200 ms drain deadline.

---

## 4. Testing

**The golden output** (debug run; the release run prints the same lines except the throughput figure):

```text
-- one session
> PING             < +PONG
> SET user:1 ada   < +OK
> GET user:1       < $ada
> GET user:2       < _
> DEL user:1       < +OK
> DEL user:1       < _
> BOGUS 1          < -ERR UNKNOWN_COMMAND unknown command 'BOGUS'
> GET              < -ERR WRONG_ARITY wrong number of arguments for 'GET'
>                  < -ERR EMPTY empty command
-- one request split across three TCP writes, 20 ms apart, then a pipelined GET
["+OK", "$works"]
-- a 70,000-byte request line
< -ERR LINE_TOO_LONG request longer than 65536 bytes
< <closed> (the server hung up)
-- 1000 concurrent connections x 100 commands (fd limit 524288)
all 101000 replies correct: true; 116098 commands/s, clients and server in one process (one run)
peak connections served at once: 1001; OS threads in the process: 5 before, 5 during
server 1 stopped: drained true, aborted 0
server 1 stats: accepted 1001 | refused BUSY 0 | requests 101011 | closed: client 1000, idle 0, slow reader 0, line too long 1, shutdown 0 | panics 0 | peak connections 1001
-- limits: max_connections 2, idle 300 ms, write timeout 300 ms, drain 200 ms
client A: +PONG   client B: +PONG
client C: -ERR BUSY connection limit reached   then <closed>
idle client: <closed> after 300 ms
slow reader: disconnected after 600 ms
last client: +PONG
last client, after shutdown: -ERR SHUTTING_DOWN server is shutting down   then <closed>
server 2 stopped: drained true, aborted 0
server 2 stats: accepted 5 | refused BUSY 1 | requests 46 | closed: client 2, idle 1, slow reader 1, line too long 0, shutdown 1 | panics 0 | peak connections 2
store: 50002 keys across 16 shards (smallest 3039, largest 3223), poison recoveries 0
```

How to read it:

- **The session block is v1's protocol, with codes.** Same replies for every success; every error now starts with its
  code.
- **The split request**: `SE`, then `T split `, then `works\r\nGET split\n`, 20 ms apart, became one `SET split works`
  (the `\r` stripped) and one pipelined `GET`, answered in order.
- **The 70,000-byte line** got its `-ERR LINE_TOO_LONG` reply intact before the close, courtesy of the lingering close.
- **"peak connections served at once: 1001"** is the line to compare with v1's "4 (= workers)": 1,000 load clients plus
  the session client, all served at once, on **5 OS threads** (main plus 4 workers, the Playground's CPU count). v1
  would have needed 1,001 threads, or made 997 clients wait.
- **Throughput is one run on the shared Playground, with clients and server in one process** and every command a full
  request/reply round trip (no pipelining): 107,138–116,098 commands/s in debug and 160,272–202,685 in release over two
  runs each. Compare with v1's 98,615 (debug) and 184,364 (release) from 8 clients: similar per-command cost, now at 125×
  the connection count. The demo's values are a few bytes, so the shared-value change doesn't show here: its benefit
  grows with value size, and Chapter 20.7 measured it at 4 KiB.
- **Server 2's stats account for every connection**: accepted 5 (A, B, the idle client, the slow reader, the last
  client), refused 1 (C), closed by client 2 (A and B), idle 1, slow reader 1, shutdown 1. Requests 46 means the slow
  reader's 2,000 pipelined `GET big` requests produced about 40 replies (~2.6 MB of 64 KiB values) before its socket
  buffers filled and the 300 ms write timeout fired. Counts like this vary slightly between runs (another run closed 999
  connections "by client" and 1 at "shutdown", when a client's close raced the shutdown signal).

**What the first runs taught.** The golden output above is from the third version of `main`. The first two are worth
more than the final one:

1. **The first version set `idle_timeout: 2 s`** and made all 1,000 load clients wait at a `Barrier` after their first
   `PING`, so that all connections would be open at once. Its release run printed `all 101000 replies correct: false`,
   `peak connections 817`, and `closed: ... idle 183`. The idle timeout had done its job: 183 clients had sat at the
   barrier, silent, for more than 2 s, and the server closed them. That's the most common misconfiguration of idle
   timeouts in production: **an idle timeout shorter than a legitimate client's think time** (a connection pool that
   holds connections open between bursts, here a barrier). v2's demo now uses 10 s for that server.
2. **Why did 1,000 clients take more than 2 s to reach the barrier?** The second version (with 10 s) was correct but
   slow: 31,867 commands/s in debug and 60,667–65,605 in release. The listener came from `TcpListener::bind`, whose
   backlog is **128** (13.5 §6). A thousand simultaneous connects overflowed the accept queue, and many handshakes waited
   for SYN or SYN-ACK retransmissions, about a second each. Listing `ch05-07-backlog-burst.rs` isolates the effect:
   with a backlog of 128, only 243–609 of 1,000 connections were accepted within half a second, though every client's
   `connect()` had returned. With `backlog: 2_048`, the same demo ran at 101,654–108,988 commands/s in debug and
   173,570–196,220 in release, about 3×. The `backlog` field in `Config` exists because of this run.

**Unit and integration tests** (11, all passing):

```text
test tests::frames_split_across_reads_are_reassembled ... ok
test tests::errors_carry_stable_codes ... ok
test tests::long_lines_are_rejected_with_or_without_a_newline ... ok
test tests::a_panic_costs_one_connection_not_the_server ... ok
test tests::shutdown_says_goodbye_between_requests_and_drains ... ok
test tests::store_semantics ... ok
test tests::over_the_limit_is_refused_with_busy_and_capacity_comes_back ... ok
test tests::v1_poisoning_policy_still_serves_the_shard ... ok
test tests::idle_connections_are_closed ... ok
test tests::a_client_that_does_not_read_is_disconnected ... ok
test tests::the_drain_deadline_aborts_connections_stuck_writing ... ok
```

- **`frames_split_across_reads_are_reassembled`** drives the decoder directly with the chunks `SE`, `T k v\r\nGE`, `T k\n`
  and checks for exactly `["SET k v", "GET k"]` and an empty buffer. It's the 2.4 design exercise as a test, and it
  needs no sockets.
- **`store_semantics`** is v1's conformance test, unchanged. Every Ferrite store must pass it (v3 will run it against
  the disk engine).
- **The network tests use real sockets and real time** (100–200 ms timeouts), not a paused clock. Tokio's paused clock
  auto-advances when every task is idle, and a task waiting on a real socket counts as idle, so an idle timeout would
  fire instantly. Small real timeouts keep the suite at 0.33 s.
- **`a_panic_costs_one_connection_not_the_server`** uses a test store (`Exploding`) that panics on `GET boom`. The victim
  connection is closed, a bystander on another connection still gets `+PONG`, the server counts one panic, and the
  active-connection gauge returns to 1. It checks the promise from Chapter 8.3 (per-request containment) and 13.2 (the
  runtime catches task panics).
- **`v1_poisoning_policy_still_serves_the_shard`** poisons a shard through v1's test hook before starting the server and
  checks that `GET k` over TCP still returns `$v` and that the recovery was counted once.

---

## 5. Production concerns

- **Memory per connection.** A task is 88 bytes plus its future (13.1 §5), but a framed connection also holds two
  buffers: listing `ch05-08-framed-buffers.rs` measures **16,384 bytes** for an idle `FramedRead` + `FramedWrite` pair
  (tokio-util 0.7.19 starts each with 8 KiB). At 100,000 connections that's **1.64 GB** before anyone sends a byte, plus
  kernel socket buffers (13.5 §5). For connection-heavy deployments, create the reader with
  `FramedRead::with_capacity(rd, codec, 512)` and let buffers grow on demand, and cap `SO_SNDBUF`. The `max_connections`
  limit should be computed from this budget, not guessed.
- **The refusal path is bounded only by time.** Each refused connection gets a task for up to ~200 ms (100 ms to write,
  100 ms of lingering). A connection flood of 50,000/s would hold ~10,000 refusal tasks. That's cheap, but it's not
  bounded. A second semaphore for refusals (and simply closing, without a reply, beyond it) closes the gap.
- **One slow request per connection.** Requests on one connection are served in order. That's the protocol's contract
  (pipelined replies must stay in order), and it means a slow request (a large `GET`) delays the requests behind it on
  that connection, not on others. v3's disk reads will make that visible.
- **The store is synchronous.** For the in-memory store, calling it on a worker is right (microseconds, no I/O). A disk
  store must not be called this way (Chapter 13.2): v3 either keeps disk I/O off the workers (`spawn_blocking`, or a
  dedicated I/O thread with a channel) or grows an async trait. The server code isolates the call in one line,
  `protocol::handle_line(store, &line)`, which is where that change will happen.
- **Values are still whitespace-free UTF-8 tokens**, and a value is limited by `max_line`. Binary-safe framing
  (length-prefixed bulk values) is Project L6's protocol.
- **No authentication, no TLS.** Bind to localhost or a private network. Chapter 21.3 adds TLS.
- **Metrics are counters without labels.** They're per server now (v1's global statics are gone), and exporting them is
  one `tracing` or Prometheus adapter away (Chapter 22.5). The ones to alert on: `refused_busy` rate, `closed_slow_reader`
  rate, `panicked` (any), and `peak` against `max_connections`.
- **Shutdown is only as graceful as the clients.** v2 tells a client `SHUTTING_DOWN` at its next request boundary. A
  client that doesn't send another request never sees it and just gets a close. Clients should treat both as "reconnect
  elsewhere, with jitter" (13.5 §9).

---

## 6. Omissions (deliberate)

| Omitted | Why here | Where it's covered |
|---|---|---|
| Persistence (WAL, segments, recovery, fsync) | v2 is about serving; durability is its own design | Project L7 (Ferrite v3), Part XXIII |
| A fallible store API (`FerriteError`) | an in-memory store can't fail | v3 adds a fallible trait next to `KvStore` (L4's plan) |
| Binary-safe values, length-prefixed framing | the v1 grammar is kept deliberately | Project L6 (Part XXII) |
| Atomic read-modify-write (`INCR`, compare-and-set) | a store-level feature, not a server one | L4's architecture extension; Chapter 23.6 |
| TLS, authentication | orthogonal to the async design | Chapter 21.3 |
| Per-client rate limits | needs client identity | Chapter 21.4, tower `RateLimit` (13.5) |
| Replication, partitioning | single process | Project L11 (Ferrite v5) |
| Metrics export, tracing | the counters exist; exporting them is ecosystem work | Chapter 22.5 |

---

## 7. Architecture review

*You're the principal engineer reviewing Ferrite v2 before it replaces v1 as the shared cache for three services. Model
answers are in Appendix A (Part XIII).*

1. **The store in async code.** `ShardedStore` uses `std::sync::RwLock`. Why is that correct here, and exactly what
   change to the store would make it wrong? What would you check in review to catch it?
2. **Refuse vs queue.** v2 refuses connections beyond `max_connections` with `BUSY`. Make the case for queueing them
   instead, then say why it loses. What should a well-behaved client do with `BUSY`?
3. **The flush rule.** Why does v2 check the read buffer for a `\n` rather than for emptiness? Construct a sequence of
   client writes where "flush only when the read buffer is empty" delays a reply indefinitely.
4. **Cancellation.** List every `.await` in `serve`. For each, say whether shutdown can interrupt it, what happens to the
   request in flight if it's interrupted, and why the design chose that.
5. **Timeouts.** What does `write_timeout` bound, exactly (per reply? per connection?). A client reads replies very
   slowly but steadily (1 KB/s). Is it ever disconnected? Should it be?
6. **Memory at scale.** Compute the memory for 100,000 idle connections (task, buffers, kernel) with v2's defaults.
   Which three changes would you make for a 100,000-connection deployment, and what does each cost?
7. **The error codes.** A client team asks for `-ERR NOT_FOUND` instead of `_` for missing keys. Answer them, with
   reference to protocol compatibility and to Chapter 8.2.
8. **Shutdown under load.** A deploy happens while 1,000 clients pipeline requests continuously. Walk through what each
   client sees, how long the drain takes, and what `Report` says. How would you make the drain deadline safe for
   `SET`s that were read but not yet executed? (Are there any?)

---

## 8. Extensions

- **Beginner:** add `EXISTS <key>` (`+1` / `+0`) with codec-level and TCP tests, and add a `closed_by_reason` breakdown
  to `Stats::line`.
- **Intermediate:** add a per-connection request rate limit (a token bucket in the connection task; reply
  `-ERR RATE_LIMITED` and keep the connection open). Test it on a real socket with a client that sends 1,000 `PING`s
  as fast as it can.
- **Advanced:** replace `refuse` with a bounded refusal path (a second `Semaphore`; beyond it, close without a reply),
  and write a test that floods the server with 5,000 connections while it's at its limit. Measure how many refusal tasks
  exist at the peak.
- **Systems:** measure memory per idle connection in v2 end to end (counting allocator + `/proc/self/status` VmRSS),
  first with the default `FramedRead` capacity and then with `with_capacity(512)`, at 1,000 and 10,000 connections.
  Compare with the 16,384-byte figure from listing `ch05-08`.
- **Architecture:** design v2's graceful *handover*: a new server process starts on the same port (`SO_REUSEPORT`),
  the old one stops accepting and drains, and clients reconnect without seeing errors. Which of v2's pieces change, and
  what does a client library need to do?
