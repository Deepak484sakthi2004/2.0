# Project Level 4 — A Concurrent Key-Value Store (Ferrite v1)

> **Where this sits:** Part XI · the fourth rung of the project ladder, and the first version of **Ferrite**, the system
> you'll grow for the rest of the book
> **Uses:** `Send`/`Sync` and trait objects (11.2, 6.4), `RwLock` and the poisoning policy (11.3), atomics (11.4),
> sharding (11.7), keyed hashing (9.3), and Project L3's worker pool, reused unchanged.
> **Full source:** `listings/part-11/project-04-ferrite-v1.rs` (about 630 lines, no dependencies), verified with rustc
> 1.98.1: it **runs in debug and release, and all 8 tests pass**. `main` starts the server on `127.0.0.1` and drives it
> with in-process clients over real TCP sockets.

Project L3 built a server whose handlers shared nothing. Real services share *state*. This project builds the smallest
honest version of the most common piece of shared state in backend systems, a key-value store, and serves it over a
line protocol. Everything later in the book grows from here:

```text
 v1  Part XI     (this project)  in-memory ShardedStore, threaded TCP server, line protocol
 v2  Part XIII   Project L5      same protocol on Tokio: connection limits, backpressure, timeouts, graceful shutdown
 v3  Part XXIII  Project L7      WAL + memtable + sorted segments on disk, recovery, fsync policy
 v4  Part XXIV   Project L10     range scans, a small query layer, MVCC snapshot reads
 v5  Part XXIV   Project L11     leader/follower replication, Raft-style election, key-range partitioning
```

So the most important deliverable here isn't the server. It's the **`KvStore` trait and its concurrency contract**,
which every later version keeps.

---

## 1. Requirements

| Area | Requirement |
|---|---|
| Library API | `KvStore { get(&self, key) -> Option<Vec<u8>>, put(&self, key, value) -> Option<Vec<u8>>, delete(&self, key) -> Option<Vec<u8>> }`, usable concurrently from any number of threads through `&self` |
| Storage | In memory: `ShardedStore`, `N` shards of `RwLock<HashMap<Vec<u8>, Vec<u8>>>`, shard = `hash(key) % N` with a keyed hash |
| Protocol (v1) | One request per `\n`-terminated line; tokens separated by ASCII whitespace; commands case-insensitive: `PING`, `GET <key>`, `SET <key> <value>`, `DEL <key>` |
| Responses | `+PONG`, `+OK`, `$<value>`, `_` (nil), `-ERR <message>`, each terminated by `\n` |
| Semantics | `SET` → `+OK`. `GET` → `$value` or `_`. `DEL` → `+OK` if a key was removed, `_` if it was absent |
| Pipelining | A client may send several requests before reading replies; replies come back in order |
| Limits | Line length ≤ 64 KiB (`-ERR line too long`, then close); idle timeout per connection (2 s in the demo) |
| Concurrency | Project L3's `WorkerPool<TcpStream>`: a fixed number of workers, a bounded queue, `-ERR server busy` when it's full |
| Robustness | A panic while a shard lock is held must not make the shard unusable (§3's poisoning policy) |
| Shutdown | Stop accepting, serve queued connections, join the workers, report counts |

**Non-goals** (§6): persistence, binary-safe keys and values, authentication, eviction, replication.

---

## 2. Design

```text
            ┌───────────── accept thread (Server::run) ─────────────┐
 clients ──►│ TcpListener::incoming() → pool.try_submit(stream)      │── Err(stream) ──► "-ERR server busy\n", close
            └──────────────────────────┬─────────────────────────────┘
                                       │ Ok: the TcpStream MOVES to one worker
                                       ▼
            ┌─ WorkerPool<TcpStream> (Project L3, unchanged) ───────────────────────────────────────┐
            │  worker k: handle_connection(stream, &*store, &config)                               │
            │    loop { read_until('\n', ≤ max_line) → parse → execute(store, request) → reply }   │
            └───────────────────────────────┬──────────────────────────────────────────────────────┘
                                            │ &S where S: KvStore + Send + Sync  (an Arc<S> captured by the handler)
                                            ▼
            ┌─ ShardedStore ───────────────────────────────────────────────────────────────────────┐
            │  router: RandomState (keyed SipHash)      shard i = hash_one(key) % 16                │
            │  [ RwLock<HashMap> ] [ RwLock<HashMap> ] ... [ RwLock<HashMap> ]   (16 shards)        │
            │  poison_recoveries: AtomicU64                                                         │
            └──────────────────────────────────────────────────────────────────────────────────────┘
```

**Ownership decisions:**

| Value | Owner | Shared how | Why |
|---|---|---|---|
| The store | `Arc<ShardedStore>`, held by `main` and by the pool's handler closure | `&self` methods; `ShardedStore: Send + Sync` because every field is | One store for every connection; the library decides how to synchronize, not the server |
| Each shard's map | Its `RwLock` | Read guards for `GET`, write guards for `SET`/`DEL` | Aliasing XOR mutation at run time, per shard (11.3) |
| Each `TcpStream` | The accept thread, then the queue, then one worker | Moved, never shared | Exactly as in L3: no lock on any socket |
| Keys and values | The map; `GET` returns a **copy** | Copied out under the read lock | A reference into the map can't outlive the guard (below) |
| `Config` | Moved into the pool's handler closure | Read-only | Workers read limits and timeouts |
| `ACTIVE`, `PEAK` counters | `static` atomics | `Relaxed` | Metrics only |

Three choices carry the design, and each is a Part XI chapter applied.

**The trait takes `&self`, not `&mut self`.** A store is shared by every connection. With `&mut self`, callers would
need a lock *around* the store, and one lock around the store is the "`Mutex<HashMap>`" row of Chapter 11.7's table,
the slowest one. With `&self`, the store synchronizes **internally**, so each implementation picks its own strategy:
16 locked shards here, a WAL writer thread plus a lock-free memtable in v3, a replication log in v5. The server needs
only `S: KvStore + Send + Sync + 'static`. That's the interior-mutability contract from Chapter 11.4, written as an API.

**`get` returns an owned `Vec<u8>`, not `&[u8]`.** A borrowed value would have to borrow from the map *inside* the
shard's lock, so the read guard would have to live as long as the reference. The API would then either hand out guards
(leaking the locking strategy into every caller and holding locks across network writes, 11.7 §10's mistake) or be
impossible to express. Copying under the lock costs one allocation per `GET`. §5 discusses the cheaper alternative,
`Arc<[u8]>` values, and why v1 doesn't take it yet.

**Sharding with a keyed hash.** Chapter 11.7 measured sharded maps as the fastest and most stable concurrent-map design
(74–81 ns per operation with 4 threads and 16 shards, against 160–272 ns for one `Mutex<HashMap>`). The router is
`std::hash::RandomState`, the same keyed SipHash as `HashMap`'s default. A client that controls keys can't aim them all
at one shard, which is Chapter 9.3's HashDoS defense applied one level up. The price is that two processes route the
same key to different shard numbers. That doesn't matter for an in-memory store, and it will matter in v5, where
partitioning across machines needs a stable hash.

---

## 3. Implementation walkthrough

### The contract

```rust,ignore
/// The Ferrite storage contract. Every method takes `&self`: implementations synchronize internally,
/// so one store can be shared by every connection (`Arc<S>` where `S: KvStore + Send + Sync`).
/// v1: in memory. v3 (Part XXIII): WAL + LSM on disk, behind this same trait.
pub trait KvStore {
    /// A copy of the value, if the key exists.
    fn get(&self, key: &[u8]) -> Option<Vec<u8>>;
    /// Inserts or replaces; returns the previous value.
    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Option<Vec<u8>>;
    /// Removes; returns the removed value.
    fn delete(&self, key: &[u8]) -> Option<Vec<u8>>;
}
```

(Listing `project-04-ferrite-v1.rs`; all excerpts in this chapter come from it.) Three details are deliberate:

- **Keys and values are bytes.** The protocol layer happens to accept UTF-8 tokens, but the storage contract doesn't
  care. v2's framing and v3's on-disk format will carry arbitrary bytes.
- **`put` takes ownership of `key` and `value`.** The store keeps them, so taking `Vec<u8>` by value lets the caller
  hand over buffers it already owns without a copy. `get` and `delete` only need to *find* the key, so they borrow it.
- **The trait is dyn-compatible** (Chapter 6.4): no generic methods, no `Self` by value. The test
  `works_as_a_trait_object` uses `Box<dyn KvStore + Send + Sync>`. Later versions may pick their store at run time
  (memory or disk) behind one pointer.

**The contract is infallible, and that's a v1 simplification.** An in-memory map can't fail except by running out of
memory, and Rust aborts on that (Chapter 9.1). A disk-backed store can fail on every call. Chapter 8.2's design exercise
already asked what `FerriteError` should look like. v3 will add a fallible trait next to this one, rather than change
this one, so v1 and v2 code keeps compiling.

### Shards and the poisoning policy

```rust,ignore
fn shard(&self, key: &[u8]) -> &RwLock<Map> {
    let h = self.router.hash_one(key);
    &self.shards[(h % self.shards.len() as u64) as usize]
}

// Poisoning policy: RECOVER (and count). Under a shard lock we run nothing but HashMap get/insert/
// remove on Vec<u8> keys, whose Hash and Eq cannot panic, and entries have no cross-key invariant.
// A map observed after some panic is still a valid map. (Contrast Chapter 11.3's ledger, whose
// invariant spans two fields: there, recovering blindly would be wrong.)
fn read<'a>(&'a self, shard: &'a RwLock<Map>) -> RwLockReadGuard<'a, Map> {
    shard.read().unwrap_or_else(|poisoned| {
        self.note_recovery(shard);
        poisoned.into_inner()
    })
}
```

Chapter 11.3 turned poisoning into a per-lock decision: *can a panic leave this lock's data breaking an invariant?* For a
shard, the answer is no. Every critical section is a single `HashMap` operation on byte vectors, whose `Hash` and `Eq`
can't panic. So the only way a shard lock gets poisoned is a panic somewhere *else* in the thread while a guard is alive
(an allocation failure is an abort, not a panic). The map itself is valid. Refusing to serve it would turn one bug into
an outage for 1/16 of the keyspace.

The policy is to **recover, count, and clear**. `note_recovery` increments `poison_recoveries` (a metric to alert on:
it means a bug exists) and calls `clear_poison()` (stable since 1.77), so the recovery is counted once, not on every
later access. The test `a_poisoned_shard_is_recovered_once` poisons a shard on purpose and checks exactly that: the
value is still served, twice, and the counter reads 1.

The rationale lives in a comment next to the lock, because a poisoning policy is a statement about invariants. If a
later version adds a second map that must stay consistent with this one (an index, say), the comment tells the reviewer
which decision to revisit.

**The operations** are one line each, and the copy in `get` happens under the read lock:

```rust,ignore
impl KvStore for ShardedStore {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.read(self.shard(key)).get(key).cloned() // the copy happens under the read lock
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Option<Vec<u8>> {
        let shard = self.shard(&key);
        self.write(shard).insert(key, value)
    }

    fn delete(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.write(self.shard(key)).remove(key)
    }
}
```

Look at `put`: `self.shard(&key)` borrows `key` only to compute the index. By elision, `shard`'s result borrows
`self`, not `key` (Chapter 4.3), so that borrow has ended by the time `key` moves into `insert`. The two lines make the
order visible. Every guard is a temporary that dies at the end of its statement (Chapter 11.3 §5), so no lock is held
across anything else.

**Why `RwLock` and not `Mutex` per shard?** Chapter 11.7 measured them as a tie inside a shard when critical sections are
tiny. Ferrite's `GET` critical section includes copying the value, which for large values isn't tiny. A read lock lets
two `GET`s of big values in the same shard copy in parallel. That's a prediction from mechanism, not a measurement. The
Systems extension in §8 asks you to measure it.

### Parsing and executing

The v1 grammar is deliberately simple: split on ASCII whitespace, match the command case-insensitively, and count
arguments exactly:

```rust,ignore
pub fn parse(line: &str) -> Result<Request, String> {
    let mut t = line.split_ascii_whitespace();
    let Some(cmd) = t.next() else {
        return Err("empty command".to_string());
    };
    let (a, b, extra) = (t.next(), t.next(), t.next());
    let args = [a, b, extra].iter().filter(|x| x.is_some()).count(); // 3 means "3 or more"
    let wrong = || Err(format!("wrong number of arguments for '{}'", cmd.to_ascii_uppercase()));
    // ... PING / GET / SET / DEL, each checking `args` exactly, else "unknown command"
}
```

Errors are *values* that become `-ERR` replies, and the connection stays open. A malformed request is the client's
problem (Chapter 8.4's "Rejected" class), and it mustn't cost the client its connection or the server a worker.

`execute` is generic over `S: KvStore + ?Sized`, so the same function serves a concrete `ShardedStore` and a
`dyn KvStore` (the `?Sized` is what admits the trait object):

```rust,ignore
/// SET -> +OK; GET -> $value or _; DEL -> +OK if something was removed, _ if the key was absent.
pub fn execute<S: KvStore + ?Sized>(store: &S, req: Request) -> Response {
    match req {
        Request::Ping => Response::Pong,
        Request::Get(k) => store.get(&k).map_or(Response::Nil, Response::Value),
        Request::Set(k, v) => {
            store.put(k, v);
            Response::Ok
        }
        Request::Del(k) => {
            if store.delete(&k).is_some() { Response::Ok } else { Response::Nil }
        }
    }
}
```

`Request::Set(k, v)` owns its two `Vec<u8>`s, and `execute` moves them straight into the store. The bytes read from the
socket are copied once, from the line buffer into the request, and never again.

### The connection loop: bounded lines, pipelining, one flush per batch

```rust,ignore
loop {
    line.clear();
    let n = reader.by_ref().take(config.max_line as u64 + 1).read_until(b'\n', &mut line)?; // timeout = Err
    if n == 0 {
        return Ok(()); // the client closed the connection
    }
    if n > config.max_line {
        // We can't find the next line boundary cheaply: report and hang up.
        Response::Error("line too long".into()).write_to(&mut writer)?;
        return writer.flush();
    }
    // ... trim the line end, check UTF-8, parse, execute, write the reply into `writer` (a BufWriter)
    if reader.buffer().is_empty() {
        writer.flush()?; // flush only when no pipelined request is already waiting: fewer syscalls
    }
}
```

Three production details, each from an earlier chapter:

- **Every read is bounded before it happens** (L3's `take(limit + 1)` trick). A client that never sends `\n` gets
  `-ERR line too long` after 64 KiB, instead of growing `line` without limit.
- **Pipelining costs nothing extra to support**, and the flush rule makes it cheap. Replies go into a `BufWriter`. The
  loop flushes only when the `BufReader` has no more buffered input, which means no pipelined request is already
  waiting. A client that sends 100 `SET`s in one packet gets 100 replies in (usually) one `write` system call, not 100.
  The golden output's `["+OK", "+OK", "$1"]` is three requests sent in one `write` and answered in order.
- **The read timeout ends idle connections.** `set_read_timeout(idle_timeout)` makes a blocked `read` return an error
  after 2 s of silence, and `?` ends the connection. That's what frees the worker. As in L3, it's the price of one worker
  per connection.

The loop uses `?` freely because the handler closure in `Server::bind` ignores the result:
`let _ = handle_connection(stream, &*store, &config); // I/O errors end the connection only`. An I/O error is a fact
about one connection, not about the server.

### Reusing the pool

The pool module is copied from L3 **unchanged**, and that's the reuse test the L3 review asked about. `WorkerPool<T>`
knows nothing about HTTP. It takes items of any `T: Send + 'static` and a handler `Fn(T) + Send + Sync + 'static`. Ferrite
passes `TcpStream`s and a closure that captures the `Arc<S>` and the `Config`:

```rust,ignore
let pool = WorkerPool::new(workers, capacity, "ferrite", move |stream: TcpStream| {
    ACTIVE.fetch_add(1, Ordering::Relaxed);
    PEAK.fetch_max(ACTIVE.load(Ordering::Relaxed), Ordering::Relaxed);
    let _ = handle_connection(stream, &*store, &config); // I/O errors end the connection only
    ACTIVE.fetch_sub(1, Ordering::Relaxed);
});
```

The bound `S: KvStore + Send + Sync + 'static` on `Server::bind` is exactly what `WorkerPool::new` needs from the
closure: `Arc<S>` is `Send + Sync` only when `S` is (Chapter 11.2), and the closure is shared by every worker. If a later
store implementation holds an `Rc` or a `RefCell` field, `Server::bind` stops compiling at the call site, before any
request is served. In a real workspace the pool would be its own crate (`ferrite-pool`), shared by the HTTP admin
endpoint and the data server.

---

## 4. Testing

**The golden output** is `main`'s scripted session over real localhost sockets. This is the debug run; the release run
prints the same lines except the three throughput figures:

```text
-- one session
> PING               < +PONG
> SET user:1 ada     < +OK
> GET user:1         < $ada
> GET user:2         < _
> SET user:1 grace   < +OK
> get user:1         < $grace
> DEL user:1         < +OK
> DEL user:1         < _
> GET user:1         < _
> BOGUS 1            < -ERR unknown command 'BOGUS'
> GET                < -ERR wrong number of arguments for 'GET'
> SET k v extra      < -ERR wrong number of arguments for 'SET'
>                    < -ERR empty command
-- pipelining: three requests in one write, three replies
["+OK", "+OK", "$1"]
-- 8 concurrent clients x 1,000 commands, 4 workers
all 8,000 replies correct: true; 98615 commands/s over TCP (one run)
peak connections served at once: 4 (= workers: one worker per connection)
-- the library without the network: 8 threads x 100,000 ops on the same store
2.6 M ops/s in-process (one run)
keys: 6002 across 16 shards (smallest 347, largest 411); poison recoveries: 0
server stopped: 9 connections served, 0 panics, 0 refused
```

How to read it:

- **The session block is the protocol specification, executed.** Each row is one request and its reply, including every
  error class: unknown command, wrong arity (too few and too many), and the empty line.
- **"peak connections served at once: 4 (= workers)"** is the L3 design showing through. Eight clients connected at
  once. Four were served, and four waited in the queue (capacity 16) until a worker finished a whole connection. They
  all got correct replies, but half of them waited for the other half. That's the one-worker-per-connection limit, and
  it's what Ferrite v2 removes in Part XIII.
- **The throughput numbers are one run each on the shared Playground**, and they say more as ratios than as values.
  Over TCP: 98,615 commands/s in debug and 184,364 in release. In process: 2.6 M ops/s in debug and 18.8 M ops/s in
  release. The ~100× gap between the library and the server (in release) is the cost of a request/reply round trip over
  loopback with one command in flight per connection: two system calls and a wakeup per command, against a hash lookup
  under an uncontended lock. That's the case for pipelining, and for batching in general.
- **"keys: 6002 across 16 shards (smallest 347, largest 411)"**: 6,002 keys means 4,000 TCP keys (8 clients × 500),
  2,000 library keys (8 threads × 250 distinct keys each), and the pipelined `a` and `b`. A perfectly even split would
  be 375 per shard. The keyed hash put every shard within 10% of that. The release run's split was 340–427.
- **"9 connections served"**: the session client and the eight load clients. The shutdown handle's wake-up connection
  isn't counted, because the accept loop checks the shutdown flag *before* submitting a connection, and so it drops
  that one unserved. `0 panics, 0 refused`.

**Unit and integration tests** (8, all passing):

```text
test tests::a_poisoned_shard_is_recovered_once ... ok
test tests::parse_accepts_the_v1_grammar ... ok
test tests::parse_rejects_bad_input_with_a_message ... ok
test tests::store_semantics ... ok
test tests::works_as_a_trait_object ... ok
test tests::responses_encode_as_specified ... ok
test tests::end_to_end_over_tcp ... ok
test tests::keys_spread_across_shards ... ok
```

- **The parser and the encoder are tested without sockets**: `parse` takes `&str`, and `Response::write_to` takes
  `impl Write`, so a `Vec<u8>` stands in for the connection.
- **`store_semantics`** pins down the return values the trait documents: `put` returns the previous value, and a
  second `delete` returns `None`. Every future store implementation should pass this same test. In v3 it becomes a
  shared conformance suite.
- **`keys_spread_across_shards`** inserts 16,000 keys and asserts every shard holds 750–1,250 (the ideal is 1,000). It's
  a statistical test with a wide margin. With a keyed hash, a spurious failure would need an extremely unlikely
  imbalance. With a *broken* router (say, `% 1`), it fails every time.
- **`end_to_end_over_tcp`** starts a real server with `max_line = 32`, checks a `SET`/`GET` round trip, checks that a
  40-byte line gets `-ERR line too long`, then checks through the library that the `SET` really landed in the shared
  store, and that shutdown reports no panics.

---

## 5. Production concerns

- **One worker per connection, again.** Everything L3 §5 said applies: idle clients hold workers until the idle timeout,
  and slow clients can hold them indefinitely. For a cache-like store whose clients hold long-lived connection pools,
  this is the design's hard limit: 1,000 client connections need 1,000 workers. That's why Part XIII rebuilds the server
  on Tokio while keeping `ShardedStore` behind `Arc`.
- **No write timeout.** `handle_connection` sets a read timeout but not a write timeout. A client that pipelines
  requests and never reads replies eventually fills its socket's receive buffer and the server's send buffer, and the
  worker blocks in `write` forever. The Advanced extension fixes it. It's the kind of gap a code review of a network
  service should look for first: **every blocking operation needs a bound**.
- **Memory is unbounded.** `max_line` bounds a single request, but nothing bounds the number of keys. A client can fill
  the heap with `SET`s. Production stores enforce a memory budget (refuse writes with `-ERR OOM`, or evict: an LRU like
  Chapter 3.6's, or sampled eviction like Redis). Tracking the budget needs a size counter, which is one more atomic, or
  one per shard (11.7).
- **`GET` allocates.** Every `GET` copies the value into a new `Vec<u8>`. For small values that's cheap. For 1 MB values
  it's a 1 MB allocation and copy per read, under the shard's read lock. Storing `Arc<[u8]>` (or `bytes::Bytes`) values
  would make a `GET` a reference-count increment, at the price of a changed trait signature and an atomic per read (11.2
  measured the contended cost). That's a v2/v3 decision, driven by the value-size distribution.
- **Hot keys defeat sharding.** Sharding spreads *different* keys. If 30% of traffic hits one key (a feature flag, a
  global counter), its shard's lock is the bottleneck, whatever `N` is. The remedies are workload-specific: cache hot
  reads client-side, split hot counters per thread (11.7's per-worker counters), or replicate hot keys.
- **No atomic read-modify-write.** A client that wants to increment a counter must `GET`, compute, and `SET`, which is
  Chapter 1.3's TOCTOU race across the network. Two clients incrementing concurrently lose updates. The Architecture
  extension adds `INCR` properly.
- **Metrics are process-global statics.** `ACTIVE` and `PEAK` are `static` atomics, so two servers in one process (say,
  in tests) share them. Metrics belong to the server instance, passed down like `Config`. v1 took the shortcut because
  it runs one server per process.
- **Security.** No authentication, no TLS, and the demo binds to `127.0.0.1`. Don't expose v1 to a network you don't
  control. Chapter 21.3 adds TLS.

---

## 6. Omissions (deliberate)

| Omitted | Why here | Where it's covered |
|---|---|---|
| Persistence (WAL, snapshots, recovery) | v1 is about concurrency; durability is its own design | Project L7 (Ferrite v3), Part XXIII |
| Async I/O, connection limits beyond the pool | The thread-per-connection baseline is the point | Project L5 (Ferrite v2), Part XIII |
| Binary-safe keys and values (length-prefixed framing) | The v1 grammar splits on whitespace, so tokens can't contain it | Project L6's protocol (Part XXII), Ferrite v2 |
| Atomic multi-key operations, transactions | Sharding made cross-shard atomicity impossible without a protocol (11.7) | Chapter 23.6, Project L10 |
| Range scans | A `HashMap` has no order | Project L10 (Ferrite v4) |
| Replication, partitioning across machines | Single process | Project L11 (Ferrite v5), Part XXIV |
| Eviction, TTLs, memory limits | See §5 | Extensions |
| Authentication, TLS | Orthogonal to the concurrency design | Chapter 21.3 |

---

## 7. Architecture review

*You're the principal engineer reviewing Ferrite v1 before a team uses it as a shared cache for three services. Model
answers are in Appendix A (Part XI).*

1. **The contract.** Why do the `KvStore` methods take `&self`? What would `&mut self` force every caller to do, and
   which row of Chapter 11.7's matrix would that put the system in?
2. **Copies.** `get` returns `Vec<u8>`. Why can't it return `&[u8]`? Compare returning a guard, returning `Arc<[u8]>`,
   and taking a callback `fn with_value<R>(&self, key, f: impl FnOnce(&[u8]) -> R) -> Option<R>`. Which would you put
   in the v2 trait, and why?
3. **Poisoning.** Defend the recover policy for shard locks in two sentences. Name a change to `ShardedStore` that
   would make it wrong.
4. **Shard count.** Why 16? What happens with 1, with 4 on a 32-core machine, and with 4,096? What would you measure to
   choose (Chapter 11.7's Advanced exercise)?
5. **The router.** Why a keyed hash? What breaks if v5 keeps `RandomState` when it partitions keys across machines, and
   what do you replace it with?
6. **Capacity.** With 4 workers, a queue of 16, and clients that each hold a connection for a 30-second session, how
   many clients can the server serve at once, and what does the 21st see? What does the 5th see?
7. **Pipelining.** Why does the flush rule check `reader.buffer().is_empty()`? What would happen to throughput and to
   latency if the loop flushed after every reply? If it never flushed until the buffer was full?
8. **`INCR`.** A client team asks for `INCR key`. Show why it can't be implemented in the client with `GET` + `SET`, and
   design it in the store. Does it belong in the `KvStore` trait, or in a separate trait? What does that choice mean for
   v3 (disk) and v5 (replication)?

---

## 8. Extensions

- **Beginner:** add `EXISTS <key>` (define its reply in the v1 style, for example `+1` / `+0`) and `DBSIZE`, with
  parser tests and new golden lines. Decide whether `DBSIZE` must be exact while writes are running, and document the answer (hint: look at
  `len()`'s comment).
- **Intermediate:** add `MGET k1 k2 ...` returning one reply line per key, in order. Make sure the response for a large
  `MGET` is flushed once. What happens if two keys are in the same shard: do you lock it once or twice, and does it
  matter?
- **Advanced:** add a write timeout and a per-connection cap on unflushed reply bytes, so a client that pipelines without
  reading is disconnected instead of pinning a worker. Test it with a client that sends 100,000 `GET`s and reads
  nothing.
- **Systems:** measure `RwLock` vs `Mutex` shards for `GET`s of 16-byte, 1 KiB, and 64 KiB values with 8 threads
  (reuse `main`'s in-process harness). Where does `RwLock`'s parallel copying start to pay? Does the answer match §3's
  prediction?
- **Architecture:** implement `INCR` as a store method that runs the read-modify-write under one write lock
  (`fn update(&self, key, f: impl FnOnce(Option<&[u8]>) -> Option<Vec<u8>>)`). Then write the one-page design note for
  how `update` will work in v3 (WAL record for the result, not the function) and in v5 (the leader applies it, and
  followers receive the result). Why must the *result*, not the closure, be what's logged and replicated?
