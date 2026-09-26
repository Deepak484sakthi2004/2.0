# Chapter 13.5 — Backpressure, Timeouts, and Load Shedding

> **Where this sits:** Part XIII · Tokio and Production Async · chapter 5 of 5
> **Prerequisites:** Chapters 13.1–13.4. Chapter 8.4 (error classes, retries, idempotency, deadline propagation),
> 11.5 (bounded channels), 12.1 (sockets and readiness).
> **After this chapter you can:** follow backpressure from a slow consumer down through TCP's window to a parked task,
> and explain why UDP has none; name every queue on a request's path and give each a bound and a policy; show with a
> simulation why an unbounded queue under overload does most of its work for nobody; compose limit, shed, and timeout
> middleware in the right order with `tower`; and replace per-hop timeouts with one propagated deadline.

---

## Pass 1 · User level — *Saying "not now" on purpose*

### 1. Problem

Every service eventually receives more work than it can do: a traffic spike, a slow dependency, a retry storm, a
deploy that halves capacity for a minute. What it does then decides whether the incident lasts a minute or an hour.

A service can do three things with excess work: **make the sender wait** (backpressure), **refuse it** (shedding), or
**accept it and do it later** (queueing). The third is what happens by default, because every layer has a queue: the
kernel's accept backlog, socket buffers, your channels, the runtime's task queues, the blocking pool's queue. Queueing
feels safe ("we didn't drop anything") and is the most dangerous of the three, because a queue in front of an overloaded
server fills with requests whose clients have already given up. The server then spends its capacity on work nobody
will receive, which keeps the queue full after the spike is gone.

Chapter 8.4 introduced the error classes and retry rules for this. This chapter is the mechanism: where the queues are
in a Tokio service, how pressure travels through them, and how to bound each one.

### 2. Mental model

**Every queue needs a bound, and every bound needs a policy.**

```text
 client ──► [kernel accept queue] ──► [socket receive buffer] ──► your read loop ──► [channel / semaphore] ──► worker ──► downstream
              backlog (128 in             rcvbuf (autotuned)         FramedRead         capacity or permits      your code   its own queues
              tokio's bind)               TCP window closes          max_line           policy: wait / shed      deadline    and timeouts
              policy: kernel refuses      policy: sender waits                          / drop-oldest

 replies ◄── [socket send buffer] ◄── your write loop
              sndbuf (autotuned)           max_unflushed, write timeout
              policy: write() waits        policy: disconnect slow readers
```

For each queue, answer three questions: **how big** (Little's law: items in queue = arrival rate × time in queue, so
pick the time you're willing to hide), **what happens when it's full** (wait, refuse, drop the oldest, degrade), and
**who notices** (a metric, a log line, the client).

And a rule for time: **a request has one deadline, not one timeout per hop.** A timeout says "I'll wait this long".
A deadline says "the answer is useless after this instant", and every layer can compute its remaining budget from it
and refuse work it can't finish in time.

### 3. Rust code

**TCP pushes back all the way down.** Listing `ch05-01-tcp-backpressure.rs` connects a writer to a server that accepts
the connection and then doesn't read. The writer writes 64 KiB chunks until a write makes no progress for 200 ms (real
loopback TCP, release, one run):

```text
peer not reading: the kernel accepted 2625024 bytes (2.5 MiB), then write() stopped completing
client SO_SNDBUF = 2626560 bytes
server SO_RCVBUF = 131072 bytes
after the consumer resumed: 11013632 bytes received in total (2625024 + 8388608)
```

Nobody wrote any backpressure code. The kernel buffered ~2.5 MiB (the sender's autotuned send buffer plus whatever the
receiver's buffer and the window allowed), then `write` returned `EAGAIN`, Tokio's `write` future returned `Pending`,
and the task parked until the socket became writable again [OS][LIB]. When the consumer started reading, the window
reopened and the parked writer continued: all 11,013,632 bytes arrived. **A Tokio task writing to a slow peer waits
without a thread.** It still holds its buffers and its kernel socket memory, which §5 counts.

**UDP doesn't.** Listing `ch05-05-udp-no-backpressure.rs` sends 20,000 datagrams of 1,000 bytes over loopback to a socket
that isn't reading yet, then drains it:

```text
sent datagrams of [5, 1500, 9000] bytes; recv into a 2048-byte buffer returned [5, 1500, 2048]
receiver SO_RCVBUF = 212992 bytes
sent 20000 datagrams without waiting; received 92; dropped silently 19908
```

Every `send` completed immediately, and **92 of 20,000** datagrams survived. The kernel dropped the rest because the
receive buffer was full, and neither side was told. The first line shows UDP's other contract: one `send` is one
datagram is one `recv`, and a datagram bigger than the receive buffer is **truncated**, not split (the 9,000-byte
datagram came back as 2,048 bytes). UDP protocols that need flow control build it themselves (QUIC does, Chapter 21.2),
and UDP services that don't (metrics, logs) are designed to lose data under load.

**A bounded channel makes the producer wait; `try_send` and a semaphore let it refuse.** Chapter 13.3's listings showed
both: `send().await` completing at the consumer's pace (listing `ch03-01`), and `try_acquire` returning `NoPermits` so
the handler can reply "busy" at once (listing `ch03-06`). Those two primitives are the building blocks for everything
that follows.

---

## Pass 2 · Systems level — *What overload does to a queue*

### 4. Under the hood

**A simulation of overload.** Listing `ch05-02-metastable.rs` models a server that handles one request at a time, 10 ms
each (100 requests/s), receiving 120 requests/s for 10 s from clients that give up after 500 ms. It runs on a paused
clock, so every number is exact and repeatable. Three policies:

```text
unbounded queue                      goodput  294   shed   0   work done 1200 (wasted  906)   skipped   0   p50 ok latency  255ms   server busy until 12.0s
bounded queue (20), shed the rest    goodput 1020   shed 180   work done 1020 (wasted    0)   skipped   0   p50 ok latency  205ms   server busy until 10.2s
unbounded, skip if it can't finish   goodput 1049   shed   0   work done 1049 (wasted    0)   skipped 151   p50 ok latency  493ms   server busy until 10.5s
```

**Goodput** is the number of replies that reached a client that was still waiting.

- **Unbounded queue: 294 useful replies out of 1,200 requests, while doing all 1,200 units of work.** The server was 100%
  busy the whole time, and **906 units of work (75%) produced replies for clients that had already left.** The queue
  grew by 20 requests per second, so after ~5 s every request waited more than 500 ms, and from then on nearly every
  reply was wasted. The server kept working for 2 s after the load stopped, draining a queue of dead requests. That's
  a **metastable failure**: the overload ended at 10 s, and a real system (whose clients retry) would stay overloaded,
  because the queue itself now causes the timeouts that cause the retries.
- **Bounded queue of 20, shed the rest: 1,020 useful replies, zero wasted work.** 180 requests got an immediate "busy"
  (cheap for the server, actionable for the client: retry later or elsewhere). Everything admitted was served within
  its deadline (p50 205 ms, bounded by 20 × 10 ms of queue). The server was idle at 10.2 s.
- **Unbounded, but skip requests that can't finish in time: 1,049 useful replies, zero wasted work.** The server
  checked each request's deadline before working on it and skipped 151 that would have finished too late. Goodput is
  slightly higher than with shedding (the queue absorbed bursts), but the median admitted request waited **493 ms**,
  right at the edge of the client's patience. And those 151 clients waited the full 500 ms to learn nothing.

The lesson isn't "never queue". It's that a queue must be **bounded by time**, either by capacity (a short queue can
only hide a short delay) or by deadlines (work that can't finish in time is dropped before it's done). Shedding at the
door is the cheapest way to do both, and it gives clients the fastest possible "no".

**Layer order is policy.** `tower` (on the Playground) packages these policies as middleware: `load_shed`,
`concurrency_limit`, `timeout`, `buffer`, `rate_limit`. A tower `Service` has two steps: `poll_ready` ("can you take a
request?") and `call` ("here it is"). The order of layers decides which waits are timed. Listing
`ch05-03-tower-stack.rs` sends five requests at once (four of 30 ms, one of 80 ms) through four stacks, each with a
concurrency limit of 2 and a 50 ms timeout, on a paused clock:

```text
load_shed -> limit(2) -> timeout(50ms):              ["ok(30ms)@30ms", "ok(30ms)@30ms", "shed@0ms", "shed@0ms", "shed@0ms"]
timeout(50ms) -> limit(2):                           ["ok(30ms)@30ms", "ok(30ms)@30ms", "ok(30ms)@60ms", "timeout@80ms", "ok(30ms)@90ms"]
limit(2) -> timeout(50ms):                           ["ok(30ms)@30ms", "ok(30ms)@30ms", "ok(30ms)@60ms", "timeout@80ms", "ok(30ms)@90ms"]
timeout(50ms) -> buffer(16) -> limit(2):             ["ok(30ms)@30ms", "ok(30ms)@30ms", "timeout@50ms", "timeout@50ms", "timeout@50ms"]
```

- **Shed first:** the limit's `poll_ready` returns "not ready" for requests 3–5, and `load_shed` turns "not ready" into
  an immediate `Overloaded` error at 0 ms. Two requests are served, three refused at once.
- **Timeout outside the limit, or inside it: the same result.** A `ConcurrencyLimit` waits for a permit in
  `poll_ready`, and `Timeout` only times `call`. So request 3 waited 30 ms for a permit *untimed*, then got its full
  50 ms. Request 4 (the 80 ms one) timed out at 80 ms: 30 ms waiting plus 50 ms of call. A timeout that doesn't include
  queueing time isn't a latency bound on what the client experiences.
- **`buffer` moves the wait into the call.** `Buffer` accepts every request in `poll_ready` (up to its capacity) and
  queues it for a background task that waits for the inner permit, so the permit wait now happens inside the
  `call` future, where the outer timeout sees it. Requests 3–5 all time out at 50 ms: queueing counts against the
  budget. Whether that's what you want is a policy decision, and the stack's order is where you make it.

**Deadline propagation.** Listing `ch05-04-deadline-budget.rs` models `client → gateway → payments-core → processor`
with a 300 ms client budget. Each hop runs as its own task (like a separate service, a caller that gives up doesn't stop
the callee), the gateway spends 50 ms, payments-core spends 60 ms, and the processor needs 300 ms:

```text
fixed per-hop timeouts:
  client got Err("client: gave up waiting") after 300ms
  processor work done after the client had given up: 110 ms
deadline propagated:
  client got Err("processor refused at once: 190ms left, needs 300ms") after 110ms
  processor work done after the client had given up: 0 ms
```

With fixed timeouts (gateway 280 ms, payments-core 250 ms), every hop started its work as if it had its whole timeout,
the client gave up at 300 ms, and the processor kept working for another 110 ms on an answer nobody would receive. With
the client's **deadline** passed down, the processor saw 190 ms left against 300 ms of work and refused immediately:
the client got a definite answer in 110 ms instead of an ambiguous one in 300 ms, and the processor wasted nothing.

### 5. Memory

**Queues are memory, in the kernel too.** Listing `ch05-01`'s single slow peer held **2.5 MiB of kernel memory** in its
send buffer. The kernel autotunes socket buffers up to limits in `net.ipv4.tcp_wmem` / `tcp_rmem` [OS], and on a
loopback connection with a stalled reader it filled them. Multiply by connections: 100,000 connections × even 64 KiB of
send buffer is 6.4 GB of kernel memory that doesn't show up in the process's heap profile. A server with many slow
clients can run the *kernel* out of socket memory (`tcp_mem`), and then every connection on the machine stalls. Bounds
for connection-heavy servers therefore include `SO_SNDBUF` caps (set with `socket2`, as listing `ch05-01` reads them),
an application-level cap on unflushed bytes per connection, and a write timeout that disconnects clients who stop
reading. Project L5 implements the last two.

**Channel memory tracks contents** (Chapter 13.3 §5): a bounded channel sized for 30 seconds of backlog holds 30 seconds
of messages during an incident, however cheap it is when empty. Size it from the time you're willing to hide.

**Waiting tasks hold their requests.** A task parked on a full channel or a semaphore still owns its future: the parsed
request, its buffers, its permits for other resources. 50,000 requests waiting for a 20-permit semaphore are 50,000
requests in memory. Shedding (`try_acquire`, `try_send`) frees them at once.

### 6. CPU / OS

**The first queue is the kernel's.** How long is it? Listing `ch05-06-listen-backlog.rs` reads the answer from the
sources: Tokio's `TcpListener::bind` calls `mio::net::TcpListener::bind` (tokio `listener.rs:123`), and mio 1.2.3 calls
`listen()` with a backlog of **128**, "the same backlog value as the standard library" (mio `listener.rs:85–94`) [LIB].
The kernel caps any backlog at `net.core.somaxconn` [OS]. For a server that must absorb connection bursts, build the
listener with `tokio::net::TcpSocket` and call `.listen(4096)` yourself.

What a full backlog does is stranger than "connection refused". Listing `ch05-07-backlog-burst.rs` fires 1,000
simultaneous connects at an accept loop that does nothing but accept, with a backlog of 128 and then 2,048 (release,
two runs, each observed for at most 4.5 s):

```text
backlog   128: client view: all 1000 connect() calls returned, slowest    1.0s | server view: accepted within 0.5 s  243, 1.5 s  373, 4.5 s  473
backlog  2048: client view: all 1000 connect() calls returned, slowest   8.2ms | server view: accepted within 0.5 s 1000, 1.5 s 1000, 4.5 s 1000
```

(The second run gave 609 / 746 / 938 accepted for backlog 128, with the slowest `connect()` at 2.1 s: the numbers vary,
the shape doesn't.) With a backlog of 128, **every client's `connect()` succeeded**, yet after 4.5 s the server had
accepted only half of them in the first run. When the accept queue is full, Linux drops incoming SYNs (the client
retransmits after ~1 s, then ~2 s more) and drops the handshake's final ACK (the client already considers itself
connected, and the server retransmits its SYN-ACK later) [OS]. So a client can send its first request into a connection
that the server won't `accept()` for seconds. Nothing reports an error; it shows up as latency in whole seconds. With a
backlog of 2,048, all 1,000 were accepted within half a second. Keep the accept loop doing nothing but accepting and
handing off (Project L5's loop spawns and returns to `accept` at once), and size the backlog for your connection bursts.

**Refusing is cheap; working is not.** In listing `ch05-02`, a shed request cost the server nothing (a counter check) and
a served one cost 10 ms. In real services the ratio is similar: rejecting with a short error costs microseconds, and
serving costs milliseconds. That asymmetry is why shedding restores goodput: capacity goes to requests that can still
succeed.

**Timers are cheap enough to use everywhere.** A `timeout` is a `Sleep` in the timer wheel: an intrusive insertion, no
allocation (Chapter 13.1 §5), and it's cancelled (removed) when the inner future finishes first. Wrapping every
network call in a timeout costs tens of nanoseconds. There's no performance reason to leave any `.await` on the network
unbounded.

---

## Pass 3 · Architect level — *An overload policy for every queue*

### 7. Trade-offs

**Policies for a full queue:**

| Policy | Mechanism in Tokio | Latency for admitted work | Wasted work | Good for |
|---|---|---|---|---|
| Wait (backpressure) | `send().await`, `acquire().await`, TCP | grows with the queue | none, if the waiter's deadline is honored | internal pipelines, where the producer can slow down |
| Shed at the door | `try_send`, `try_acquire`, `load_shed` | bounded | none | request/response services with retrying clients |
| Drop oldest | `broadcast` (lagging receivers), a ring buffer | bounded | the dropped items' work | telemetry, live updates, "latest wins" data |
| Skip if it can't finish | check the deadline before working (listing `ch05-02`) | up to the deadline | none | batch-like work with deadlines |
| Degrade | serve a cheaper answer (cached, partial) | low | none | read paths with acceptable stale answers |
| Unbounded | `unbounded_channel`, the blocking pool's queue | unbounded | most of it, under overload | only when the producer is bounded by construction |

**Timeouts vs deadlines:**

| | Per-hop timeouts | One propagated deadline |
|---|---|---|
| Configuration | a number per call site, drifting apart | one budget per request type, at the edge |
| Wasted downstream work | yes: callees keep working after callers leave | no: callees refuse or stop when the deadline passes |
| Failure answer | ambiguous "timed out" | often definite ("refused: not enough time left") |
| Needs | nothing | a header or field (gRPC's `grpc-timeout`, an `x-deadline` header), synchronized clocks or relative budgets |

**The order of tower layers** (outermost first, as `ServiceBuilder` reads):

| Want | Stack |
|---|---|
| refuse immediately when saturated | `load_shed` → `concurrency_limit` → `timeout` |
| bound *total* time, including queueing | `timeout` → `buffer` → `concurrency_limit` |
| bound only service time, queue without limit | `timeout` → `concurrency_limit` (usually a mistake: queueing is untimed) |

> **Why not just set a big timeout and a big queue, to be safe?** Because "safe" there means "never refuses", and a
> service that never refuses converts overload into latency for everyone, then into timeouts, then (through retries)
> into more load. Listing `ch05-02`'s unbounded row is that system: 100% busy, 25% useful. Small queues and early
> refusals look less safe on a dashboard and are much safer in an incident.

### 8. Java comparison

| Java / JVM ecosystem | Tokio / Rust |
|---|---|
| Netty `Channel.isWritable()` with high/low water marks | an unflushed-bytes cap + write timeout (Project L5) |
| Reactive Streams `request(n)` | bounded channel capacity; `poll_ready` in tower |
| Resilience4j `Bulkhead` (semaphore or thread pool) | `Semaphore`, tower `ConcurrencyLimit` |
| Resilience4j `TimeLimiter`, `RateLimiter` | `tokio::time::timeout`, tower `Timeout`, tower `RateLimit` |
| Tomcat `maxThreads` + `acceptCount` | worker/permit limits + the listen backlog |
| gRPC-Java deadlines (`Context`, propagated automatically) | an explicit deadline passed in each request (on the wire, gRPC carries the remaining budget in the `grpc-timeout` header) |
| `ThreadPoolExecutor` with `CallerRunsPolicy` / `AbortPolicy` | `send().await` (the caller waits) / `try_send` (refuse) |

> **Analogy limit.** In thread-per-request Java, the thread pool *is* the concurrency limit: when all 200 Tomcat threads
> are busy, new connections wait in `acceptCount` and then get refused, so overload has a bound whether you designed one
> or not. An async server has no such accidental limit: 100,000 tasks are cheap, so without an explicit `Semaphore` it
> admits everything, and the overload appears downstream (a database with 50 connections, a processor with a rate
> limit). Async removes the thread limit, and with it the only backpressure many Java services ever had.

### 9. Production scenario

**`merchant-notify` admission control.** Chapter 12.6 §10 ended with a to-do list after the May 2026 handshake incident:
admission control and a handshake budget. The design the team shipped, one policy per queue:

| Queue | Bound | Policy when full | Signal |
|---|---|---|---|
| Kernel accept backlog | `TcpSocket::listen(4096)` (bind's default is 128), `somaxconn` raised to match | kernel drops SYNs | `ListenOverflows` counter from `/proc/net/netstat` |
| Connections per pod | `Semaphore(110,000)` | reply "retry-after 5–15 s (jittered)" and close | `connections_refused_total` |
| Handshakes in progress | `Semaphore(4)` around the `spawn_blocking` password hash (the May fix) plus a 200-handshake wait queue | beyond the queue: "retry-after" | handshake wait time histogram |
| Per-connection outbound events | `mpsc(256)` per connection | drop the connection's queue and send a "resync" marker; the client refetches state | `slow_consumer_resyncs_total` |
| Per-connection unflushed bytes | 256 KiB app cap, `SO_SNDBUF` 64 KiB, 10 s write timeout | disconnect the slow reader | `slow_reader_disconnects_total` |
| Upstream event bus | `broadcast(4,096)` per merchant | lagging subscribers get `Lagged` and resync | `lagged_total` |

Two principles run through the table: **each bound is a time** (256 events at a busy merchant's peak rate is about 2 s of
backlog), and **each refusal tells the client what to do** (retry later with jitter, or resync). A reconnect storm now
hits the connection semaphore and the handshake queue, and gets spread out over 5–15 s, instead of reaching the password
hash.

### 10. Failure scenario

**The processor slowdown that outlived itself (payments-core, 2026, fictional).** `payments-core` called the card
processor with a fixed 2 s timeout, and the gateway called `payments-core` with a 2.5 s timeout. Clients (the checkout
frontend) gave up after 3 s and retried once. Between the gateway and `payments-core` there was an unbounded channel
feeding a pool of processor-call tasks, "so bursts don't get rejected".

One afternoon the processor's latency rose from 150 ms to 1.8 s for about 4 minutes. The queue in front of the
processor calls grew, and after about a minute every charge waited in it for longer than the client's 3 s. From then on
it was listing `ch05-02`'s unbounded row at production scale. `payments-core` kept authorizing charges whose checkout
pages had already shown "please try again". Each retry added a second authorization request for the same checkout
(safe only because of idempotency keys, Chapter 8.4, which is why no customer was double-charged), and the queue held
~40,000 requests at its peak. When the processor recovered, `payments-core` needed another **11 minutes** to drain
requests whose clients were long gone, and real traffic waited behind them. A 4-minute dependency slowdown became a
15-minute outage.

The fix, in this chapter's terms:

1. **Deadline propagation.** The gateway sets a deadline header from the client's budget (3 s from the edge), and every
   hop computes its remaining time. `payments-core` refuses a charge with less than 400 ms left (the processor's p99)
   with a definite `PAY_DEADLINE_EXCEEDED`, before calling the processor. Listing `ch05-04` shows the effect: a definite
   answer early instead of an ambiguous one late, and no work for departed clients.
2. **A bounded queue with shedding** in front of the processor calls: 200 slots (about 1 s of processor throughput),
   `try_send` beyond that, and a 503 with `Retry-After` for the shed request.
3. **Skip-if-expired at dequeue**: a request whose deadline passed while it waited is dropped before the processor
   call (listing `ch05-02`'s third policy), with a metric.
4. **A retry budget at the gateway** (Chapter 8.4): retries capped at 10% of requests.

In the next processor slowdown (July 2026), the shed rate rose to 30% for the duration, p99 for admitted charges stayed
under 2.2 s, and recovery was immediate when the processor recovered.

---

## Practice

### 11. Interview & architecture questions

1. Trace backpressure from a consumer that stops reading a TCP socket back to the producing task. Which parts are the
   kernel, which are Tokio, and where does the task wait?
2. Why does UDP have no backpressure, and what did listing `ch05-05` measure? How do QUIC and a metrics pipeline each
   deal with it?
3. Explain listing `ch05-02`'s unbounded row: why did 75% of the work produce nothing, and why does the server stay
   busy after the load stops?
4. When is waiting (backpressure) the right overload policy, and when is shedding?
5. In listing `ch05-03`, why did `timeout → limit` and `limit → timeout` behave the same? What changed with `buffer`?
6. What's the difference between a timeout and a deadline? Why do per-hop timeouts waste work downstream?
7. How big should a queue be? Use Little's law with a concrete example.
8. Where are the hidden queues in an async server that has no explicit channel at all?
9. A Java service on Tomcat never had an overload problem, and its Rust rewrite on Tokio does. Explain why.
10. What does a client need in a "busy" response to make shedding helpful rather than harmful?

### 12. Exercises

- **Beginner.** Change listing `ch05-02`'s bounded queue from 20 to 5 and to 100. Record goodput, shed count, and p50
  latency for each, and explain the trend with Little's law.
- **Intermediate.** Add a fourth policy to listing `ch05-02`: a bounded queue of 20 **and** skip-if-expired at dequeue.
  Compare it with the two policies it combines.
- **Advanced.** Add a `rate_limit` layer to listing `ch05-03` and find the order in which rate limiting, shedding, and
  the timeout give "refuse immediately when above 50 req/s, and bound total latency at 100 ms".
- **Systems.** Rerun listing `ch05-01` with `SO_SNDBUF` set to 64 KiB on the client (via `socket2::SockRef`) before
  writing. How much does the kernel accept now? What's the per-connection kernel memory bound for 100,000 such
  connections?
- **Architecture.** Draw the complete queue map of Ferrite v2 (Project L5): every queue from the client's socket to the
  store and back, with its bound, its policy, and the metric that exposes it. Mark the one queue that is still unbounded.

### 13. Debugging exercise

A service's p99 is fine at 80% load and explodes at 95%, but CPU never exceeds 70%. Its handler:

```rust,ignore
// Debugging exercise: find the defects (not a verified listing)
static DB_PERMITS: Semaphore = Semaphore::const_new(50);   // the DB pool has 50 connections

async fn handler(req: Request) -> Response {
    let _permit = DB_PERMITS.acquire().await.unwrap();     // (a)
    let user = tokio::time::timeout(Duration::from_secs(2), db.load_user(req.user_id)).await; // (b)
    let prices = pricing_client.quote(&req.items).await;   // (c) remote service, ~40 ms
    render(user, prices)
}
```

Identify the queue that grows at 95% load, why CPU stays low, and why the timeout at (b) doesn't bound what the client
sees. Rewrite the handler with a deadline, a bounded wait for the permit, and the permit held only around the database
call. Model answer in Appendix A.

### 14. Design exercise

Design the overload policy for Meridian's **API gateway** (Chapter 1.2: 400K req/s peak, ~120 upstreams, per-key rate
limits) in its Rust form. For every queue from the client connection to the upstream call (accept backlog, connection
count, per-connection requests, per-upstream concurrency, retries), give its bound (with the Little's-law arithmetic),
its policy when full, the response the client gets, and the metric that shows it. Then describe how a 4-minute slowdown
of one upstream (5% of traffic) affects the other 119, and what in your design guarantees that.
