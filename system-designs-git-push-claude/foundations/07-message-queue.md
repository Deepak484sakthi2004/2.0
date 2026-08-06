# Message Queue Architecture

## 1. Problem Statement & Scope

Design the asynchronous messaging backbone for a large platform (order processing + notifications + analytics events for an e-commerce site). Goal: decouple producers from consumers, absorb bursts, guarantee delivery semantics per use case, preserve ordering where required, and keep consumers correct under retries and failures.

### Functional Requirements
- Produce/consume messages across independent services; producers never block on consumer availability.
- Per-use-case delivery semantics: at-least-once for order events (with idempotent consumers ⇒ effectively-once), at-most-once acceptable for metrics, exactly-once *processing* for payment ledger updates.
- Ordering: per-entity ordering (all events for order #123 in order); no global ordering requirement.
- Consumer groups: N instances of a service share a stream's load; multiple independent services each get the full stream (fan-out).
- Replay: analytics can re-read the last 7 days.
- Retry with exponential backoff; poison messages routed to a Dead Letter Queue (DLQ) with tooling for inspection and redrive.
- Backpressure: slow consumers must not crash the broker or lose data.

### Non-Functional Requirements
- Throughput: sustain peak event rate with ≤ p99 200 ms produce latency; end-to-end lag SLO < 5 s for order pipeline, < 5 min for analytics.
- Durability: acknowledged messages survive loss of any single broker (RF=3, no data loss on 1-node failure).
- Availability: 99.95% produce availability (order placement must not fail because a downstream is slow).
- Retention: 7 days on the main log; DLQ 14 days.

### Back-of-Envelope Estimation
- 50 M DAU; 10 M orders/day; each order emits ~20 events through its lifecycle (created, paid, packed, shipped, …) → 200 M order events/day.
- Clickstream/analytics: 2 B page views/day × 3 events = 6 B events/day.
- Total ≈ 6.2 B events/day / 86,400 ≈ 72 K msg/s average; peak 3× ≈ **215 K msg/s**.
- Avg message 1 KB → peak ingress ≈ **215 MB/s**; with RF=3, broker disk write ≈ 645 MB/s across the cluster — trivially handled by ~9–12 brokers doing sequential I/O (~100+ MB/s each even on modest disks; NVMe makes this laughable).
- Storage: 6.2 B × 1 KB × 7 days × 3 replicas ≈ **130 TB** cluster-wide → ~11 TB/broker on 12 brokers. Fine.
- Partitions: order topic at peak 3× (200 M/86,400) ≈ 7 K msg/s; a single Kafka partition comfortably does 5–20 K msg/s, but consumer processing (say 200 msg/s per consumer thread due to DB writes) dominates: 7,000 / 200 = 35 → provision **48 partitions** (headroom + divisibility).

## 2. Brute-Force / Naive Design

Naive #1 — synchronous HTTP chaining: order service calls payment, then inventory, then email, in-line.
- Availability math: 4 services at 99.9% each ⇒ 0.999⁴ ≈ 99.6% — order placement now fails when the *email* service hiccups.
- Latency: sum of downstream p99s; one slow dependency (email provider at 2 s) makes checkout 2 s.
- Bursts: flash sale at 10× normal rate has nowhere to queue; everything times out simultaneously.

Naive #2 — a database table as a queue (`SELECT ... WHERE status='pending' FOR UPDATE SKIP LOCKED`):
- Works to ~1–5 K msg/s, then dies: polling load, table/index bloat from churn (every message = insert + update + delete ⇒ vacuum storm in Postgres), lock contention among consumers.
- At 215 K msg/s you'd be doing ~600 K row operations/s on a hot table plus index maintenance — 2 orders of magnitude past comfortable.
- No fan-out (multiple independent consumer services need duplicated rows), no replay without keeping everything, no ordering guarantees under SKIP LOCKED.
- Legit at small scale — and it *is* the right building block for the outbox pattern (§7) — but not the backbone.

## 3. Evolving the Design

**Step 1 — Bottleneck: synchronous coupling.** Introduce a broker: producers publish `order.events`, consumers subscribe. Order placement now depends only on broker availability (one 99.99% dependency instead of four 99.9% ones). Burst absorption: the queue depth grows during the flash sale; consumers drain it after.

**Step 2 — Bottleneck: one consumer can't keep up.** Consumer processing (200 msg/s each, DB-bound) < peak 7 K msg/s. Fix: **partitions + consumer groups**. Topic split into 48 partitions; the group protocol assigns each partition to exactly one consumer in the group. Scale consumers up to 48 (one per partition — the hard parallelism ceiling; more consumers sit idle). Key insight to state: *partition count is chosen from consumer throughput, not broker throughput*.

**Step 3 — Bottleneck: ordering broke when we parallelized.** Two events for order #123 processed by different consumers can commit out of order (shipped before paid). Fix: **partition by entity key** — `partition = hash(order_id) % 48`. All events for one order land in one partition ⇒ consumed by one consumer, in log order. Trade-off accepted: a hot key (one order can't be hot, but a hot *merchant* key could be) skews a partition; and increasing partition count later re-maps keys (breaks ordering during transition) — so over-provision partitions up front.

**Step 4 — Bottleneck: duplicate deliveries corrupt state.** At-least-once delivery means redelivery after consumer crash between "processed" and "committed offset". Fix: **idempotent consumers** — dedupe on a message/business key (§7), not attempts to make the network exactly-once. State the law: *in a distributed system you pick at-most-once or at-least-once on the wire; exactly-once is an end-to-end property built from at-least-once + idempotency (or transactions)*.

**Step 5 — Bottleneck: lost messages at the source.** Service writes DB then publishes; crash between the two loses the event (or publish-then-write creates events for rolled-back transactions). Fix: **transactional outbox** — write the event to an `outbox` table in the same DB transaction as the state change; a relay (poller or CDC/Debezium) publishes it and marks it sent. At-least-once from the outbox + idempotent consumers = end-to-end effectively-once.

**Step 6 — Bottleneck: poison messages block partitions.** One malformed message that always throws will, with naive blocking retry, halt its partition forever (head-of-line blocking). Fix: bounded in-process retries with exponential backoff + jitter → then publish to **retry topics** (`orders.retry.1m`, `orders.retry.10m`) with delayed consumption → after N total attempts, to **DLQ** with full error metadata. Partition keeps flowing; humans/automation redrive the DLQ.

**Step 7 — Bottleneck: slow consumer during incident, queue grows unbounded?** With log-based Kafka, backlog is bounded by retention (disk), not memory — the broker doesn't care about lag; consumers **pull** at their own pace (natural backpressure). Add lag-based autoscaling (scale consumer group on `consumer_lag` metric) and producer-side load shedding for the analytics tier only (drop sampled clickstream before dropping order events).

**Step 8 — Bottleneck: rebalance storms.** Every consumer join/leave triggers partition reassignment; with eager rebalancing the whole group stops (stop-the-world). Fix: cooperative incremental rebalancing (`CooperativeStickyAssignor`), static group membership (`group.instance.id`) so a pod restart doesn't trigger reassignment, and tuned `max.poll.interval.ms` so slow batches aren't mistaken for dead consumers.

## 4. Protocol & Technology Choices — Why This, Not That

### Kafka vs RabbitMQ vs SQS vs Pulsar (the core table)

| Dimension | Kafka (chosen for backbone) | RabbitMQ | SQS (+SNS) | Pulsar |
|---|---|---|---|---|
| Model | Distributed, partitioned, replicated **log**; consumers track offsets | **Smart broker**: exchanges/bindings route to queues; broker tracks per-message acks | Fully managed queue; visibility-timeout redelivery | Log like Kafka, but **compute (brokers) separated from storage (BookKeeper)** |
| Throughput | Millions msg/s; sequential I/O + zero-copy + batching | ~10–50 K msg/s per node typical | High but per-API-call cost; 3K msg/s per FIFO queue (30K batched) | Kafka-class |
| Ordering | Per partition, strong | Per queue (single consumer); weak with competing consumers/requeues | FIFO queues: per MessageGroupId; standard: best-effort | Per key/partition |
| Replay / fan-out | Free: consumers are independent cursors over retained log | No replay — consumed = gone; fan-out via exchange duplication | No replay; fan-out via SNS→multiple SQS | Free, like Kafka; tiered storage native |
| Delayed/priority msgs | Not native (retry-topic pattern) | Native-ish (TTL+DLX plugin, priority queues) | Native delay up to 15 min | Native delayed delivery |
| Routing sophistication | Dumb broker: topic/partition only | Rich: topic/headers/fanout exchanges, per-message routing keys | None (SNS filter policies help) | Moderate |
| Ops burden | Significant (or pay for MSK/Confluent) | Moderate | **Zero** | Highest (brokers + BookKeeper + ZK) |
| Latency | Low ms (batching adds a few ms) | Very low ms end-to-end | 10s of ms + polling | Low ms |

**Why Kafka:** we need replay for analytics, fan-out to many independent consumer services without duplicating data, per-key ordering at 200 K+ msg/s, and 7-day retention — that's precisely the log abstraction. **When RabbitMQ wins:** complex per-message routing (route by header to region-specific queues), task/job queues needing per-message ack + fair dispatch + priorities + low latency at modest scale. **When SQS wins:** small team, AWS-native, no ops appetite, no replay/ordering-at-scale needs — the correct default for 90% of startups; also as a DLQ substrate for Lambda consumers. **When Pulsar wins:** need per-topic multi-tenancy with millions of topics, geo-replication built-in, or elastic storage/compute scaling independently (adding brokers without moving data).

### Delivery semantics — pick per use case

| Semantics | Mechanism | Cost | Use here |
|---|---|---|---|
| At-most-once | Fire-and-forget produce (`acks=0`); commit offset before processing | Loss on any failure | Sampled clickstream metrics |
| At-least-once | `acks=all` + producer retries; commit offset **after** processing | Duplicates on redelivery | Order events (default) |
| Exactly-once (Kafka-internal) | Idempotent producer (PID + seq) + transactions (`read_process_write` atomically, `read_committed` consumers) | Throughput/latency overhead; only covers Kafka→Kafka (streams) | Stream-processing aggregations |
| Effectively-once (end-to-end) | At-least-once + **idempotent consumer** (dedupe key in target store) | Dedupe storage + design discipline | Payment ledger, anything touching external systems |

Key interview line: Kafka's "exactly-once" (EOS) is exactly-once *stream processing within Kafka*; the moment a consumer calls an external API or writes a non-transactional store, you're back to needing idempotency at the sink.

Decision checklist to run per topic (say it as a drill, not a debate):

1. Can the business tolerate losing this message? Yes → at-most-once is on the table (metrics only).
2. Is the sink a single transactional DB you control? Yes → store offsets in the sink transaction (strongest, cheapest exactly-once).
3. Is the sink another Kafka topic? Yes → idempotent producer + transactions (Kafka EOS applies).
4. Is the sink an external API? → at-least-once + pass `message_id` as the provider's idempotency key; if the provider has none, wrap the call in a local dedupe check-then-call with a claim row (accept the tiny crash window between call and record, and reconcile).
5. Does processing have a natural idempotent form (`SET status=X`, version-gated apply)? Prefer it over a dedupe table — no storage, no pruning, no horizon bugs.
6. Whatever you picked: verify the dedupe horizon ≥ max redelivery horizon (retention + DLQ redrive window), or the guarantee is fiction.

### Offset commit strategy

| Strategy | Behavior on crash | Verdict |
|---|---|---|
| Auto-commit (every 5 s, before processing completes) | Can lose messages (committed-but-unprocessed) *and* duplicate | Never for important data |
| Manual commit after batch processed | Redelivers the in-flight batch → duplicates only | Chosen (with idempotent handlers) |
| Commit per message (sync) | Minimal duplication window | Throughput killer (~1 commit RTT/msg); only for very low volume |
| Store offset in the sink DB transactionally with results | True exactly-once into that DB | Best when sink is a single transactional DB |

### Push vs Pull consumption

| | Pull (Kafka, chosen) | Push (RabbitMQ, webhooks) |
|---|---|---|
| Backpressure | Implicit — consumer polls at its capacity | Broker must track windows (prefetch/QoS); overrun risk |
| Latency | Long-poll makes it near-push | Slightly lower |
| Batching | Natural (fetch.min.bytes) | Harder |

Pull wins for throughput systems; push wins for latency-critical dispatch to many idle consumers.

### Broker replication settings (Kafka specifics worth saying aloud)
`replication.factor=3`, `min.insync.replicas=2`, producer `acks=all`, `enable.idempotence=true`: tolerates one broker loss with zero acknowledged-message loss; a second failure makes the partition unavailable-for-produce rather than silently lossy (CP choice on the write path). `unclean.leader.election=false` — prefer unavailability over data loss.

## 5. High-Level Design (HLD)

```mermaid
flowchart LR
    subgraph Producers
        OS[Order Service] --> OB[(Outbox table)]
        OB --> CDC[CDC Relay / Debezium]
        WEB[Web/App clients] --> GW[Event Gateway]
    end
    CDC -->|order.events, key=order_id| K[(Kafka Cluster<br/>RF=3, 48 partitions)]
    GW -->|clickstream, acks=0/1| K
    subgraph ConsumerGroups
        K --> PG[payments-group<br/>idempotent handlers]
        K --> NG[notifications-group]
        K --> AG[analytics-group<br/>replays 7d]
    end
    PG -->|fail fast| R1[(orders.retry.1m)]
    R1 --> R2[(orders.retry.10m)]
    R2 --> DLQ[(orders.dlq)]
    DLQ --> OPS[DLQ Console: inspect / patch / redrive]
    PG --> PDB[(Payments DB<br/>processed_messages dedupe)]
    NG --> EXT[Email/SMS providers]
    AG --> DWH[(Warehouse / lake)]
    MON[Lag monitor + autoscaler] -.-> ConsumerGroups
```

### Write Path (produce)
1. Order service commits business change + outbox row in one DB transaction.
2. CDC relay reads the outbox (binlog tail), publishes to `order.events` with `key=order_id`, headers `{message_id (UUID), event_type, schema_version, trace_id}`; marks/watermarks the outbox row. Producer: idempotent, `acks=all`, batches (`linger.ms=5`) with compression (lz4/zstd — 3–5× on JSON).
3. Broker: leader appends to log, waits for ISR quorum (min 2 of 3), acks. Clickstream path skips the outbox and uses `acks=1` — loss-tolerant by contract.

### Read Path (consume)
1. Consumer in group `payments-group` is assigned partitions {0..11} by the group coordinator; polls batches (up to 500 records).
2. For each record: check `processed_messages` for `message_id` → skip if present; else process + insert dedupe row in the same DB transaction.
3. Commit offsets manually after the batch. Crash before commit ⇒ batch redelivered ⇒ dedupe rows absorb it.
4. On handler exception: retry in-process 3× with backoff; still failing → produce to `orders.retry.1m` (with `attempt` header), commit offset, move on — the partition never blocks.

### Data Model
```sql
-- Outbox (in each producing service's DB)
CREATE TABLE outbox (
  id            BIGSERIAL PRIMARY KEY,
  aggregate_id  TEXT NOT NULL,        -- becomes the Kafka key
  event_type    TEXT NOT NULL,
  payload       JSONB NOT NULL,
  headers       JSONB NOT NULL,       -- message_id, trace_id, schema_version
  created_at    TIMESTAMPTZ DEFAULT now()
);
-- Consumer-side dedupe
CREATE TABLE processed_messages (
  consumer_group TEXT,
  message_id     UUID,
  processed_at   TIMESTAMPTZ DEFAULT now(),
  PRIMARY KEY (consumer_group, message_id)
);  -- pruned after 7d (must exceed max redelivery horizon)
```

### Message Envelope (API)
```json
{
  "message_id": "uuid-v7",
  "event_type": "order.paid",
  "schema_version": 3,
  "occurred_at": "2026-08-06T10:15:00Z",
  "trace_id": "…",
  "partition_key": "order_1234",
  "payload": { "order_id": "1234", "amount_cents": 4999, "currency": "USD" }
}
```
Schema managed in a registry (Avro/Protobuf), **backward-compatible evolution enforced at CI**: consumers deploy before producers for new required fields; never renumber/reuse fields.

## 6. Low-Level Design (LLD)

Patterns: **Strategy** (RetryPolicy, PartitionStrategy), **Template Method** (ConsumerWorker skeleton: poll → dedupe → handle → ack), **Decorator** (IdempotencyDecorator, MetricsDecorator wrap MessageHandler), **Repository** (ProcessedMessageRepository, OutboxRepository), **Factory** (ConsumerFactory builds configured pipelines), **Chain of Responsibility** (retry → retry-topic → DLQ escalation).

```mermaid
classDiagram
    class Producer~T~ {
        <<interface>>
        +send(String topic, String key, Message~T~ msg) Future~Ack~
        +sendTransactional(List~Message~ batch)
        +flush()
        +close()
    }
    class KafkaProducerAdapter~T~ {
        -KafkaProducer delegate
        -PartitionStrategy partitioner
        -DeliverySemantics semantics
        +send(topic, key, msg) Future~Ack~
    }
    class Consumer~T~ {
        <<interface>>
        +subscribe(List~String~ topics)
        +poll(Duration timeout) List~Message~T~~
        +pause(Set~TopicPartition~ tps)
        +resume(Set~TopicPartition~ tps)
        +close()
    }
    class KafkaConsumerAdapter~T~ {
        -KafkaConsumer delegate
        -OffsetStore offsets
    }
    class OffsetStore {
        <<interface>>
        +committed(TopicPartition tp) long
        +commit(Map~TopicPartition,long~ offsets)
        +commitInTx(Map offsets, TxContext tx)
    }
    class KafkaOffsetStore
    class SinkDbOffsetStore {
        -DataSource db
    }
    class DeliverySemantics {
        <<enumeration>>
        AT_MOST_ONCE
        AT_LEAST_ONCE
        EXACTLY_ONCE_KAFKA
        EFFECTIVELY_ONCE_E2E
    }
    class MessageHandler~T~ {
        <<interface>>
        +handle(Message~T~ msg)
    }
    class IdempotencyDecorator~T~ {
        -ProcessedMessageRepository repo
        -MessageHandler~T~ inner
        +handle(Message~T~ msg)
    }
    class PaymentHandler {
        -LedgerService ledger
        +handle(Message~PaymentEvent~ msg)
    }
    class ConsumerWorker~T~ {
        -Consumer kafkaConsumer
        -MessageHandler~T~ handler
        -RetryPolicy retryPolicy
        -DlqPublisher dlq
        +runLoop()
        #processBatch(records)
    }
    class RetryPolicy {
        <<interface>>
        +nextDelay(int attempt) Duration
        +shouldRetry(int attempt, Exception e) bool
    }
    class ExponentialBackoffJitter {
        -Duration base
        -Duration cap
        -int maxAttempts
    }
    class NoRetryPolicy
    class PartitionStrategy {
        <<interface>>
        +partition(String key, int numPartitions) int
    }
    class HashPartitionStrategy
    class ProcessedMessageRepository {
        <<interface>>
        +markIfNew(String group, UUID msgId) bool
        +pruneOlderThan(Duration d)
    }
    class OutboxRepository {
        <<interface>>
        +append(OutboxRecord r)
        +fetchUnpublished(int limit) List
        +markPublished(List~long~ ids)
    }
    class OutboxRelay {
        -OutboxRepository outbox
        -Producer producer
        +pollAndPublish()
    }
    class DlqPublisher {
        +sendToRetryTier(Message m, int attempt)
        +sendToDlq(Message m, Exception cause)
    }
    class ConsumerFactory {
        +build(TopicConfig cfg) ConsumerWorker
    }
    Producer <|.. KafkaProducerAdapter
    Consumer <|.. KafkaConsumerAdapter
    OffsetStore <|.. KafkaOffsetStore
    OffsetStore <|.. SinkDbOffsetStore
    KafkaProducerAdapter o-- PartitionStrategy
    KafkaProducerAdapter ..> DeliverySemantics : configured by
    KafkaConsumerAdapter o-- OffsetStore
    ConsumerWorker o-- Consumer : polls via
    OutboxRelay o-- Producer : publishes via
    MessageHandler <|.. IdempotencyDecorator
    MessageHandler <|.. PaymentHandler
    IdempotencyDecorator o-- MessageHandler : wraps
    IdempotencyDecorator o-- ProcessedMessageRepository
    ConsumerWorker o-- MessageHandler
    ConsumerWorker o-- RetryPolicy
    ConsumerWorker o-- DlqPublisher
    RetryPolicy <|.. ExponentialBackoffJitter
    RetryPolicy <|.. NoRetryPolicy
    PartitionStrategy <|.. HashPartitionStrategy
    OutboxRelay o-- OutboxRepository
    ConsumerFactory ..> ConsumerWorker : creates
```

Why: Decorator keeps idempotency orthogonal to business logic (every handler gets it by construction, none can forget it); Strategy makes backoff policy per-topic config, not code; Repository isolates the dedupe store so it can be Postgres today, Redis-with-TTL tomorrow. The `Producer`/`Consumer` interfaces are **Adapter** seams: business code never imports Kafka client types, so unit tests run against an in-memory broker fake and a future broker migration (or an SQS-backed low-volume topic) is a wiring change, not a rewrite. `OffsetStore` is abstracted for one specific reason: the exactly-once-into-a-DB pattern stores offsets *in the sink database transaction* (`SinkDbOffsetStore.commitInTx`) — on restart the consumer seeks to the DB's offset rather than Kafka's, making "process + record position" atomic. `DeliverySemantics` as an explicit enum on the producer config forces every topic owner to declare their contract in code review rather than inheriting whatever the client library defaults to.

### Hardest algorithm — consumer loop with dedupe, backoff, retry-tiers, DLQ
```java
void runLoop() {
    while (running) {
        ConsumerRecords<String, byte[]> records = consumer.poll(Duration.ofMillis(500));
        for (ConsumerRecord<String, byte[]> rec : records) {
            Message msg = codec.decode(rec);
            int attempt = msg.header("attempt", 0);
            try {
                // markIfNew: INSERT ... ON CONFLICT DO NOTHING; returns rowCount==1
                // Executed INSIDE the same DB tx as the handler's writes.
                tx.run(() -> {
                    if (!processedRepo.markIfNew(groupId, msg.messageId())) return; // dup: skip
                    handler.handle(msg);
                });
            } catch (TransientException e) {
                if (retryPolicy.shouldRetry(attempt, e)) {
                    // full jitter: sleep in [0, min(cap, base * 2^attempt)]
                    long capped = Math.min(capMs, baseMs * (1L << Math.min(attempt, 20)));
                    sleep(ThreadLocalRandom.current().nextLong(capped + 1));
                    dlqPublisher.sendToRetryTier(msg.withAttempt(attempt + 1), attempt + 1);
                } else {
                    dlqPublisher.sendToDlq(msg, e);   // exhausted
                }
            } catch (PermanentException e) {          // e.g. deserialization, validation
                dlqPublisher.sendToDlq(msg, e);       // no retry: it will never succeed
            }
        }
        consumer.commitSync();  // after the whole batch; crash before ⇒ redelivery ⇒ dedupe absorbs
    }
}
```
Interview notes: (1) **full jitter** prevents synchronized retry waves — with plain exponential backoff, 10 K consumers that failed together retry together; (2) classify exceptions — retrying a validation error just burns 6 attempts before the inevitable DLQ; (3) the dedupe insert and the business write share one transaction — that's what upgrades at-least-once to effectively-once; (4) retry *topics* (not in-place sleep) keep `max.poll.interval.ms` honest and the partition unblocked.

### Retry-tier consumption trick
`orders.retry.1m` consumer reads a message, checks `not_before` header; if in the future, it pauses that partition (`consumer.pause()`) until due — giving delay semantics on a log that has none natively.

## 7. Deep Dives & Failure Modes

**Outbox pattern, precisely.** The dual-write problem: `db.commit(); kafka.send();` — crash between them loses the event; reverse order publishes phantom events for rolled-back transactions. Outbox makes the event part of the ACID transaction; the relay is at-least-once (it may re-publish after a crash before marking sent) — which is fine because consumers dedupe on `message_id`. CDC relay (Debezium) beats a poller at scale: no polling load, ordering follows the binlog, latency ~10–100 ms. Poller is simpler and fine to start (index on unpublished, `FOR UPDATE SKIP LOCKED`, batch 500).

**Idempotency taxonomy (say all three):** (1) *natural* — `SET status='shipped'` is idempotent by construction; prefer these; (2) *dedupe table* — generic, exact, needs storage + pruning ≥ redelivery horizon; (3) *version/fencing* — apply event only if `event.version == row.version + 1`; also fixes out-of-order application. For external side effects (charging a card) pass `message_id` as the provider's idempotency key — you can't dedupe someone else's side effects locally.

**Ordering edge cases.** Per-partition ordering breaks when: producer retries with `max.in.flight > 1` without idempotence (batch 2 succeeds, batch 1 retries → reordered — fixed by `enable.idempotence=true` which allows 5 in-flight *with* ordering); a message detours through retry topics (its siblings pass it — mitigate with per-entity version checks in handlers, or park *all* subsequent messages for that key — expensive, usually version-check wins); partition count changes (key→partition mapping shifts; drain-then-switch or over-provision from day one).

**On-disk log layout — why Kafka is fast (worth 60 seconds in any deep dive).** A partition is a directory of **segments**:

```
orders-7/
  00000000000000000000.log      # append-only record batches
  00000000000000000000.index    # sparse offset → file-position index
  00000000000000000000.timeindex# timestamp → offset (powers offsetsForTimes,
                                #   i.e., "replay from 3 days ago")
  00000000000123456789.log      # rolled at segment.bytes (1 GB) or segment.ms
  ...
```

Mechanics to narrate: appends are **sequential writes into page cache** (no per-message fsync); consumers reading near the tail are served *from page cache* — a healthy cluster does almost no read disk I/O; the offset index is sparse (one entry per ~4 KB), so a seek is: binary-search index → jump to file position → scan forward. Delivery to consumers uses **zero-copy** (`sendfile`: page cache → NIC without entering user space) — unless TLS or broker-side decompression forces a copy, which is why produce-side compression with pass-through matters. Retention (`retention.ms`) and compaction operate on whole segments — deleting old data is `rm` on a file, O(1), which is why 7-day retention at 130 TB is operationally boring. This is the answer to "why is a disk-based log faster than an in-memory queue?": sequential + batched + zero-copy beats random + per-message + copied, and the OS page cache *is* the memory tier.

**Metrics that must exist:** consumer lag per group/partition and **backlog age** (lag ÷ consume rate, alarmed against retention); produce p99 and error rate by error type (`NotEnoughReplicas` spikes = ISR shrink); under-replicated partitions (the single best broker-health signal — nonzero and rising = a broker is falling behind); ISR shrink/expand rate (flapping = network or GC trouble); rebalance frequency per group; DLQ inflow rate; end-to-end pipeline latency via a heartbeat/tracer message published every second and timed at each consumer (measures the *pipeline*, not just Kafka).

**Consumer lag & backpressure.** Lag = latest offset − committed offset, the single most important health metric. Runbook: lag rising + consumer CPU low ⇒ downstream (DB) is the bottleneck — scale that, not consumers. Lag rising + consumers maxed ⇒ scale group (up to partition count). Backlog age approaching retention ⇒ data-loss risk: raise retention now, then fix throughput. Broker itself never buffers in memory per-consumer (log is on disk) — this is why slow consumers are safe in Kafka but dangerous in RabbitMQ (unbounded queue growth in RAM until flow-control kicks in / node falls over).

**Kafka replication internals — ISR, high watermark, acks, unclean election.** Each partition has a leader and followers; the **ISR** (in-sync replica set) is the leader plus followers within `replica.lag.time.max.ms` (default 30 s) of the log end. The **high watermark (HW)** is the minimum log-end offset across the ISR; consumers can only read up to HW — records past it are appended-but-uncommitted and invisible. `acks=all` means the leader acks after every *current ISR member* has the record — crucially, combined with `min.insync.replicas=2`: if the ISR shrinks to just the leader, produces are rejected (`NotEnoughReplicasException`) instead of `acks=all` silently degenerating to `acks=1` — that pairing is the actual durability guarantee, and interviewers probe exactly this: *`acks=all` alone guarantees nothing if the ISR can shrink to one*. On leader failure the controller elects a new leader **from the ISR**, so no acked record is lost. `unclean.leader.election.enable=false` forbids electing an out-of-sync replica when the whole ISR is gone: the partition goes offline (unavailable) rather than resurrecting with a truncated log — CP over AP on that partition; setting it `true` is a business decision to prefer availability and accept silent loss (defensible for clickstream, never for orders). Followers that recover truncate their log to the leader's (leader-epoch fencing prevents the old "truncate to HW" divergence bugs of pre-0.11 Kafka).

**Produce-request lifecycle under `acks=all` (narrate this end to end):**

```
1. App calls send() → record enters the producer's in-memory accumulator,
   batched per partition (linger.ms=5, batch.size=64KB), compressed (zstd).
2. Sender thread picks full/expired batches, attaches (PID, epoch, seq),
   sends ProduceRequest to the partition leader.
3. Leader validates the sequence number (idempotence dedupe/reorder check),
   appends to its local log (page cache; fsync policy is flush-by-OS —
   durability comes from replication, not fsync).
4. Followers in the ISR fetch-replicate the records (they pull, like
   consumers); each append advances that follower's log-end offset.
5. Leader advances the high watermark to min(ISR log-end offsets); once
   HW covers the batch AND ISR ≥ min.insync.replicas, the leader responds.
6. Producer receives the ack; on retriable error it resends the SAME
   (PID, seq) — broker dedupes, so retries cannot duplicate or reorder.
7. Consumers may now see the records (reads capped at HW;
   read_committed additionally capped at LSO).
```
The two facts interviewers fish for: Kafka's durability is *replication-based, not fsync-based* (a power loss across the whole rack can lose page-cache data on all replicas — hence rack-aware replica placement), and the HW advance in step 5 is what makes "acked ⇒ survives one broker loss" true.

**Exactly-once mechanics, one level deeper.** Two independent machines: (1) **Idempotent producer** — broker assigns a Producer ID (PID); every batch carries `(PID, epoch, sequence)` per partition; the broker accepts only the next expected sequence, silently deduping retried batches and rejecting reordered ones — this is what makes `max.in.flight=5` safe and is free (on by default in Kafka ≥ 3.0); it deduplicates *retries within a producer session*, nothing more. (2) **Transactions** — producer declares a `transactional.id`; a transaction coordinator (backed by the `__transaction_state` topic) tracks a two-phase protocol: `beginTxn` → produce to N partitions → `sendOffsetsToTransaction` (the consumed offsets join the same transaction — this is the read-process-write atomicity) → `commitTxn` writes commit markers into each partition. `read_committed` consumers buffer past the **LSO** (last stable offset) and skip aborted records. The `transactional.id` also provides **zombie fencing**: a restarted producer bumps the epoch, and the coordinator rejects the old instance's late commits. Costs to name: coordinator round trips per commit (batch transactions to ~100 ms windows), consumer latency floor = transaction commit interval, and the guarantee's boundary — the moment the "write" is an external HTTP call, transactions can't help; only sink-side idempotency can.

**Consumer rebalancing protocols — eager vs cooperative, precisely.**

| | Eager (range/round-robin assignors) | Cooperative sticky (`CooperativeStickyAssignor`) |
|---|---|---|
| On any membership change | **Every** consumer revokes **all** partitions, rejoins, gets a fresh assignment | Two-phase: only partitions that must *move* are revoked; the rest keep processing |
| Pause duration | Full stop-the-world for the whole group (seconds to minutes on big groups) | Proportional to moved partitions only |
| Stickiness | None (range) / accidental | Deliberate — minimizes movement, preserves warm state (local caches, DB connections) |
| Duplicate window | Large — all in-flight batches interrupted | Small — only moved partitions' in-flight work |
| Protocol | Single join/sync round | Two rebalance rounds (revoke-then-assign), converges incrementally |

Add **static membership** (`group.instance.id=pod-name`): the coordinator remembers the instance across restarts within `session.timeout.ms`, so a rolling pod restart triggers zero rebalances — the returning pod just resumes its old assignment. KIP-848 (the next-gen consumer protocol) moves assignment computation broker-side and makes rebalancing fully incremental without a group-wide barrier — worth name-dropping as the direction of travel. Tuning triangle to recite: `session.timeout.ms` (liveness detection, heartbeat-thread based), `max.poll.interval.ms` (processing liveness — exceeded ⇒ proactive leave), `heartbeat.interval.ms` (≈ ⅓ of session timeout); confusing the first two is the most common consumer-ops bug.

**Rebalance storms.** Symptom: group loops join/leave, no progress. Causes: processing a batch exceeds `max.poll.interval.ms` (coordinator assumes death — lower `max.poll.records` or raise interval); GC pauses exceed `session.timeout.ms`; flapping pods. Fixes: cooperative-sticky assignor (only moved partitions stop), static membership (restart ≠ leave), health-check-before-join deploys.

**Hot partitions.** `hash(merchant_id)` when one merchant is 30% of traffic ⇒ one partition at 30% of topic load. Fixes: composite key `merchant_id + order_id` when merchant-level ordering isn't actually required (interrogate the ordering requirement — it's usually per-order, not per-merchant); or two-tier topics (whales get a dedicated topic).

**Poison message anatomy.** Distinguish: *transient* (downstream timeout — retry), *permanent* (unparseable — straight to DLQ), *wedged* (handler infinite-loops/OOMs — needs processing timeout wrapper + circuit). DLQ hygiene: alert on DLQ rate (a DLQ nobody watches is a data-loss device with extra steps), store original topic/partition/offset/exception in headers, redrive tool replays to the original topic *with the original message_id* so dedupe still protects double-redrive.

**Poison pills at the deserialization layer.** The nastiest poison pill fails *before* your handler runs: `poll()` itself throws on an undeserializable record, and a naive loop crashes, restarts, seeks to the same offset, and crash-loops forever — the partition is wedged and so is the pod. Defenses in order: (1) consume as `byte[]` and deserialize inside your own try/catch (the design above does this via `codec.decode` — say so); (2) if using framework deserializers, wrap them (Spring's `ErrorHandlingDeserializer` pattern): failures surface as a typed error record routed to DLQ instead of an exception in `poll()`; (3) an OOM-class pill (a 50 MB record that kills the JVM on decode) needs `fetch.max.bytes`/`max.partition.fetch.bytes` caps and a max-record guard before allocation; (4) last-resort operational tool: a "skip offset" runbook (seek past the wedged offset after copying the raw bytes to the DLQ manually). Test for it explicitly — inject a garbage record in staging chaos tests; most teams discover this failure mode in production.

**Priority-queue emulation on Kafka.** Kafka has no per-message priority — the log is FIFO per partition, full stop. Patterns, weakest to strongest: (1) **priority topics** — `orders.p0` / `orders.p1` / `orders.p2`, consumers poll p0 first and only drain p1/p2 when p0 is empty (or with weighted budgets, e.g., 70/20/10 per poll cycle to prevent p2 starvation); simple, coarse, the right default; (2) **dedicated consumer capacity** — same topics, but separately scaled consumer groups per tier, so a p2 backlog can never consume p0's capacity (stronger isolation, more infra); (3) **broker swap for the priority slice** — route the genuinely latency-critical minority (say < 1% of traffic) through RabbitMQ priority queues or SQS with separate queues while the bulk stays on Kafka — heterogeneous, but honest about tool fit. Anti-pattern to call out: single topic with a priority header and consumers that re-sort in memory — it breaks offset semantics (you can't commit past messages you deferred) and rebuilds a priority queue in the worst possible place. Also note the requirement smell: "priority" often really means *"the backlog shouldn't delay urgent work"* — which tiered topics solve — not per-message preemption.

**Schema registry & compatibility, concretely.** The registry stores versioned schemas per *subject* (typically `<topic>-value`); producers register/resolve a schema ID and prepend it to each payload (magic byte + 4-byte ID); consumers resolve the ID → schema from a local cache. Compatibility modes and what they permit:

| Mode | Consumers using old schema can read new data? | Allowed changes | Deploy order |
|---|---|---|---|
| `BACKWARD` (default, chosen) | Yes — new writer, old reader | Delete fields; add optional/defaulted fields | Consumers first |
| `FORWARD` | Old writer, new reader | Add fields; delete optional/defaulted | Producers first |
| `FULL` | Both directions | Only add/remove optional-with-default | Either |
| `NONE` | No guarantee | Anything | You own the outage |

`*_TRANSITIVE` variants check against **all** prior versions, not just the latest — use them, since consumers in a 7-day-replay world read arbitrarily old records. Enforced at CI (schema PR gate) *and* at registration time (registry rejects incompatible schemas), so an incompatible producer can't even start publishing. Rules that prevent the classic wrecks: never reuse or renumber a Protobuf field / never change an Avro field type in place; evolve by adding optional fields with defaults; breaking change ⇒ new topic (`order.events.v2`) with a migration period of dual-publish or a translator consumer. Failure containment: the registry sits on the produce/consume *setup* path only — clients cache schemas by ID with effectively infinite TTL, so a registry outage stalls new schema deployment, not steady-state traffic.

**Thundering herd on recovery.** Downstream DB comes back after 10 min; consumers rip through 10 min of backlog at max speed and knock it over again. Fix: rate-limit consumers (token bucket per instance) at the sink's known safe throughput; drain deliberately, not maximally.

**Failure walkthrough:**
- **Broker dies:** partitions with leaders there fail over to ISR followers (~seconds); producers/consumers retry through metadata refresh; no acked data lost (min.insync=2).
- **Two of three replicas die:** partition rejects produces (`NotEnoughReplicas`) — availability sacrificed for durability; producers buffer/backpressure upstream; order API can degrade to "accepted, processing delayed" (outbox holds events).
- **Consumer instance dies:** its partitions reassigned within session timeout; in-flight uncommitted batch redelivered elsewhere; dedupe absorbs.
- **CDC relay dies:** events accumulate in outbox (durable); relay resumes from binlog position; burst on recovery — producer-side rate cap.
- **Dedupe store dies:** consumers must stop (fail closed) or accept duplicates (fail open) — per-topic policy: payments fail closed, notifications fail open (a rare duplicate email beats no emails).
- **Schema registry down:** producers/consumers use cached schemas — cache with long TTL, registry is not on the hot path.
- **Zombie consumer** (paused by GC, thinks it still owns partition 7, writes to sink after reassignment): fencing — sink writes carry epoch/generation, or rely on dedupe + version checks. This is the subtle one interviewers love.

## 8. Trade-off Summary & Interview Soundbites

| Decision | Trade-off accepted |
|---|---|
| Kafka over SQS/RabbitMQ | Real ops burden, for replay + fan-out + per-key ordering at scale |
| At-least-once + idempotent consumers | Dedupe storage & discipline, instead of chasing wire-level exactly-once |
| Partition by order_id | Hot-key skew risk + fixed parallelism ceiling, for per-entity ordering |
| Outbox + CDC | Extra table + relay component, to kill the dual-write problem |
| Retry topics over in-place blocking retry | Per-key ordering weakened during retries (version checks compensate), to avoid head-of-line blocking |
| acks=all, min.insync=2, no unclean election | Reduced availability under double failure, for zero acked-loss |
| Manual post-batch offset commit | Redelivery duplicates on crash, absorbed by idempotency |
| 48 partitions up front | Idle parallelism early, to avoid key-remapping later |
| Consumer rate limiting at sinks | Slower backlog drain, to prevent recovery herding |
| Cooperative-sticky + static membership | Two-round rebalance protocol complexity, for near-zero-pause deploys |
| BACKWARD_TRANSITIVE schema compatibility, CI-enforced | Slower schema evolution (consumers deploy first), for zero broken-consumer incidents |
| Priority via tiered topics, not per-message priority | Coarse-grained priority only, to keep offset/ordering semantics intact |
| byte[] consumption + own-codec decode | A little boilerplate per consumer, to make deserialization poison pills routable instead of crash-loops |
| Kafka transactions only for Kafka→Kafka stages | External sinks still need idempotency keys, to avoid pretending EOS crosses system boundaries |

**Soundbites:**
1. "Exactly-once delivery doesn't exist on a network; exactly-once *processing* is at-least-once plus idempotency — I build the latter and stop arguing about the former."
2. "Partition count is set by consumer throughput, not broker throughput — the broker was never the bottleneck."
3. "The outbox pattern makes 'save and publish' atomic by making publish a database write."
4. "A queue is a buffer, and every buffer needs a policy for when it's full: Kafka's answer is disk + retention; make sure yours isn't 'OOM'."
5. "Retry with jitter, or your retries arrive as synchronized waves and become the second outage."
6. "A DLQ without an alert is just a slow /dev/null."
7. "Lag rising with idle CPUs means the bottleneck is downstream — scaling consumers would just aim the firehose better."
8. "Interrogate ordering requirements: 'ordered' almost always means per-entity, and per-entity ordering is nearly free."
9. "acks=all without min.insync.replicas=2 is a durability guarantee that evaporates exactly when you need it — the two settings are one decision."
10. "Kafka's durability is replication, not fsync — which is why replicas belong in different racks, not just different processes."
11. "Eager rebalancing stops the world for the whole group to move one partition; cooperative-sticky moves the one partition."
12. "The registry that can't reject an incompatible schema at CI will let a producer break every consumer at 2 a.m. instead."
13. "Alert on backlog age against retention, not on lag count — lag is a number, backlog age is a deadline."
14. "A dedupe horizon shorter than the redelivery horizon turns 'exactly-once' into a comment in the design doc."

**Common follow-ups:**
- *"How do you replay just one customer's events?"* You can't seek by key — replay the time range with a filtering consumer, or maintain a keyed materialized view (compacted topic / event store) alongside.
- *"Compacted topics?"* Retain latest value per key — a changelog for state restoration (consumer groups' `__consumer_offsets` is one); not for event history.
- *"How big can a message be?"* Keep ≤1 MB; for larger payloads use claim-check pattern: blob to S3, pointer in the message.
- *"Kafka without ZooKeeper?"* KRaft mode — metadata quorum inside Kafka; operationally simpler, the modern default.
- *"Priority messages?"* Kafka has no priorities — separate topics per priority class with weighted consumer capacity; if you truly need per-message priority, that's a RabbitMQ-shaped problem.
- *"When would you go back to a DB-table queue?"* Sub-1K msg/s, single consumer service, transactional enqueue wanted for free — it's simpler and correct; graduate when polling load or fan-out needs appear.
- *"A consumer group is processing duplicates constantly, not just on crashes — where do you look?"* Ordered: (1) rebalance loop — check group state churn and `max.poll.interval.ms` vs actual batch processing time (each forced leave redelivers the in-flight batch); (2) commit failures — `CommitFailedException` swallowed in logs means offsets never advance; (3) retry-topic re-entry publishing a *new* `message_id` instead of propagating the original (breaks dedupe by construction — the redrive/retry path must preserve identity); (4) dedupe-table pruning shorter than the redelivery horizon (a 24 h prune with a 7-day replay = "duplicates" that are really un-deduped replays). The fix is rarely "add more dedupe" — it's stopping the redelivery source.
- *"How do you run this across two regions?"* Decide the topology first: **active-passive** (MirrorMaker 2 / Confluent Replicator async-replicates topics; failover accepts a small unreplicated tail → consumers rely on idempotency to absorb the overlap after offset translation — MM2's checkpoint topic maps offsets between clusters, and it's approximate) vs **active-active** (each region produces locally to region-prefixed topics, both consume both — no failover, but per-key ordering only holds within a region, so route each entity's writes to a home region). Never stretch one Kafka cluster across high-latency WAN links (replication and controller quorums degrade); stretch clusters are for ≤ 2 ms metro links. The outbox helps again here: events survive in the source DB regardless of broker-replication gaps.
- *"Consumer needs to call a rate-limited third-party API at 100 req/s but the topic peaks at 7 K msg/s — design it."* Don't fight it with consumer count. The queue *is* the buffer: run few consumers with a shared token bucket at 95 req/s, let lag grow during peaks, and alert on *backlog age vs retention* (data-loss horizon) rather than lag count. If peak sustained rate exceeds the API quota long-term, no queue saves you — shed (sample), batch (if the API has a bulk endpoint), or renegotiate the quota; a queue converts a rate mismatch into latency only when the *average* rate fits.
