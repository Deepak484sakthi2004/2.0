# Week 4 — Observability, Mocks & the Values Round (Days 22–28)

Nothing new gets built this week. You harden what exists, perform under interview conditions four times, and prepare the round candidates most under-prepare: values/behavioral.

---

## Day 22 — Observability

### The three pillars, and the two acronyms

**Metrics** (cheap, aggregable, alertable), **logs** (rich, per-event, expensive), **traces** (causality across services). For every *service*, instrument **RED**: **R**ate (req/s), **E**rrors (failure rate), **D**uration (latency histogram — p50/p95/p99, never averages: one 30 s outlier hides in an average, and tail latency is what users feel). For every *resource* (pool, queue, host), **USE**: **U**tilization, **S**aturation (queue depth, wait time — saturation is the leading indicator), **E**rrors.

### SLI → SLO → error budget → alerts

**SLI** = a measured ratio of good events / total (e.g., "successful notification dispatches within 5 s / all dispatches"). **SLO** = the target ("99.9% over 30 days"). **Error budget** = 1 − SLO: 99.9% monthly ≈ **43.8 minutes** of allowed badness — the budget converts reliability into a spendable resource (budget healthy ⇒ ship features; budget burned ⇒ freeze and fix). **Alert on burn rate**, not raw dips: "budget burning 14.4× too fast over the last hour" pages a human; the multiwindow pattern (fast-burn 1 h page + slow-burn 6 h ticket) avoids both missed incidents and 3 a.m. flappy pages. Saying "burn-rate alerts" is a differentiator.

### Structured logging and tracing

Logs are **JSON events**, not prose: `{ts, level, service, trace_id, user_id_hashed, event: "notify.dispatch.failed", provider, attempt, latency_ms}` — machine-queryable, joinable to traces via `trace_id`, and PII-free by policy. Levels: ERROR = a human may need to act; WARN = degraded but handled; INFO = state changes; DEBUG = off in prod.

**Tracing:** a request gets a **trace id**; each hop adds **spans** (name, start, duration, parent) propagated via headers (W3C `traceparent`). Sampling: **head-based** (decide at ingress, cheap, may drop the interesting one) vs **tail-based** (keep slow/erroring traces after the fact, costlier). OpenTelemetry is the vendor-neutral instrumentation standard feeding Datadog/Jaeger/etc. Datadog vocabulary for the JD: monitors (alert rules), dashboards, APM (their tracing), facets (indexed log fields), SLO tracking built in.

### Build: retrofit the notification service

Write the observability addendum: **SLIs** — dispatch success rate; ingest→provider-handoff p99; end-user delivery success (join provider receipts). **Three alerts with thresholds** — (1) fast burn: 5-min dispatch success < 99% AND burn rate > 14× → page; (2) `channel DLQ depth > 100` or growing 10 min → page (means retries are exhausting); (3) `Kafka consumer lag > 60 s` on the events topic → ticket, page at 5 min. **The slow-request trace, narrated:** ingress span 40 ms → router span 2.3 s (child: Redis dedup 1 ms; child: preference DB query **2.2 s** ← the culprit: missing index) → channel span 300 ms. Practice *telling* that story; "I'd open the trace and look for the widest span" is the answer to half of all operational questions.

### DSA (light drip resumes)

Two mediums from your weak-topic list. From today, every solve is out-loud.

---

## Day 23 — Resilience Patterns

**Timeouts:** every remote call has one, always; no timeout ⇒ threads pile up on a slow dependency until *you* are the outage. Budgeted timeouts: if the edge promises 2 s, inner hops get slices of it. **Retries:** only idempotent operations; exponential backoff **with full jitter** (`sleep = rand(0, base·2^attempt)`) — without jitter, synchronized clients retry in waves and DDoS the recovering service; cap attempts; add a **retry budget** (e.g., retries ≤ 10% of requests) so retry storms can't amplify an outage. **Retries need deadlines too** — retrying past the caller's timeout wastes work.

**Circuit breaker** — stop hammering a failing dependency; fail fast; probe for recovery:

```java
public final class CircuitBreaker {
    private enum State { CLOSED, OPEN, HALF_OPEN }
    private final int failureThreshold;       // consecutive failures to open
    private final long openCooldownNanos;     // how long to stay open
    private final LongSupplier clock;
    private State state = State.CLOSED;
    private int consecutiveFailures = 0;
    private long openedAt = 0;

    public CircuitBreaker(int failureThreshold, Duration cooldown, LongSupplier clock) {
        this.failureThreshold = failureThreshold;
        this.openCooldownNanos = cooldown.toNanos();
        this.clock = clock;
    }

    public synchronized boolean allowRequest() {
        if (state == State.OPEN) {
            if (clock.getAsLong() - openedAt >= openCooldownNanos) {
                state = State.HALF_OPEN;      // let exactly the next request probe
                return true;
            }
            return false;                     // fail fast
        }
        return true;                          // CLOSED or HALF_OPEN (probe in flight)
    }
    public synchronized void onSuccess() { consecutiveFailures = 0; state = State.CLOSED; }
    public synchronized void onFailure() {
        consecutiveFailures++;
        if (state == State.HALF_OPEN || consecutiveFailures >= failureThreshold) {
            state = State.OPEN;
            openedAt = clock.getAsLong();
        }
    }
}
```

(Production versions use failure-*rate* windows and limited half-open concurrency — say so.) **Bulkheads:** partition resources per dependency (separate pools/semaphores) so one sick downstream can't drain the shared pool — you built the primitive on Day 6. **Load shedding:** bounded queues + reject early (HTTP 429/503 + `Retry-After`) beats accepting work you'll fail slowly; shed by priority (drop analytics before checkout). **Graceful degradation:** fallbacks (cached/stale data, default recommendations), feature flags to switch off expensive paths under duress. **Backpressure** ties the week together: bounded queues (Day 1) → CallerRuns (Day 2) → semaphores (Day 6) → shedding — the same idea at four altitudes; drawing that line in an interview is gold.

**Afternoon — chaos thought-experiments, one per design:** Notification: SMS provider 100% failure for 20 min → breaker opens ≤ threshold·latency, failover provider takes over, DLQ catches the boundary losses, burn-rate alert pages — walk the timeline in writing. Editor: doc-service node dies with 200 hot docs → lease expiry, new owners rebuild from snapshot+log, clients resend unacked ops (idempotent by opId) — what's the p99 rebuild time and what bounds it? Board: Redis cluster down → cache-miss storm on Postgres → does your connection pool + shed policy keep p99 bounded, or do you need a request-coalescing layer? Write the honest answer, including the parts that would hurt.

---

## Day 24 — Mock 1 (LLD) + STAR Stories I

**Mock prompt (unseen — do not pre-read): *Design an in-memory task queue with priorities and delayed execution.* 75 min, recorded.** After: solution shape to grade against — this composes Week 2: `PriorityBlockingQueue` ordered by `(readyAtNanos, priority, seq)` won't block on not-yet-ready heads, so either (a) a `DelayQueue` of ready-gates feeding a priority queue, or (b) single `DelayQueue` where `compareTo` orders by readyAt then priority, accepting that priority applies among *due* tasks via a small ready-buffer the dispatcher drains by priority. Option (b) with a clear explanation is the strong answer; recognizing the tension between "delayed" and "priority" *is* the test. Plus: `seq` for FIFO fairness within equal priority (starvation!), cancel via live-map + lazy skip, worker pool separation — all Day 12 muscles.

**Recording review checklist:** Did you clarify before coding? Interfaces first? State the concurrency unit? Handle the priority/delay tension explicitly? Narrate continuously (silence > 60 s = flag)? Test or describe tests? Score the 7 axes; weakest axis becomes Day 27's mock focus.

### STAR + Atlassian values (evening)

Structure every story: **S**ituation (2 sentences) → **T**ask (your responsibility) → **A**ction (what *you* did — "I", not "we"; the technical and interpersonal specifics) → **R**esult (quantified + what you learned). 90 seconds spoken, headline first.

Atlassian's five published values, and what each story must show:

1. **Open company, no bullshit** — transparency, sharing bad news early, honest disagreement. *Prompt: a time you surfaced an uncomfortable truth (a slipping deadline, a flawed design you'd championed).*
2. **Build with heart and balance** — caring about craft AND sustainability; considered trade-offs over heroics. *Prompt: a time you balanced speed vs quality, or pushed back on burnout-mode planning with data.*
3. **Don't #@!% the customer** — customer impact drives decisions. *Prompt: a time you took the harder path because the easy one degraded user experience; an incident where you prioritized user comms.*
4. **Play, as a team** — team wins over solo wins; helping others succeed. *Prompt: unblocking a teammate at cost to your own task; handling conflict inside the team constructively.*
5. **Be the change you seek** — improving things nobody asked you to. *Prompt: a process/tooling/quality improvement you initiated and drove to adoption.*

Tonight: draft stories 1–4 (values 1 and 3 first — the interviews weight candor and customer-obsession heavily). One page each, then compress to spoken-90-seconds bullets.

---

## Day 25 — Mock 2 (System Design) + STAR Stories II

**Mock prompt: *Design a webhook delivery system* — tenants register URLs; your platform must deliver events reliably to millions of endpoints you don't control. 60 min, recorded.** (Deliberately adjacent-but-new: it forces transfer, and it's an extremely Atlassian problem.) Grade-against outline: ingest → Kafka keyed by (tenant, endpoint) for per-endpoint ordering → delivery workers with per-endpoint token buckets, timeouts, breakers per endpoint (an endpoint down for hours must cost you almost nothing — bulkhead + breaker + park in a per-endpoint retry schedule, exponential to hours), HMAC request signing (integrity + authn), at-least-once + `event_id` for consumer-side dedup, DLQ + tenant-visible dead-letter UI + manual redrive, and the **slow/hanging endpoint** deep dive (connection pool isolation; response-size caps; never follow redirects blindly — SSRF one-liner: validate/deny-list internal IP ranges at egress). Failure mode to volunteer: a popular endpoint provider (one hostname, thousands of tenants) goes down — per-endpoint isolation must aggregate up or your worker fleet drowns in timeouts.

**Review** against the 8-axis rubric; rewrite the section you fumbled.

**Evening:** STAR stories 5–8 (values 2, 4, 5 + one spare "failure/learning" story — the "tell me about a time you failed" answer where the *learning changed your later behavior* is the whole point). **Anti-patterns to edit out ruthlessly:** "we did" hiding your part · no numbers in results · rambling context (cap S+T at 20 s) · stories where the conflict has no resolution · blaming ("the PM kept changing scope" → "I set up a change-triage step with the PM").

---

## Day 26 — Mock 3 (DSA) + Values Rehearsal

**Mock: two problems, 60 min, out loud, recorded.**

**Problem 1 — Insert Delete GetRandom O(1).** Key idea: array (O(1) random via index) + map value→index; delete = swap-with-last, pop, fix map.

```java
private final List<Integer> a = new ArrayList<>();
private final Map<Integer, Integer> idx = new HashMap<>();
public boolean insert(int v) {
    if (idx.containsKey(v)) return false;
    idx.put(v, a.size()); a.add(v); return true;
}
public boolean remove(int v) {
    Integer i = idx.get(v);
    if (i == null) return false;
    int last = a.get(a.size() - 1);
    a.set(i, last); idx.put(last, i);          // move last into the hole
    a.remove(a.size() - 1); idx.remove(v);
    return true;
}
public int getRandom() { return a.get(ThreadLocalRandom.current().nextInt(a.size())); }
```

(Edge to narrate: removing the last element itself — the swap is a harmless self-swap because the map update precedes the removal.)

**Problem 2 — Rotting Oranges (multi-source BFS).** Enqueue all initially-rotten cells with t=0; BFS layer by layer; answer = max time; if any fresh remains, −1. The transferable idea: *multi-source BFS = start the queue with all sources*; same trick solves walls-and-gates, 01-matrix.

**Review both:** state complexity unprompted; dry-run one example before declaring done — the two habits DSA rounds actually grade beyond correctness.

**Values mock (evening) — answer these six aloud, recorded, 90 s each:** Tell me about a time you disagreed with a technical decision — what did you do? · A time you had to deliver bad news · Your biggest production incident — your role, and what changed after · A time you helped a teammate at cost to yourself · Something you improved that nobody asked you to · A time you were wrong in a design discussion. Listen back for: headline-first? "I" specific? quantified result? under 2 minutes?

---

## Day 27 — Mock 4 + Final Polish

**Mock 4 (75 min):** rerun the *format* that scored lowest this week, with a fresh prompt. LLD pool: design a thread-safe object pool with health-checks and max-idle eviction · an in-memory search index with AND-queries over tokenized docs · a sliding-window metrics library (`record(name, value)` / `p99(name, window)`). Design pool: a feature-flag service (reads must survive the flag store being down — cached rules + last-known-good) · a distributed cron service (leader election + at-least-once firing + idempotent jobs — Day 12 meets Day 16) · an audit-log pipeline (append-only, tamper-evident hashing, tiered retention).

**Final polish checklist (afternoon):** every repo README current · four LLD components with green tests · three-plus-two design docs each stating trade-off + failure mode + observability paragraph · gap list empty or consciously accepted · STAR bullets on one printable page.

**Rapid-fire flashcards — the 60.** Answer from memory; anything shaky gets a re-read tonight. **Concurrency:** volatile vs synchronized guarantees · why wait() in a while · two Conditions vs notifyAll · deadlock's four conditions + which you break · CAS and ABA · AtomicLong vs LongAdder · CHM write path (CAS empty bin, lock bin head) · thenApply vs thenCompose · fixed-rate vs fixed-delay · why inject a clock · ThreadPoolExecutor admission order · CallerRunsPolicy's virtue · semaphore vs rate limiter axes · ThreadLocal leak in pools · safe-publication idioms. **Data:** read-your-writes fixes · W+R>N meaning · consistent hashing + vnodes · hot-key mitigations · SI vs serializable (write skew!) · SELECT FOR UPDATE vs version column · 2PC's flaw · saga compensation · composite-index column order · index-only scans · keyset pagination · Dynamo single-table premise · GSI consistency · when to refuse NoSQL · why replicas shouldn't self-expire TTLs. **Messaging:** Kafka ordering scope · commit-after vs commit-before · consumer-group rebalancing cost · retention vs compaction · acks=all + min ISR · SQS visibility timeout · FIFO trade-off · outbox pattern · idempotent-consumer table · honest exactly-once. **Design/ops:** CAP said correctly + PACELC · cache-aside stale-set race · stampede defenses (name 3) · versioned cache keys · LexoRank idea · single-writer-per-doc rationale · OT vs CRDT one-liner · RED vs USE · error budget of 99.9%/month · burn-rate alerting · full jitter formula · breaker states · bulkhead definition · shed-early rationale · budgeted timeouts.

---

## Day 28 — Taper

**Morning re-derivations (30 min each, blank page):** Token bucket — the 8 decisions in order: API shape (`tryAcquire` boolean) → per-client state map → `computeIfAbsent` atomicity → lazy refill math → monotonic clock, injected → per-bucket lock → capacity=burst semantics → the distributed (Redis+Lua) and lock-free (packed CAS) extensions. KV-TTL — API → entry with deadline → lazy check on get → two-arg remove race → sweeper (sampling) → why lazy+active together → LRU bolt-on → the three Redis contrasts. If either derivation stalls, re-read only that section — nothing else today.

**Logistics (afternoon):** environment tested (IDE, screenshare, mic) · water, notebook, the one-page STAR sheet · **questions to ask them** (have real ones): "What does on-call look like for this team?" · "What's a recent incident and what changed after it?" · "How do design decisions get made and documented here?" · "What separates good from great at SDE 2 on your team?"

**Evening:** one easy-medium warm-up for fluency, close the laptop by 18:00. The plan is done — 196 hours, four shipped components, five design docs, four recorded mocks, eight stories. You didn't cram an interview; you did the job for a month. Go show them.

---

## Appendix — the two rubrics

**LLD (grade 1–5 each):** requirements clarified · interfaces before implementation · concurrency unit chosen and justified · error/edge handling · extensibility story · tests written or precisely described · narration quality.

**System design (grade 1–5 each):** requirements + NFRs stated · estimates done and sane · API sketched · data model justified · architecture with per-box purpose · one genuine deep dive · explicit trade-offs (consistency!) + failure modes · observability mentioned unprompted.
