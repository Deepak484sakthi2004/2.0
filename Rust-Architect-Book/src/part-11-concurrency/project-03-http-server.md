# Project Level 3 — A Multithreaded HTTP Server from Raw TCP

> **Where this sits:** Part XI · the third rung of the project ladder (a network server)
> **Uses:** threads and pools (11.1), `Send`/`Sync` (11.2), `Mutex` + `Condvar` and poisoning (11.3), atomics (11.4),
> admission control (11.5), plus Part VIII's panic boundaries and Part II's discipline about limits and exit paths.
> **Full source:** `listings/part-11/project-03-http-server.rs` (about 660 lines, no dependencies), verified with rustc
> 1.98.1: it **runs in debug and release, and all 5 tests pass**. `main` starts the server on `127.0.0.1` and drives it
> with in-process clients over real TCP sockets.

Every Java backend engineer has used an HTTP server; few have built one. This project builds a small but honest
HTTP/1.1 server on nothing but `std::net::TcpListener`. It isn't meant to replace hyper (Chapter 22.3). It's meant to
make every production property of a server visible in code you wrote: **how many requests can be in flight, what happens
to the next one, how a bug in one handler is contained, how the server stops, and what a hostile client can do to it.**

The worker pool built here is reused unchanged by Ferrite v1 in the next project.

---

## 1. Requirements

| Area | Requirement |
|---|---|
| Protocol | HTTP/1.0 and HTTP/1.1 requests; `Content-Length` bodies; keep-alive by the version's default and the `Connection` header |
| Routes | `GET /health`, `GET /hello?name=`, `POST /echo`, `GET /slow?ms=` (for demos), `GET /panic` (a deliberate handler bug) |
| Errors | `400` malformed, `404` unknown path, `405` wrong method, `413` body too large, `431` headers too large, `500` handler panic, `501` chunked bodies, `503` overloaded, `505` other HTTP versions |
| Limits | Request line and each header ≤ 8 KiB; ≤ 64 headers and ≤ 16 KiB of headers; body ≤ 1 MiB; ≤ 100 requests per connection |
| Concurrency | A fixed pool of worker threads; a **bounded** queue of accepted connections; refuse with `503` when full |
| Timeouts | Read and write timeouts per socket operation (500 ms in the demo) |
| Robustness | A panicking handler produces a `500`; the connection, the worker, and the server continue |
| Shutdown | Stop accepting, finish queued and in-flight connections, join every worker, report counts |

**Non-goals** (see §6): TLS, HTTP/2, chunked request bodies, async I/O, routing frameworks.

---

## 2. Design

```text
            ┌──────────────── accept thread (Server::run) ─────────────────┐
 clients ──►│ TcpListener::incoming()                                        │
            │   set read/write timeouts                                      │
            │   pool.try_submit(stream) ──Err(stream)──► refuse(): 503, close│
            └───────────────┬────────────────────────────────────────────────┘
                            │ Ok: the TcpStream MOVES into the queue
                            ▼
            ┌─ WorkerPool<TcpStream> ──────────────────────────────────────┐
            │ Mutex<Queue { VecDeque<TcpStream>, closed }> + Condvar         │
            │ capacity = queue bound (the admission policy)                  │
            └───┬──────────────┬──────────────┬──────────────┬──────────────┘
                ▼              ▼              ▼              ▼
             worker 0       worker 1       worker 2       worker 3      (named "http-0".."http-3")
             handle_connection(stream):  loop { read_request → catch_unwind(route) → write response }
                                         until close, error, timeout, or the per-connection request cap
```

**Ownership decisions:**

| Value | Owner | Shared how | Why |
|---|---|---|---|
| `TcpListener` | `Server` (the accept thread) | Not shared | Exactly one thread accepts |
| Each `TcpStream` | The accept thread, then the queue, then **one** worker | Moved, never shared | A connection is served by exactly one thread at a time: no locks on the socket |
| The queue | `Arc<Shared>` inside the pool | `Mutex` + `Condvar` | The only mutable state shared by threads |
| `Config` | `Arc<Config>` | Read-only | Workers read limits and timeouts |
| Counters (completed, panicked, rejected, idle timeouts) | Atomics | `Relaxed` increments | Metrics only; `join` orders the final reads (11.1) |
| Shutdown flag | `Arc<AtomicBool>` | `SeqCst` store/load | Read by the accept loop after each wakeup |

The central choice is **one worker per connection, with a hard cap**. It's the simplest model that is still bounded:
at most `workers` connections are served and at most `capacity` wait. Everything else is refused immediately. The
price, measured in §4 and discussed in §5, is that an idle keep-alive connection occupies a whole thread until its
timeout. That price is exactly what Part XIII's async version removes.

---

## 3. Implementation walkthrough

### The pool: a bounded queue that hands refused work back

The pool is Chapter 11.3's `Condvar` queue plus three production details: a non-blocking `try_submit`, panic
containment, and shutdown through `Drop` (listing `project-03-http-server.rs`, excerpts):

```rust,ignore
/// Queue `item` if there is room. If not, hand it back: the caller decides how to refuse.
pub fn try_submit(&self, item: T) -> Result<(), T> {
    let mut q = self.shared.queue.lock().unwrap();
    if q.closed || q.items.len() >= self.shared.capacity {
        self.shared.stats.rejected.fetch_add(1, Ordering::Relaxed);
        return Err(item);
    }
    q.items.push_back(item);
    drop(q); // unlock first: the woken worker can take the lock at once
    self.shared.item_ready.notify_one();
    Ok(())
}
```

`Result<(), T>` is the important part of the signature. A refused connection comes **back** to the accept loop, which
still owns it and can answer `503` on it. With an API that took ownership and dropped refused items, the client would
see a silently closed socket. Chapter 11.5's `try_send` makes the same choice for the same reason.

```rust,ignore
fn worker_loop<T, H: Fn(T)>(shared: &Shared<T>, handler: &H) {
    loop {
        let item = {
            let q = shared.queue.lock().unwrap();
            let mut q = shared.item_ready.wait_while(q, |q| q.items.is_empty() && !q.closed).unwrap();
            match q.items.pop_front() {
                Some(item) => item,
                None => return, // closed and drained: this worker is finished
            }
        }; // the queue lock is released BEFORE the handler runs: a panic can't poison it
        match panic::catch_unwind(AssertUnwindSafe(|| handler(item))) {
            Ok(()) => shared.stats.completed.fetch_add(1, Ordering::Relaxed),
            Err(_) => shared.stats.panicked.fetch_add(1, Ordering::Relaxed), // the worker lives on
        };
    }
}
```

Two details carry the design:

- **The lock's scope ends before the handler runs.** The block expression returns the item and drops the guard. A
  handler that panics, or takes seconds, never holds the queue lock, so it can't poison it (Chapter 11.3) and can't
  stall other workers taking work.
- **`catch_unwind` around every item keeps the worker alive** (the Part VIII promise: a pool that survives job panics).
  `AssertUnwindSafe` is justified because nothing the handler touches is observed after a panic except the counters,
  which are atomics. The item itself (the connection) is dropped during unwinding, which closes the socket. This
  requires `panic = "unwind"`. With `panic = "abort"` (Chapter 2.2), a handler bug ends the process, and the design
  would need process-level supervision instead.

**Shutdown is `close` + `notify_all` + `join`**, run by both `shutdown(self)` and `Drop`:

```rust,ignore
/// Refuse new items, let the workers drain the queue, wait for all of them.
fn close_and_join(&mut self) {
    self.shared.queue.lock().unwrap().closed = true;
    self.shared.item_ready.notify_all();
    for w in self.workers.drain(..) {
        let _ = w.join();
    }
}
```

`wait_while`'s predicate is `items.is_empty() && !closed`, so after `close` a worker keeps taking items until the queue
is empty and only then returns. Queued connections are **served, not dropped**. `workers.drain(..)` empties the vector,
so the `Drop` that runs after an explicit `shutdown()` finds nothing left to join. It's the Chapter 11.1 lesson (join
what you spawn) turned into a type.

### Parsing with limits

Every read is bounded before it happens. The helper reads at most `limit + 1` bytes of a line, so an 8 KiB limit can't
be bypassed by a client that never sends `\n`:

```rust,ignore
/// Reads one line (up to and including b'\n') but never more than `limit` bytes.
fn read_line(r: &mut impl BufRead, limit: usize, buf: &mut Vec<u8>) -> Result<usize, ParseError> {
    buf.clear();
    let n = r.by_ref().take(limit as u64 + 1).read_until(b'\n', buf).map_err(ParseError::Io)?;
    if n > limit {
        return Err(ParseError::HeadersTooLarge);
    }
    // ...
}
```

The parser reuses one line buffer for the request line and every header, the same allocation discipline as Project L1.
That produced this project's one borrow-checker lesson. The first version kept `method` and `target` as `&str` slices
of the request line, and the header loop then reused the buffer: **E0502**, a shared borrow of `line` still live while
`read_line` needed `&mut line`. The fix copies the two small strings out before the loop:

```rust,ignore
// `method` and `target` borrow `line`, which the header loop reuses: copy them out first (E0502 otherwise).
let (method, target) = (method.to_string(), target.to_string());
```

It's Chapter 4.6's error-as-proof in miniature. The compiler proved that the header loop would overwrite the bytes that
`method` pointed into, which in C would have been a silent corruption bug.

`ParseError` separates *what went wrong* from *what to send*. `Closed` and `Io` (timeouts, resets) get no response,
while protocol errors map to one status each:

```rust,ignore
pub fn response(&self) -> Option<Response> {
    Some(match self {
        ParseError::Closed | ParseError::Io(_) => return None,
        ParseError::BadRequest(why) => Response::text(400, "Bad Request", why),
        ParseError::HeadersTooLarge => Response::text(431, "Request Header Fields Too Large", "headers too large"),
        ParseError::BodyTooLarge => Response::text(413, "Content Too Large", "body too large"),
        ParseError::VersionNotSupported => Response::text(505, "HTTP Version Not Supported", "HTTP/1.x only"),
        ParseError::NotImplemented(what) => Response::text(501, "Not Implemented", what),
    })
}
```

The match is exhaustive, so adding a variant forces a decision about its status code (Chapter 8.4's rule).

### The connection loop: keep-alive, timeouts, panics

`handle_connection`'s loop (excerpt; the error branch is condensed to a comment):

```rust,ignore
for served in 1..=config.max_requests_per_connection {
    let req = match http::read_request(&mut reader, &limits) {
        Ok(req) => req,
        Err(e) => { /* count idle timeouts; send e.response() if any; */ return; }
    };
    // A handler bug becomes a 500 for this request; the connection and the worker carry on.
    let resp = panic::catch_unwind(AssertUnwindSafe(|| crate::app::route(&req)))
        .unwrap_or_else(|_| Response::text(500, "Internal Server Error", "internal error"));
    let keep_alive = req.keep_alive() && served < config.max_requests_per_connection;
    if resp.write_to(&mut writer, keep_alive).is_err() || !keep_alive {
        return;
    }
}
```

There are **two** panic boundaries. The inner one, per request, turns a handler bug into a `500` and keeps the
connection. The outer one, per item in the pool, catches anything that escapes the connection code itself. The socket
is split with `try_clone` into a `BufReader` for parsing and a raw writer. Each response is assembled in one buffer and
written with one `write_all`, so a response never goes out as two TCP segments that a slow client might see half of.

### The accept loop and the shutdown handle

```rust,ignore
for stream in self.listener.incoming() {
    if self.shutdown.load(Ordering::SeqCst) {
        break;
    }
    let Ok(stream) = stream else {
        continue; // e.g. EMFILE, or a connection reset before accept(): keep serving
    };
    let _ = stream.set_read_timeout(Some(self.config.read_timeout));
    let _ = stream.set_write_timeout(Some(self.config.write_timeout));
    if let Err(stream) = self.pool.try_submit(stream) {
        refuse(stream); // the queue is full: answer 503 now instead of queueing forever
    }
}
self.pool.shutdown() // graceful: queued connections are still served, then workers are joined
```

`accept` blocks, and std offers no way to interrupt a blocked `accept` from another thread. The `ShutdownHandle`
therefore stores the flag and then **connects to the server itself**. That wakes `accept`, the loop sees the flag, and
it stops. It's a small, portable trick. Production servers use a non-blocking listener with `poll`/`epoll` (Chapter
12.1) or an async runtime instead.

---

## 4. Testing

**The golden output** is `main`'s scripted session over real localhost sockets (debug and release print the same):

```text
-- basic requests (Connection: close)
HTTP/1.1 200 OK | ok
HTTP/1.1 200 OK | hello, Ada
HTTP/1.1 200 OK | payment=42
HTTP/1.1 405 Method Not Allowed | method not allowed
HTTP/1.1 404 Not Found | not found
-- a handler panics; the server keeps serving
HTTP/1.1 500 Internal Server Error | internal error
HTTP/1.1 200 OK | ok
-- limits and protocol errors
HTTP/1.1 431 Request Header Fields Too Large | headers too large
HTTP/1.1 413 Content Too Large | body too large
HTTP/1.1 505 HTTP Version Not Supported | HTTP/1.x only
HTTP/1.1 400 Bad Request | malformed request line
HTTP/1.1 501 Not Implemented | chunked request bodies are not supported
-- keep-alive: two requests, one connection
HTTP/1.1 200 OK | hello, first
HTTP/1.1 200 OK | hello, second
-- an idle connection holds a worker until the read timeout
server closed the idle connection (read returned 0) after ~500 ms: true
-- overload: 9 slow requests against 4 workers + a queue of 2
200 OK: 6, 503 Service Unavailable: 3
-- graceful shutdown: an in-flight request still completes
in-flight request: HTTP/1.1 200 OK | slept 300 ms
pool: 21 connections handled, 0 handler panics escaped, 3 refused
idle connections closed by the read timeout: 1
```

and on stderr, from the panic hook: `[panic hook] handler bug: index out of range in report builder`.

Read the overload line against the design: **4 workers + 2 queue slots = 6 connections admitted, and the rest refused
immediately.** Nine clients each holding a connection for 400 ms got exactly 6 × `200` and 3 × `503`. The refused
clients got an answer at once instead of waiting in an invisible backlog. That's admission control, and it's
the most important line in the output. "0 handler panics escaped" confirms the inner boundary caught the `/panic` route,
so the outer boundary never fired.

**Unit tests** (5, all passing) cover what the golden run can't isolate:

```text
test tests::keep_alive_rules ... ok
test tests::parses_request_line_headers_and_body ... ok
test tests::workers_survive_panics_and_drop_drains_the_queue ... ok
test tests::limits_and_errors ... ok
test tests::a_full_queue_hands_the_item_back ... ok
```

- `parse` tests run on `Cursor<Vec<u8>>` instead of sockets. The parser takes `impl BufRead`, so no network is needed
  (the same testability choice as Project L1).
- `workers_survive_panics_and_drop_drains_the_queue` submits 10 items, a third of which panic, then **drops** the pool.
  It asserts that exactly the 6 non-panicking items ran, which proves both panic survival and drain-on-drop.
- `a_full_queue_hands_the_item_back` blocks the only worker, fills the only slot, and checks that `try_submit(2)`
  returns `Err(2)`, then that the final counts are `(2 completed, 0 panicked, 1 rejected)`.

---

## 5. Production concerns

- **Slow clients hold workers.** The read timeout applies to *each* `read` call, not to the whole request. A client
  that sends one byte every 400 ms (a "slowloris") never trips a 500 ms timeout and keeps a worker busy indefinitely.
  With 4 workers, four such clients take the server down. The fix is a **per-request deadline** (a start time checked
  between reads), plus a per-IP connection limit at the load balancer. In an async server (Part XIII) a slow client costs
  a few kilobytes instead of a thread, which is the real reason async servers exist.
- **Keep-alive trades latency for capacity.** An idle keep-alive connection occupies a worker until the read timeout
  (the "idle connection" demo held one for ~500 ms). Production thread-per-connection servers keep idle timeouts short
  and cap requests per connection, as here, or they hand idle connections back to a poller that waits on many sockets
  at once. That's the design Part XII builds up to.
- **The 503 path runs on the accept thread.** `refuse` writes with a 100 ms write timeout. A burst of refused clients
  whose receive windows are full could slow `accept` by up to 100 ms each. Production servers make refusal cheaper
  still: a pre-built response, a non-blocking write, and closing on failure.
- **Request smuggling surface.** The parser rejects `Transfer-Encoding` (`501`) and takes the *first* `Content-Length`.
  RFC 9112 requires rejecting messages with conflicting `Content-Length` values, and a proxy in front of this server
  that picked the *last* one would disagree about where the request ends. Parsers that sit behind other parsers must be
  strict (see the exercises).
- **Panics and `AssertUnwindSafe`.** Catching panics is only safe when nothing observed afterwards can be left in a
  broken state. Here that's true by construction: requests share no mutable state. A handler that mutated shared state
  under a lock would also need Chapter 11.3's poisoning policy.
- **Graceful shutdown needs a deadline.** The current shutdown waits for every queued and in-flight connection. A
  keep-alive client could keep a worker for up to 100 requests. Production shutdown stops keep-alive (`Connection:
  close` on the next response), then waits with a deadline, then closes whatever remains.
- **Observability.** The counters are the start of the metrics a real server exports: queue depth, active connections,
  refusals, handler panics, idle timeouts, and per-route latency histograms (Project L1's bounded histogram fits).

---

## 6. Omissions (deliberate)

| Omitted | Why here | Where it's covered |
|---|---|---|
| TLS | Needs `rustls` and certificates; orthogonal to the concurrency design | Chapter 21.3 |
| HTTP/2 and HTTP/3 | Multiplexing changes the unit of work from connection to stream | Chapter 21.2 |
| Chunked request bodies | Rejected with `501`; a streaming body parser is a project of its own | Chapter 21.2 |
| Non-blocking / async I/O | The point of this project is the thread-per-connection baseline | Parts XII–XIII (Ferrite v2) |
| A routing framework, middleware | `match` on (method, path) is enough to show the boundaries | Chapter 22.3 (Tower) |
| Header value validation, `Host` checks, percent-decoding | Parser strictness beyond limits | Exercises; Chapter 21.2 |

---

## 7. Architecture review

*You're the principal engineer reviewing this server before a team adopts its pool for an internal admin API. Model
answers are in Appendix A (Part XI).*

1. **Capacity.** With 4 workers, a queue of 2, and handlers that take 50 ms on average, what's the maximum sustained
   throughput? What happens at 1.5× that load, and at 10×? Where would you put the metrics that tell you which regime
   you're in?
2. **Queue size.** Why is the queue so small (2)? What would a queue of 10,000 change about latency during overload
   (Little's law)? When is a larger queue right?
3. **Slowloris.** Show how four slow clients take the server down, and design the per-request deadline. Which layer
   should also defend against it?
4. **Panic boundaries.** Why are there two? What would break if the pool's `catch_unwind` were removed? If the
   per-request one were removed?
5. **Shutdown.** Walk through shutdown with one in-flight request, two queued connections, and one idle keep-alive
   client. What does each see? Add a drain deadline to the design.
6. **Ownership of the socket.** Why is a `TcpStream` never behind a lock anywhere in this design? What would change if
   responses were produced by a *different* thread from the one reading requests (pipelining to a backend pool)?
7. **Reuse.** The next project reuses `WorkerPool<T>` unchanged for a different protocol. What makes it reusable?
   What would you change before publishing it as a crate (Chapter 22.1)?
8. **Thread-per-connection vs async.** At what number of concurrent, mostly idle connections does this design stop
   being viable, and what exactly runs out first (Chapter 11.1's memory table)?

---

## 8. Extensions

- **Beginner:** add `GET /time` returning the server's uptime, and a `Server:` response header. Update the golden output.
- **Intermediate:** reject requests with more than one `Content-Length` header, or with a `Content-Length` that has a
  sign or leading `+`, with `400`. Add tests for both.
- **Advanced:** add the per-request deadline from §5 (the whole request must arrive within 2 s, whatever the per-read
  timeout), plus a test with a client that sends a byte every 100 ms.
- **Systems:** run the server locally and load it with `wrk -c 200 -t 4 --latency http://127.0.0.1:PORT/health` (not
  verifiable on the Playground). Plot throughput and p99 as you vary workers (4, 16, 64) and queue size (2, 64, 4096).
  Where does each curve bend, and why?
- **Architecture:** put two pools in the server: a small one for `/health` (so health checks answer even when the main
  pool is saturated) and the main one for everything else. What does the accept thread need to know to route a
  connection before reading its request, and why is that hard with HTTP/1.1 keep-alive?
