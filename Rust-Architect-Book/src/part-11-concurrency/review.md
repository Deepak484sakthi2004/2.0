# Part XI Review — The Partner-Quota PR & Interview Mode

> Consolidate Part XI, then use it: review a pull request that compiles, passes its author's test, works for one
> request at a time, and contains more than a dozen concurrency defects. Two of them let partners exceed the quota
> they pay for. Then answer senior-level questions without notes. Answers are in **Appendix A, Part XI**.

---

## Part XI on one page

```text
 THREADS        std::thread = OS thread (clone + 2 MiB mmap + guard page); spawn ~30 µs, hand-off ~5 µs (one run)
                spawn needs 'static (the thread may outlive you); join moves T (or the panic payload) back
                main returning ends the process: join what you spawn; cancellation is cooperative (flags, closed queues)
        │
 TYPES          T: Send  = T may MOVE to another thread;  T: Sync = &T may be SHARED  (T: Sync ⇔ &T: Send)
                auto traits compose from fields; one !Send field (Rc, raw pointer, MutexGuard) poisons the whole type
                checked at spawn/Scope::spawn/rayon/tokio bounds; captures are PLACES since edition 2021
                unsafe impl Send/Sync = a proof obligation; a data race is UB (Miri: "Data race detected")
        │
 SHARING        Mutex<T> owns T: lock() → guard (DerefMut), unlock = Drop; uncontended = lock cmpxchg + xchg
                RwLock: many readers XOR one writer; readers WRITE the reader count; Linux std prefers writers
                  → a nested read behind a waiting writer deadlocks
                poisoning reports a possibly broken invariant: propagate, recover (count + clear), or repair
                Condvar: wait_while(guard, pred) consumes and returns the guard; spurious wakeups allowed
                UnsafeCell is the only legal mutation through &T; Cell/RefCell/OnceLock/LazyLock/atomics build on it
                ArcSwap: lock-free reads (~8 ns vs ~220 ns through a lock); decide WHERE old versions are freed
        │
 MOVING         channels move values (E0382 after send); disconnection = end of stream; Receiver is !Sync (mpsc)
                capacity is an admission policy: ∞ → heap, n → backpressure, 0 → rendezvous; try_send hands v back
                ~18 ns/item when neither side waits, ~7 µs when every item parks a thread; batch tiny items
                scope: threads borrow, joined by CONTROL FLOW (not Drop: the Leakpocalypse); rayon = work stealing
                parallelize milliseconds, not elements; never block inside rayon
        │
 DECIDING       don't share < share immutably < share mutably;  cost = how many cores WRITE the same line, how often
                no sharing 0.3–0.6 ns < padded atomic 3–7 < false sharing 5–23 ≈ shared atomic 9–19 < mutex 26–181
                maps: 16 shards ~75 ns < RwLock 92–154 < Mutex 160–272 ≪ owner thread µs  (4 threads, 95% reads)
                a lock serializes for exactly as long as it's held: decide under the lock, act after it
```

## Ten ideas to carry forward

1. **A thread is a budgeted, long-lived resource**, like a database connection. Create a fixed number, give each a job
   loop, and make the queue bound the explicit control point.
2. **Thread safety is a property of types**, computed from fields and checked at the operation that shares. You can't
   forget to make a type thread-safe. You can only lie about it with `unsafe impl`.
3. **The lock owns the data.** There's no way to touch the data without the lock, and unlocking is a `Drop`. What's left
   to get wrong is *how long* the guard lives.
4. **Poisoning is a question about invariants**, answered per lock and written next to it: propagate when a panic can
   leave the data inconsistent, recover and count when it can't.
5. **A read lock is a write** to a shared counter. For read-mostly data, publish immutable snapshots (`ArcSwap`) or
   shard, and decide with a measurement.
6. **Channels move ownership, and capacity is policy.** Unbounded means "overload becomes heap". Bounded means "overload
   becomes backpressure or refusal", visible where it happens.
7. **Soundness may rely on control flow, never on a destructor running.** That's why `thread::scope` is a closure and
   why `mem::forget` is safe.
8. **Parallel results equal sequential ones only when the merge is exact.** Design the per-thread state (histograms,
   integer sums) so it is.
9. **The cost of sharing is the number of cores writing the same cache line.** "Atomic" isn't one cost, and false
   sharing costs like true sharing.
10. **Decide under the lock, act after it.** I/O, callbacks, logging, and blocking sends inside a critical section turn
    parallel work into serial work, and nested locking turns it into deadlock.

---

## Capstone: review the partner-quota PR

Meridian's gateway enforces per-partner request quotas (Chapter 6.1's `FixedWindow` limiter covered one worker; this is
the shared, process-wide version). A teammate submits the tracker below (listing `review-01-quota-pr.rs`, verified: it
compiles, and its single-threaded test `admits_up_to_the_limit` passes). Every gateway worker thread calls
`try_acquire` for every request. A timer thread calls `reset_window` once a minute. The on-call alert hook is passed in
as `on_exceeded`.

```rust,ignore
pub static ADMITTED_TOTAL: AtomicU64 = AtomicU64::new(0);
pub static BILLING_EVENTS_UPLOADED: AtomicU64 = AtomicU64::new(0);

pub struct QuotaTracker {
    limits: RwLock<HashMap<String, u64>>, // requests per window, per partner
    usage: Mutex<HashMap<String, u64>>,   // requests admitted in the current window
    inflight: Mutex<HashMap<String, u64>>, // requests currently being served, per partner
    events: Sender<String>,               // one usage event per admitted request, for billing
    on_exceeded: Box<dyn Fn(&str) + Send + Sync>,
}

pub struct Slot<'a> {
    tracker: &'a QuotaTracker,
    partner: String,
}

impl QuotaTracker {
    pub fn new(limits: HashMap<String, u64>, on_exceeded: Box<dyn Fn(&str) + Send + Sync>) -> Arc<QuotaTracker> {
        let (tx, rx) = mpsc::channel::<String>();
        // The billing reporter: batches events and uploads every 50.
        thread::spawn(move || {
            let mut batch = Vec::new();
            for event in rx {
                batch.push(event);
                if batch.len() == 50 {
                    thread::sleep(Duration::from_millis(5)); // "upload"
                    BILLING_EVENTS_UPLOADED.fetch_add(50, Ordering::Relaxed);
                    batch.clear();
                }
            }
        });
        Arc::new(QuotaTracker {
            limits: RwLock::new(limits),
            usage: Mutex::new(HashMap::new()),
            inflight: Mutex::new(HashMap::new()),
            events: tx,
            on_exceeded,
        })
    }

    fn limit_for(&self, partner: &str) -> u64 {
        *self.limits.read().unwrap().get(partner).unwrap_or(&100)
    }

    /// Admit one request for `partner` if it is under its quota for this window.
    pub fn try_acquire(&self, partner: &str) -> bool {
        let used = *self.usage.lock().unwrap().get(partner).unwrap_or(&0);
        if used >= self.limit_for(partner) {
            let _usage = self.usage.lock().unwrap(); // a consistent view for the alert
            (self.on_exceeded)(partner);
            return false;
        }
        write_audit(&format!("admit {partner}")); // compliance: every admission is audited
        *self.usage.lock().unwrap().entry(partner.to_string()).or_insert(0) += 1;
        let n = ADMITTED_TOTAL.load(Ordering::Relaxed);
        ADMITTED_TOTAL.store(n + 1, Ordering::Relaxed);
        self.events.send(format!("{partner} +1")).unwrap();
        true
    }

    /// A concurrency slot: at most `max` requests of one partner in flight at once.
    pub fn acquire_slot(&self, partner: &str, max: u64) -> Option<Slot<'_>> {
        let mut inflight = self.inflight.lock().unwrap();
        let n = inflight.entry(partner.to_string()).or_insert(0);
        if *n >= max || self.usage_of(partner) >= self.limit_for(partner) {
            return None;
        }
        *n += 1;
        Some(Slot { tracker: self, partner: partner.to_string() })
    }

    pub fn usage_of(&self, partner: &str) -> u64 {
        *self.usage.lock().unwrap().get(partner).unwrap_or(&0)
    }

    /// Called by a timer thread at the end of each window.
    pub fn reset_window(&self) {
        let mut usage = self.usage.lock().unwrap();
        for (partner, used) in usage.iter() {
            let line = format!("window closed: {partner} used {used}");
            thread::spawn(move || write_audit(&line));
        }
        usage.clear();
        self.inflight.lock().unwrap().retain(|_, n| *n > 0);
    }
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        *self.tracker.inflight.lock().unwrap().get_mut(&self.partner).unwrap() -= 1;
    }
}

fn write_audit(_line: &str) {
    thread::sleep(Duration::from_millis(1)); // an append to the audit volume
}
```

The listing's `main` runs it the way the gateway would: 8 worker threads, 10 requests each, all for partner `acme`,
whose quota is 20 per window. One run printed:

```text
limit 20, attempts 80: admitted 27, usage_of = 27
over-admitted: 7
quota alerts fired: 53
unknown partner 'zeta' admitted: true
concurrency slot for 'zeta': true
after reset_window: usage_of(acme) = 0
billing events uploaded: 0 of 28 admitted
```

The exact over-admission varies from run to run. It's a race, but it isn't rare: five runs of this listing admitted 26
or 27 requests against the quota of 20.

**Your review.**

1. Find **at least twelve** defects. For each, name the Part XI chapter whose rule it breaks, and say what happens in
   production: which partner, which on-call engineer, or which finance analyst notices, and when.
2. Explain the output line by line: why 27 admissions against a quota of 20, why 53 alerts for one partner, why an
   unknown partner is admitted, and why no billing event was uploaded at all.
3. Two methods can deadlock with each other, and one can deadlock with the alert hook. Find the lock orders. Which of
   them could the demo never trigger, and why does that make them more dangerous, not less?
4. `Slot`'s `Drop` and the `unwrap`s on every lock form one failure chain with Chapter 8.3's double panic. Walk through
   it, starting from a panic inside `on_exceeded`.
5. Rewrite the design: the data structure behind the locks, the admission check, the alert path, the audit path, the
   billing path, shutdown, and the poisoning policy. State which strategy from Chapter 11.7's procedure each piece of
   state gets.

A redesign (listing `review-02-quota-fixed.rs`, verified in debug and release, 6 tests passing) runs the same load and
prints:

```text
limit 20, attempts 80: admitted 20, usage_of = 20
quota alerts fired: 1
unknown partner 'zeta': Some(UnknownPartner)
5th concurrent 'beta' request: Some(TooManyInFlight)
after the 4 finish: admitted = true
partners reported at window close: Ok(2)
usage_of(acme) after close: 0
new window, new limits: acme admitted = true
writer on shutdown: 26 admissions (286 bytes) written in 2 batches; summaries ["acme used 20", "beta used 5"]
```

(The batch count depends on timing; the counts don't.) Its tests include exactness under 8 concurrent threads (exactly
50 admitted against a limit of 50, one alert), audit backpressure that refuses and rolls back, and recovery from a
poisoned shard. Write your review first, then compare with the redesign and the model answers. The redesign is one
defensible answer, not the only one.

---

## Interview mode

*Senior-level. Answer aloud or in writing, without notes, before checking Appendix A.*

### Language

1. Define `Send` and `Sync`, derive the rules for `&T`, `&mut T`, `Arc<T>`, `Mutex<T>`, and `RwLock<T>` from them, and
   explain why they have no run-time cost.
2. Why does `thread::spawn` require `'static` while `thread::scope`'s `spawn` doesn't? What was unsound about the
   pre-1.0 `thread::scoped`, and what language rule came out of it?
3. A struct gains a private `Rc<str>` field. What breaks, where, and is it a semver-major change?

### Compiler and runtime

4. What does `Mutex::lock` compile to when uncontended? When does it make a system call? Where is the poison flag, and
   when is it set?
5. What does `OnceLock::get_or_init` guarantee under a race, and what does a read cost afterwards? How does a failed
   initializer behave in `OnceLock` versus `LazyLock`?
6. How does a `for msg in rx` loop know to stop? What happens to a value sent after the receiver is dropped?

### Performance

7. Rank no sharing, a padded per-thread atomic, adjacent per-thread atomics, one shared atomic, and one shared mutex by
   cost, and explain each step in cache-line terms. Why did the absolute numbers vary up to 5× between runs?
8. Why can `RwLock` be slower than `Mutex` on a read-mostly workload? What would you use instead for data replaced in
   bulk, and for data updated per key?
9. A `par_iter` over 100 items made a request slower. Why? What's the rule of thumb for when parallelism pays?

### Architecture

10. Walk Chapter 11.7's decision procedure for a gateway's route table, its per-route counters, a session store, and a
    connection's protocol state.
11. Design admission control for a thread-per-connection server: what's bounded, what does a refused client see, and
    which metrics tell you which regime you're in?
12. Your team ports a Java service built on `synchronized` methods, `ReentrantReadWriteLock`, and
    `Executors.newFixedThreadPool`. List the three porting bugs you expect first, and the Rust design for each.

### Operations

13. A Rust service shows low CPU, flat profiles, and low throughput under load. What do you measure, in which order, and
    which Part XI failure scenarios would each measurement confirm?
14. Write the review checklist you'd apply to any PR that adds shared mutable state, in ten lines or fewer.
