# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 21 of 63 — Phase 1: Foundations (Module 21 of 28)
**Module:** Event-Driven Architecture & Kafka
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
Almost every large system you will design in Phase 2 — a payment pipeline, a notification fan-out, an activity feed, a fraud detector — is stitched together by an event log, and nine times out of ten that log is Apache Kafka. Kafka is how LinkedIn moves **7 trillion messages a day** and how Uber, Netflix, and most e-commerce checkouts decouple services so one slow consumer can't take down the whole order flow. If you can't explain what a partition is, why offsets live with the consumer, or how a replica gets promoted when a broker dies, you will freeze the moment an interviewer says "now make it asynchronous." Today we build that spine.

## Learning Objectives
By the end of this lesson you can:
- Explain the producer/consumer model and why decoupling via a log beats direct RPC calls.
- Describe how topics split into partitions and why partitions are the unit of parallelism and ordering.
- Trace how consumer groups divide partitions and how offsets let a consumer resume after a crash.
- Explain replication with a leader, followers, and the in-sync replica (ISR) set, and what `acks=all` guarantees.
- Reason about durability using retention, `log.flush`, and replication factor with real numbers.
- Connect Kafka to the classic observer pattern and to LinkedIn/e-commerce architectures.

## The Lesson

### Producer/Consumer Model
**What it is (plain English):** Instead of Service A calling Service B directly and waiting, A (the **producer**) writes a message to a shared, append-only log; B (the **consumer**) reads from that log whenever it's ready. The log sits in the middle as a buffer and a broker. Think of a restaurant kitchen: waiters (producers) clip orders onto a rail; cooks (consumers) pull them off at their own pace. The waiter never waits for the cook.

**The problem it solves:** Direct synchronous calls couple availability and speed. If B is down or slow, A blocks, threads pile up, and the failure cascades. With a log, A fires-and-forgets in ~1–2 ms and moves on; B catches up later. It also enables **fan-out**: ten different consumers can read the same order event for billing, email, analytics, and search indexing without the producer knowing any of them exist.

**How it works (mechanics):** A Kafka producer batches records, optionally keys them, and sends to a **broker**, which appends them to a partition's log segment on disk. Consumers **pull** (not push) in batches, tracking their position.
```
Producer --append--> [ Kafka topic: order-events ]
                          | partition-0: r0 r1 r2 r3 ...
   +----------------------+------------------------+
   v                      v                        v
Billing SVC          Email SVC               Analytics SVC   (independent readers)
```
**Trade-offs / when NOT to use it:** You gain decoupling but pay in eventual consistency and operational complexity — you now run a distributed log cluster. For a simple request that genuinely needs an immediate answer (e.g., "is this password correct?"), a synchronous RPC is simpler and lower-latency.

**Where you'll see it:** Kafka, RabbitMQ, AWS Kinesis, Google Pub/Sub. LinkedIn's entire activity backbone is producer/consumer over Kafka.

### Topics & Partitions
**What it is (plain English):** A **topic** is a named stream, like `order-events`. Each topic is split into **partitions** — independent ordered logs. Partitioning is how one topic scales past the throughput of a single machine: 12 partitions can be spread across 12 brokers and written/read in parallel.

**The problem it solves:** A single append-only file is limited by one disk and one CPU core (~a few hundred MB/s). One partition might sustain ~10 MB/s of a specific workload; you need 100 MB/s. Split into 10+ partitions and you scale linearly. Partitions also give you **ordering guarantees where they matter**: Kafka guarantees order *within a partition*, not across the whole topic.

**How it works (mechanics):** The producer picks a partition by hashing the record **key**: `partition = hash(key) % numPartitions`. All events for `user_42` therefore land in the same partition and stay strictly ordered relative to each other.
```
topic order-events, 3 partitions:
  P0: [k=user_42:r0][k=user_42:r1][k=user_99:r2] ->
  P1: [k=user_7:r0 ][k=user_7:r1 ] ->
  P2: [k=user_13:r0] ->
hash("user_42") % 3 = 0  => always P0
```
**Trade-offs / when NOT to use it:** More partitions = more parallelism but also more open file handles, more leader elections on failure, and higher end-to-end latency (each partition adds metadata overhead). A cluster with 200,000+ partitions strains the controller. And you **cannot easily reduce** partition count later without breaking key-to-partition mapping. Pick a count with headroom (e.g., 30) but don't go wild.

**Where you'll see it:** Every Kafka deployment. Kinesis calls the same idea "shards."

### Consumer Groups & Offsets
**What it is (plain English):** A **consumer group** is a set of consumer instances that cooperate to read a topic, splitting the partitions among themselves so each partition is read by exactly one member. An **offset** is simply the integer position of the next record a consumer will read in a partition — like a bookmark.

**The problem it solves:** You want to scale reading horizontally *and* survive crashes without reprocessing everything or dropping messages. Groups give you scale (add consumers, get more parallelism up to the partition count); offsets give you resumability (a crashed consumer restarts from its last committed bookmark).

**How it works (mechanics):** With 6 partitions and 3 consumers in a group, Kafka assigns 2 partitions each. Add a 4th consumer → **rebalance** → now 2 consumers get 2 partitions, 2 get 1. Add a 7th consumer → one sits idle (partitions are the cap on parallelism). Each consumer periodically **commits** its offset to the internal `__consumer_offsets` topic.
```
partitions P0..P5, group=billing (3 consumers)
  C1 -> P0,P1   C2 -> P2,P3   C3 -> P4,P5
C2 crashes -> rebalance -> C1 -> P0,P1,P2  C3 -> P3,P4,P5
resume: C1 reads P2 from last committed offset (say 10,842)
```
**Trade-offs / when NOT to use it:** Offset commit timing sets your delivery semantics. Commit *before* processing → **at-most-once** (may lose a message on crash). Commit *after* → **at-least-once** (may reprocess duplicates; make consumers idempotent). Rebalances also cause brief pauses ("stop-the-world") where no consumption happens.

**Where you'll see it:** Kafka Streams, Flink, and every microservice reading Kafka. Different groups (billing, search) each get a full independent copy of the stream.

### Replication & Fault Tolerance
**What it is (plain English):** Each partition is copied to multiple brokers. One copy is the **leader** (handles all reads/writes); the others are **followers** that replicate the leader. If the leader's broker dies, a follower is promoted so no data is lost and the partition stays available.

**The problem it solves:** Disks and machines fail. Without replication, one dead broker means that partition's data is gone or unavailable. Replication factor 3 means you can lose 2 brokers and still serve the partition.

**How it works (mechanics):** Followers pull from the leader and, once caught up, join the **ISR** (in-sync replica set). A producer with `acks=all` only gets an ack once *all ISR members* have the record. `min.insync.replicas=2` with RF=3 means: tolerate 1 broker down and still accept writes; if 2 are down, refuse writes rather than risk data loss.
```
partition P0, RF=3:
  Broker1 [LEADER]  offset=1050
  Broker2 [FOLLOWER] offset=1050  } ISR = {B1,B2,B3}
  Broker3 [FOLLOWER] offset=1050  }
  B1 dies -> controller elects B2 as new leader (from ISR) -> no data loss
```
**Trade-offs / when NOT to use it:** `acks=all` costs latency — you wait for replication (adds a few ms) — versus `acks=1` (leader only, faster but can lose data if the leader dies before followers catch up). RF=3 also triples storage. For truly disposable data (transient metrics), RF=1 or 2 and `acks=1` may be fine.

**Where you'll see it:** Kafka's controller (KRaft/ZooKeeper) manages this. The same leader/follower ISR pattern appears in Elasticsearch and MongoDB replica sets.

### Durability
**What it is (plain English):** Durability means once Kafka acks your write, that data survives crashes and is readable later — even days later. Kafka is not just a queue that deletes on read; it's a **retained log**. Consumers can rewind and replay.

**The problem it solves:** Traditional queues delete a message once consumed, so a bug in a downstream consumer means the data is gone forever. Kafka keeps messages for a configured retention (e.g., 7 days), so you can fix the bug and replay, or add a brand-new consumer that reads history from offset 0.

**How it works (mechanics):** Durability rests on three levers: **replication** (RF=3 across brokers), **retention** (`retention.ms=604800000` = 7 days, or size-based `retention.bytes`), and **flush behavior**. Kafka relies mostly on the OS page cache and replication rather than fsync-per-message for speed, trusting that RF=3 means a record survives even if one machine loses its unflushed page cache in a crash.
```
Numbers: 1M orders/day x 1KB = 1 GB/day raw.
RF=3 -> 3 GB/day on disk. 7-day retention -> ~21 GB stored.
Replay from offset 0 rebuilds a new search index from a week of history.
```
**Trade-offs / when NOT to use it:** Long retention costs disk. Relying on replication instead of per-message fsync means a *simultaneous* power loss across all 3 ISR brokers could lose the last few unflushed ms — extremely rare but not zero. For financial ledgers you may still want an fsync'd system of record downstream.

**Where you'll see it:** Event sourcing, CDC (Debezium), and "replay to rebuild state" architectures all depend on Kafka durability.

### The Observer Pattern
**What it is (plain English):** The observer pattern is the object-oriented ancestor of event-driven systems: a **subject** maintains a list of **observers** and notifies them all when its state changes, without knowing what they do. Kafka is this pattern scaled across machines — the topic is the subject, consumer groups are observers.

**The problem it solves:** It removes hard-coded dependencies from a source to its listeners. Instead of the order service calling `email.send()`, `analytics.log()`, `search.index()` in a rigid chain, it just emits "order placed" and any number of observers subscribe. You add a new observer without touching the producer.

**How it works (mechanics):** In-process, it's a `subject.subscribe(observer)` list and a `notifyAll()` loop. Distributed, "subscribe" becomes "join a consumer group on the topic," and "notify" becomes "append to the log; each group reads independently."
```
In-process:            Distributed (Kafka):
Subject.notifyAll()    producer.send(topic)
  -> obsA.update()       -> group A reads all records
  -> obsB.update()       -> group B reads all records
```
**Trade-offs / when NOT to use it:** In-process observers run synchronously in the subject's thread — a slow observer blocks the subject, and an exception can break the chain. That coupling of failure and timing is exactly why we graduate to an async broker at scale. But for a single UI widget reacting to a model change, the full-blown Kafka machinery is absurd overkill.

**Where you'll see it:** GUI frameworks (React state, Java Swing listeners), `EventEmitter` in Node.js, and — scaled up — every Kafka pub/sub topology.

### How LinkedIn and E-commerce Platforms Use Kafka
**What it is (plain English):** Kafka was born at LinkedIn (2011) to solve "every service needs every other service's data." E-commerce platforms use the same idea to decouple checkout from the dozen things that must happen after an order.

**The problem it solves:** At LinkedIn, N services each producing data that M others need created an N×M spaghetti of point-to-point pipelines. A central log turns that into N producers + M consumers on shared topics — linear, not quadratic. In e-commerce, a synchronous checkout that must call inventory, payment, email, loyalty, and fraud inline would be slow and fragile; one failing dependency fails the sale.

**How it works (mechanics):** LinkedIn emits page views, connection events, and profile updates to Kafka topics; consumers feed the feed, search index, analytics (Hadoop), and metrics — at **~7 trillion messages/day**. E-commerce: checkout writes one `order-placed` event, then independent consumers act.
```
[checkout] --order-placed--> Kafka
   -> inventory-svc (decrement stock)
   -> payment-svc (charge)
   -> email-svc (confirmation)
   -> fraud-svc (score async)
Checkout returns in ~20ms; downstream work happens async.
```
**Trade-offs / when NOT to use it:** Eventual consistency bites — the customer sees "order placed" before payment fully settles, so you need sagas/compensation for failures. Small shops with 10 orders/day gain nothing from this and should just use a database transaction.

**Where you'll see it:** LinkedIn, Uber (trip events), Netflix (viewing events), Shopify, and virtually every checkout at scale.

## Comparison Table

| Dimension | Synchronous RPC | Kafka (event-driven) |
|---|---|---|
| Coupling | Tight (caller waits) | Loose (fire-and-forget) |
| Failure blast radius | Cascades to caller | Isolated; buffered in log |
| Latency of caller | Sum of downstream | ~1–2 ms to produce |
| Replay / new consumers | Not possible | Read history from offset 0 |
| Consistency | Strong, immediate | Eventual |
| Best for | "Need answer now" | Fan-out, pipelines, decoupling |

**Verdict:** Use RPC when the caller genuinely needs the result to proceed; use Kafka when work can happen asynchronously and multiple parties care about the same event.

## Common Misconceptions
- **Myth:** Kafka guarantees global ordering across a topic. → **Reality:** Ordering is guaranteed only *within a partition*. Cross-partition order is not defined.
- **Myth:** A message is deleted once a consumer reads it. → **Reality:** Kafka retains messages by time/size; many independent groups can read the same record, and you can replay.
- **Myth:** More partitions is always better. → **Reality:** Too many partitions strain the controller, slow rebalances, and raise latency. Size deliberately.
- **Myth:** The broker tracks each consumer's position. → **Reality:** Consumers own their offsets (stored in `__consumer_offsets`); the broker just serves the log.
- **Myth:** `acks=all` means "written to disk." → **Reality:** It means "acknowledged by all in-sync replicas," which for Kafka usually means page cache + replication, not necessarily fsync.

## Real-World Case
LinkedIn built Kafka in 2010–2011 precisely because their data pipelines had become an N×M nightmare: dozens of services each needed a custom feed of another's data, and every new consumer meant a new bespoke pipeline. Jay Kreps and team replaced this with a single distributed commit log where any service produces once and any number of teams consume independently. It scaled from a monitoring side-project to the backbone of the entire company, and by the late 2010s handled over **7 trillion messages per day** across thousands of brokers. The design insight — "make the log the source of truth, not the database" — later became the foundation of event sourcing, stream processing (Kafka Streams, Flink), and change-data-capture across the industry. It's arguably the single most influential piece of distributed-systems infrastructure of the 2010s.

## Self-Test (answers at the bottom)
1. What does a consumer group guarantee about how partitions are assigned to its members?
2. A topic has 8 partitions. You run a group with 12 consumers. How many are actively consuming, and why?
3. With RF=3 and `min.insync.replicas=2`, how many broker failures can you tolerate while still accepting writes? What happens on the next failure?
4. You committed offsets *after* processing and your consumer crashed mid-batch. What delivery semantic do you have, and what must the consumer be to stay correct?
5. Design sketch: An e-commerce checkout must trigger inventory, payment, email, and fraud scoring. Design the Kafka topology (topics, partitions, keys, consumer groups). How do you keep all events for one order ordered, and how do you handle a payment failure after "order placed" was already emitted?

## Interview Soundbites
- "Kafka guarantees ordering per partition, not per topic — so I key by entity ID to keep one user's events strictly ordered while still parallelizing across partitions."
- "Consumers own their offsets, which is why a crashed consumer resumes exactly where it left off, and why offset-commit timing decides at-most-once versus at-least-once."
- "With `acks=all` and RF=3, `min.insync.replicas=2`, I tolerate one broker down and still accept writes; on the second failure I'd rather reject writes than silently lose data."

## Mini-Assignment
On paper (~30 min): Design the event flow for a food-delivery order. (1) Draw a topic `order-events` with 6 partitions and show how keying by `order_id` keeps each order's events ordered. (2) Define three consumer groups (restaurant-notify, courier-dispatch, analytics) and assign partitions for a 3-consumer group. (3) Now assume RF=3; write out what happens to partition 0 when its leader broker crashes — which replica gets promoted and what the producer with `acks=all` experiences. (4) Sketch how you'd replay the last 24 hours to rebuild a broken analytics table.

## Recap & Tomorrow
- **Producer/consumer model:** decouple via an append-only log so slow/failed consumers don't block producers.
- **Topics & partitions:** partitions are the unit of parallelism and per-partition ordering; key = `hash % partitions`.
- **Consumer groups & offsets:** partitions split one-per-consumer; offsets are resumable bookmarks that set delivery semantics.
- **Replication & fault tolerance:** leader + followers + ISR; `acks=all` with `min.insync.replicas` trades latency for safety.
- **Durability:** retention + replication let you store and replay days of history.
- **Observer pattern:** Kafka is the observer pattern scaled across machines.
- **LinkedIn / e-commerce:** the central log turns N×M spaghetti into N producers + M consumers.

Tomorrow, **Lesson 22 — Performance Tuning Techniques**: multithreading, connection pooling, batching, compression, async I/O, pagination, lazy loading, read replicas, denormalization, and precomputation — the levers that turn a 2-second endpoint into a 20-millisecond one.

## Self-Test Answers
1. Each partition is assigned to exactly one consumer in the group, so within a group no two consumers read the same partition — that's how work is split without duplication.
2. Only 8 are active; the other 4 sit idle. Partitions are the cap on parallelism — you can't have more active consumers in a group than partitions.
3. You can tolerate 1 failure (2 of 3 ISR still meet `min.insync.replicas=2`). On the second failure only 1 replica remains, which is below the minimum, so the broker rejects new writes to protect durability (reads can still serve).
4. At-least-once: because the offset wasn't committed, on restart the batch is reprocessed, so some records may be handled twice. The consumer must be idempotent (e.g., upsert by order_id) so duplicates don't cause double effects.
5. One topic `order-events` keyed by `order_id` (so all events for an order hit the same partition and stay ordered), say 12 partitions. Four consumer groups — inventory, payment, email, fraud — each read the full stream independently. Checkout emits `order-placed` and returns fast. On payment failure, the payment consumer emits a compensating `order-cancelled`/`payment-failed` event (a saga); inventory and email consumers subscribe and reverse their actions (restock, send failure notice). This is eventual consistency with compensation, not a distributed transaction.
