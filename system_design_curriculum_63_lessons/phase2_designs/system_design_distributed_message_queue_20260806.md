# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 39 of 63 — Phase 2: System Design Track (Design 11 of 35)
**Topic:** Distributed Message Queue (Kafka-like)
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
A distributed message queue is the circulatory system of modern architecture: LinkedIn built Kafka to move 7 trillion messages/day, and it now underpins event sourcing, stream processing, and log aggregation at nearly every large company. The hard part is not "store messages" — it is delivering an append-only log at millions of writes/sec with durability guarantees, strict per-key ordering, and consumer groups that can rebalance without losing or double-processing data. Get the offset-commit semantics wrong and you either drop payments or bill customers twice.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Kafka & event-driven architecture (taught in Phase 1, Lesson 21):** Kafka is an append-only, partitioned, replicated commit log. Producers write to topic partitions; each partition is an ordered, immutable sequence addressed by a monotonically increasing offset. Consumers pull and track their own offset — the broker is dumb, the consumer is smart. This is the entire foundation of today's design; we are essentially reconstructing Kafka from first principles.

**Replication & quorum (taught in Phase 1, Lesson 3):** A partition has one leader and N-1 followers forming an ISR (in-sync replica) set. Writes go to the leader; followers pull to replicate. `acks=all` means the leader waits for all ISR members before acknowledging. We use this to guarantee no data loss on broker failure, and `min.insync.replicas` to trade availability for durability.

**Leader election (taught in Phase 1, Lesson 17):** Each partition needs exactly one leader. Historically ZooKeeper held the cluster metadata and elected a controller broker; the controller then elects partition leaders from the ISR. Modern Kafka (KRaft) replaces ZooKeeper with a Raft quorum of controllers. We rely on this for automatic failover when a leader broker dies.

**Consistent hashing (taught in Phase 1, Lesson 19):** Used two ways — to map a message key to a partition (`hash(key) % num_partitions`, which is NOT consistent hashing but a fixed modulo), and conceptually for consumer-group partition assignment. We'll discuss why Kafka uses fixed modulo for partitioning and the painful consequences when you change partition count.

**B-tree vs LSM / storage engines (taught in Phase 1, Lesson 23):** Kafka's storage is neither — it's a segmented append-only log with a sparse offset index. Sequential disk writes hit ~600 MB/s even on spinning disks, far faster than random B-tree writes. Understanding sequential I/O and the page cache is central to why Kafka is fast.

**Caching & the OS page cache (taught in Phase 1, Lesson 24):** Kafka deliberately does NOT maintain an application-level cache. It writes to the page cache and lets the OS flush; reads for recent data are served from page cache via `sendfile()` (zero-copy). This is the single biggest performance lever in the design.

**Capacity estimation (taught in Phase 1, Lesson 28):** We'll size partitions, replication storage, and network throughput. Keep the arithmetic of throughput = message_size × rate, and storage = throughput × retention × replication_factor at your fingertips.

> **Recap box:**
> - Partition = ordered, immutable, offset-addressed log.
> - ISR + acks=all + min.insync.replicas = your durability knob.
> - Controller elects partition leaders from ISR; consumers track their own offset.
> - Sequential I/O + page cache + zero-copy sendfile = the speed.
> - Partitioning is fixed modulo, not consistent hashing — changing partition count reshuffles keys.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. What is the difference between a message queue (like RabbitMQ/SQS) and a log-based system (like Kafka)? Why does the distinction matter?
> **What a strong answer covers:** A traditional queue treats messages as transient — once acknowledged and delivered, the message is deleted; the broker tracks per-message delivery state. A log (Kafka) is a durable, replayable, append-only sequence retained for a configured period (e.g., 7 days) regardless of consumption; consumers track a cursor (offset) and can rewind. Consequences: Kafka supports multiple independent consumer groups reading the same data, replay for reprocessing, and much higher throughput because the broker does no per-message bookkeeping. RabbitMQ gives richer routing (exchanges, per-message TTL, priority) and per-message ack/nack.
> **Common weak answer:** "Kafka is just a faster queue." Misses that the retention/replay model is a fundamentally different data structure, and that Kafka pushes ordering/dedup responsibility onto the consumer.
> **Mentor follow-up if they answer well:** If the log is retained anyway, why does Kafka still need consumers to commit offsets at all — why not just always read from the start?

Q2. Estimate the storage and network for a click-stream topic: 500K messages/sec, average 1 KB/message, replication factor 3, 7-day retention. Show the arithmetic.
> **What a strong answer covers:** Ingest = 500,000 × 1 KB = 500 MB/s = 0.5 GB/s of raw producer traffic. Per day = 0.5 GB/s × 86,400 s ≈ 43.2 TB/day of logical data. Over 7 days = ~302 TB logical. With RF=3, on-disk = ~907 TB ≈ 0.9 PB. Network: producer writes 0.5 GB/s to leaders; replication traffic adds 2× (two followers) = another 1 GB/s inter-broker; each consumer group adds 0.5 GB/s egress. Cross-check: this needs on the order of 30–50 brokers with ~24 TB disk each, and 10–25 GbE NICs. Note compression (LZ4/zstd) typically cuts 3–5×, so realistically ~200–300 TB on disk.
> **Mentor follow-up:** Where does the replication network traffic actually flow, and why does under-provisioning inter-broker bandwidth silently shrink your ISR?

Q3. A producer sets `acks=1`. What exactly is it trading away, and when is that acceptable?
> **What a strong answer covers:** `acks=1` acknowledges once the leader writes to its local log (page cache), before followers replicate. Trade-off: if the leader crashes before a follower fetches that record and a follower is elected leader, the record is lost — a silent data-loss window. Acceptable for high-volume, loss-tolerant telemetry/metrics where throughput and latency matter more than the last few messages. For payments/orders use `acks=all` with `min.insync.replicas=2`.
> **Red flag answer:** "acks=1 means it's written to disk." No — it means written to the leader's log (page cache), not fsynced, and not replicated. Conflating "acked" with "durable" is the classic mistake.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Design the high-level architecture of a Kafka-like distributed message queue. Draw it.
> **Key components expected:** Producers (with partitioner + batching + compression), a cluster of Brokers each owning partition replicas, a Controller/metadata quorum (ZooKeeper or KRaft/Raft), Consumers organized into Consumer Groups with a Group Coordinator, and an offset-storage mechanism (`__consumer_offsets` internal topic).
> **Architecture diagram (text):**
```
                 ┌─────────────┐        ┌─────────────┐
   Producers ───▶│ Partitioner │──batch─▶   Broker 1   │  Leader P0, Follower P1
   (acks=all)    │  + LZ4/zstd │        │ ┌─────────┐ │
                 └─────────────┘   ┌───▶│ │ Log Seg │ │
                        │          │    │ │ +Index  │ │
                        ▼          │    │ └─────────┘ │
                  hash(key)%N ─────┘    └──────┬──────┘
                        │                      │ follower fetch (replication)
        ┌───────────────┼──────────────────────┼───────────────┐
        ▼               ▼                       ▼               ▼
   ┌─────────┐    ┌─────────┐             ┌─────────┐     ┌───────────┐
   │ Broker 2│    │ Broker 3│  ......      │ Broker N│     │ Controller│
   │ Ldr P1  │    │ Ldr P2  │             │         │     │  (KRaft   │
   │ Fol P2  │    │ Fol P0  │             │         │     │   Raft    │
   └─────────┘    └─────────┘             └─────────┘     │  quorum)  │
        ▲                                                 └───────────┘
        │ pull (fetch, long-poll)                         metadata: ISR,
        │                                                 leaders, configs
  ┌─────┴───────────────────────────────┐
  │        Consumer Group "billing"      │  Group Coordinator (a broker)
  │  C1←P0   C2←P1   C3←P2  (1:1 assign)  │  assigns partitions, tracks
  └──────────────────────────────────────┘  offsets in __consumer_offsets
```
> **What separates SDE2 from SDE3 here:** SDE2 draws producers→brokers→consumers. SDE3 immediately separates the *data plane* (partition logs, replication fetch) from the *control plane* (controller, ISR management, leader election, group coordination), explains that the controller is NOT in the write path, and notes that `__consumer_offsets` is itself just a compacted Kafka topic — the system stores its own metadata using its own primitives.

Q5. Trace a single message from `producer.send()` to a consumer processing it, end to end.
> **Expected trace:**
> 1. Producer serializes record; partitioner computes partition = `hash(key) % num_partitions` (or sticky/round-robin if key is null).
> 2. Record appended to an in-memory batch buffer keyed by (topic, partition). Batching waits up to `linger.ms` or until `batch.size`.
> 3. Batch compressed (LZ4), sent to the *leader* broker for that partition (producer learns the leader from cluster metadata).
> 4. Leader appends batch to the active log segment (page cache), assigns offsets, updates the offset index.
> 5. Followers in ISR issue fetch requests, pull the batch, append locally, and advance their log-end-offset. Leader advances the *high-water-mark* (HWM) to min(ISR log-end-offsets).
> 6. With `acks=all`, leader acks producer only once HWM covers the batch.
> 7. Consumer in a group issues fetch from its committed offset; broker uses the sparse index to seek and `sendfile()`s bytes past HWM directly from page cache to socket (zero-copy).
> 8. Consumer decompresses, processes, then commits the new offset to `__consumer_offsets` (auto or manual).
> **Tricky part:** The high-water-mark. Consumers can only read up to the HWM, never a leader's raw log-end-offset — otherwise they'd read records that could be lost on leader failover. Candidates who forget the HWM don't understand Kafka's read-consistency model.

Q6. Design the producer and consumer API. What are the key calls and their semantics?
> **Expected API design:**
> Producer: `send(topic, key, value, headers) -> Future<RecordMetadata{partition, offset, timestamp}>`; config `acks`, `enable.idempotence=true`, `max.in.flight.requests.per.connection`, `linger.ms`, `batch.size`. `flush()` to force-send buffered batches.
> Consumer: `subscribe(topics)` (dynamic group membership) vs `assign(partitions)` (manual); `poll(timeout) -> ConsumerRecords`; `commitSync()/commitAsync(offsets)`; `seek(partition, offset)` for replay; `pause()/resume()` for backpressure.
> Admin: `createTopic(name, partitions, replication_factor, configs)`.
> **What to push on:** Idempotency — `enable.idempotence=true` makes the producer attach a (producer-id, sequence-number) so the broker dedups retries within a session, giving exactly-once *produce*. Ask how `poll()` doubles as the group heartbeat and why a slow processing loop (exceeding `max.poll.interval.ms`) triggers a rebalance that kicks the consumer out. Push on commit timing: commit-before-process (at-most-once) vs process-before-commit (at-least-once).

---

### Data Modeling (Medium–Hard)
Q7. Design the on-disk storage format for a partition. How is a log physically laid out?
> **Expected schema:**
```
Partition dir:  /data/topic-3/           (topic "topic", partition 3)
  00000000000000000000.log     <- segment: base offset 0
  00000000000000000000.index   <- sparse offset->file-position index
  00000000000000000000.timeindex<- timestamp->offset index
  00000000000000368210.log     <- next segment: base offset 368210
  00000000000000368210.index
  ...
  leader-epoch-checkpoint

Record batch (in .log):
  baseOffset | batchLength | partitionLeaderEpoch | magic |
  crc | attributes(compression) | lastOffsetDelta | firstTimestamp |
  producerId | producerEpoch | baseSequence | records[...]

.index entry (sparse, every ~4KB):  relativeOffset(4B) -> filePosition(4B)
```
> **Index choices and why:** A *sparse* offset index (one entry per ~4 KB, not per record) keeps the index small enough to mmap entirely into memory; lookup is a binary search in the index to find the nearest file position, then a short linear scan. A separate time index enables offset-by-timestamp (`offsetsForTimes`) for retention and replay-from-time. Both are memory-mapped files.
> **Partitioning key and why:** The message key determines partition via `hash(key) % N`. Choose a key that (a) gives the ordering guarantee you need (all events for one user/entity land in one partition, hence ordered) and (b) distributes evenly. Partition count is the unit of parallelism and is effectively immutable once keyed data exists.

Q8. A consumer needs to seek to a specific offset (e.g., replay from offset 4,812,553). How is that lookup done efficiently, and how do you find "messages after 2 PM yesterday"?
> **Expected answer:** For offset lookup: binary-search the segment base offsets (filenames) to find the owning segment, then binary-search that segment's sparse `.index` to get the largest indexed offset ≤ target, giving a file position; `seek()` there and scan forward a few KB to the exact record. O(log segments) + O(log index) + tiny scan. For timestamp: binary-search the `.timeindex` per segment to map timestamp→offset, then do the offset lookup above. Both are served from mmap'd index files, so no extra disk seeks in the common case.
> **Trap:** Storing a global secondary index of offset→position for every message. It would be enormous and defeat the sequential-write design. The sparseness (accepting a tiny linear scan) is the whole point — do not "optimize" it into a dense index.

Q9. Kafka gives per-partition ordering but not cross-partition ordering, and offers configurable consistency. Frame the CAP trade-off. When does eventual consistency bite?
> **Expected answer:** Within a partition Kafka is CP-leaning: with `acks=all` + `min.insync.replicas=2`, an acked write is on ≥2 replicas and totally ordered by offset. Across partitions there is NO global order — that's the availability/scalability trade: you shard for throughput and accept that two events on different partitions have no defined relative order. Reads are bounded by HWM for consistency. The classic eventual-consistency surprise is *unclean leader election*: if `unclean.leader.election=true` and all ISR replicas die, Kafka may elect a lagging out-of-sync replica as leader — availability over consistency — silently truncating acked records. Set it false for durability-critical topics (you then lose availability until an ISR member returns).
> **Mentor pushback:** You set `min.insync.replicas=2`, RF=3. Two of three brokers for a partition go down. What happens to producers with `acks=all`? (Answer: producers get `NotEnoughReplicasException` and block/fail — the partition becomes read-only for durable writes. That's CP: you chose consistency, so you lost write availability. This is by design, and candidates must own that trade rather than being surprised by it.)

---

### Low-Level Design (Hard)
Q10. Design consumer-group partition assignment and rebalancing. How are partitions distributed, and what happens when a consumer joins or dies?
> **Problem statement:** N partitions must be assigned to M consumers in a group such that each partition has exactly one owner within the group, assignments rebalance on membership change, and no two consumers process the same partition simultaneously (which would break ordering and cause duplicates).
> **Naive solution:** A coordinator statically maps partitions to consumers and, on any change, stops the world, reassigns everything, and restarts all consumers ("eager" rebalance).
> **Why naive fails at scale:** Eager (stop-the-world) rebalance revokes ALL partitions from ALL consumers on every join/leave — a "rebalance storm." With 1,000 partitions and rolling deploys, the group spends more time rebalancing than consuming; every deploy causes a full pause. Also, if assignment isn't sticky, warm local state/caches are thrown away.
> **Expected optimal approach:** Group Coordinator (a broker) runs the protocol: consumers `JoinGroup` (coordinator picks a leader consumer), leader runs an *assignor* (Range, RoundRobin, or Sticky/Cooperative-Sticky), consumers `SyncGroup` to receive assignments. Use **Cooperative Incremental Rebalancing (KIP-429)**: only the partitions that must move are revoked, others keep flowing — no stop-the-world. Use **static membership (KIP-345)** with `group.instance.id` so a consumer restart within `session.timeout.ms` does NOT trigger reassignment at all.
> **Pseudo-code or class diagram:**
```
onMembershipChange():
    members = coordinator.currentMembers()          # heartbeat-tracked
    leader  = coordinator.pickLeader(members)
    plan    = leader.assignor.assign(partitions, members)  # sticky
    # cooperative: compute diff vs previous plan
    toRevoke = previousPlan - plan
    for c in members:
        c.revoke(toRevoke ∩ c.owned)                # phase 1: give up moved parts
    coordinator.sync()                              # barrier
    for c in members:
        c.assign(plan[c] - c.owned)                 # phase 2: take new parts only

# consumer liveness
every heartbeat.interval.ms: send Heartbeat
if no heartbeat within session.timeout.ms: coordinator marks dead -> rebalance
if poll() gap > max.poll.interval.ms: self-leave (slow consumer) -> rebalance
```
> Key numbers: `session.timeout.ms` default 45s, `heartbeat.interval.ms` 3s, `max.poll.interval.ms` 5min.

Q11. Two consumers in the same group briefly both think they own partition 7 during a rebalance. One commits offset 1000, the other commits 950. Walk through the race and the fix.
> **Scenario:** Consumer A is processing P7, its `poll()` stalls past `max.poll.interval.ms`; coordinator declares A dead and reassigns P7 to B. B starts from last committed offset and processes. Meanwhile A wakes up, finishes its batch, and calls `commitSync()` for offset 1000 — clobbering B's 950 and causing reprocessing/skips. This is the "zombie consumer" fencing problem.
> **Expected fix:** Generation/epoch fencing. Each rebalance bumps a group *generation id*; the coordinator rejects commits carrying a stale generation with `IllegalGeneration` / `RebalanceInProgress`, so zombie A's commit is refused. On the consumer side, register `ConsumerRebalanceListener.onPartitionsRevoked` to commit and stop processing *before* releasing partitions. For end-to-end exactly-once, use transactional producers with `sendOffsetsToTransaction()` so offset commit and output are atomic and fenced by the producer epoch.
> **Follow-up:** What if the lock holder (partition leader broker) dies mid-write? The controller detects it via the failed broker's ZK/KRaft session, removes it from ISR, elects a new leader from the remaining ISR (highest log-end-offset), and followers truncate to the new leader's HWM using leader-epoch info to avoid divergence. Producers refresh metadata and retry against the new leader.

Q12. A downstream service is down and one consumer keeps failing on the same message ("poison pill"). Meanwhile a producer's network blips cause retries. How do you handle both without data loss or an infinite stall?
> **Scenario:** (a) A malformed/poison record throws on every process attempt, blocking the entire partition because Kafka requires in-order commit. (b) Producer retries after a timeout, risking duplicates.
> **Expected handling:** For the poison pill: bounded retries with backoff, then route the record to a **Dead Letter Topic** (with original topic/partition/offset/exception in headers) and commit past it so the partition keeps flowing; alert on DLQ depth. Never `seek` back forever. For producer retries: enable idempotence (`enable.idempotence=true`) so broker-side (producerId, sequence) dedup makes retries safe, and keep `max.in.flight.requests<=5` so ordering holds under retry. For end-to-end dedup across consumers, make processing idempotent using the (topic, partition, offset) or a business idempotency key stored in the sink. Wrap the downstream call in a circuit breaker (Lesson 10) so you fail fast to DLQ instead of hammering a dead service.

---

### Scaling to 10x / 100x (Hard)
Q13. You're at 500K msg/s and need 5M msg/s. Where does the system break first, and why?
> **Expected answer:** First bottleneck is usually **partition count / per-partition throughput and the resulting replication network**, not raw disk. A single partition tops out around 10–30 MB/s of *sustained* throughput because it's a single ordered log with one leader; you scale by adding partitions, but each partition adds replication fan-out and controller metadata. At 5M msg/s × 1 KB = 5 GB/s ingest, with RF=3 that's ~10 GB/s of extra inter-broker replication traffic — you saturate NICs before disks. Secondary bottleneck: too many partitions (100K+) strains the controller's metadata and increases leader-election time and end-to-end latency, and file-handle/open-segment counts explode on brokers.
> **Numbers to ground the answer:** Rule of thumb: ≤ ~4,000 partitions per broker and ≤ ~200K per cluster (pre-KRaft); target 10–25 MB/s per partition, so 5 GB/s needs ~250–500 partitions minimum, spread so no broker exceeds NIC (25 GbE ≈ 3 GB/s) after accounting for replication + consumer egress. Plan 30–60 brokers.

Q14. You learned partitioning in Lesson 19 — apply it. How do you shard a topic, and what do you do about a hot partition (one key = 40% of traffic)?
> **Expected sharding strategy:** Key-based hashing (`hash(key)%N`) to preserve per-key ordering with even spread for well-distributed keys. Partition count = max(throughput_target / per-partition-capacity, consumer_parallelism_target); over-provision partitions up front because increasing them later reshuffles `hash(key)%N` and breaks ordering for in-flight keys.
> **Hot spot problem:** Detect via per-partition bytes-in/records-in metrics — one partition far above the rest. Fixes: (1) **Key salting/compound key** — append a bucket suffix `key#[0..k]` to spread the hot key across k partitions, at the cost of losing strict single-key ordering (acceptable if you only need per-sub-stream order). (2) If ordering for that key is mandatory, you cannot shard it — instead scale *vertically* for that partition (dedicated fast broker) or redesign the key. (3) For producer-side skew from null keys, use the sticky partitioner to batch better. Note: because partitioning is fixed modulo, you can't just "add a vnode" like Dynamo — this is a real limitation worth calling out.

Q15. Design the read/caching path. Where are the cache layers, and what's the trickiest invalidation issue?
> **Expected layered cache design:** Kafka's "cache" is the **OS page cache** — there is deliberately no app-level cache. Recent writes stay in page cache; consumers reading the tail (the common case) are served entirely from RAM via `sendfile()` zero-copy, never touching disk. For fan-out to many consumer groups, the same page-cache pages serve all of them. Producer-side batching + compression is effectively a write cache. Cross-datacenter, MirrorMaker2 / cluster-linking replicates topics to a local cluster acting as a regional cache.
> **Cache invalidation trap:** The trap is **lagging consumers doing random historical reads**: a consumer replaying from 3 days ago reads cold segments, evicting hot tail pages from the page cache and destroying the zero-copy fast path for everyone (page-cache thrash). Mitigate by isolating replay/batch consumers to separate brokers or read-replicas, using `fetch.max.bytes` limits, and relying on tiered storage so cold reads hit object storage instead of evicting local page cache.

Q16. How do you keep costs sane at petabyte scale?
> **Expected answer:** (1) **Compression** end-to-end (zstd) — 3–5× on-wire and on-disk savings; compress at producer so it propagates through replication and storage. (2) **Tiered storage (KIP-405)** — keep only recent segments on local SSD, offload older segments to S3/object storage transparently; cuts broker disk cost by 5–10× and decouples storage from compute so you can retain months cheaply. (3) **Retention & compaction** — time/size retention for event streams, log compaction for changelog/state topics (keep only latest value per key). (4) Right-size RF (3 is standard; 2 only for cheap/replayable data) and partition count (over-partitioning wastes controller and file-handle resources). (5) Batching (`linger.ms`) amortizes per-request overhead and network round-trips.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)
**H1.** Explain exactly-once semantics end-to-end. Distinguish the idempotent producer (producerId + sequence number, broker-side dedup within a session) from transactions (transactional.id, producer epoch fencing, the transaction coordinator, `__transaction_state` topic, two-phase commit across multiple partitions, and control records / LSO — last stable offset — that hide uncommitted records from `read_committed` consumers). Where does EOS still leak? (External side effects that aren't in the transaction.)

**H2.** Multi-tenancy and security: how do you isolate tenants on a shared cluster? Cover per-topic ACLs (SASL/SCRAM or mTLS identity), quotas (byte-rate and request-rate per principal to prevent noisy neighbors), encryption in transit (TLS) and at rest, and why topic-level isolation is weak for hard multi-tenancy (shared page cache, shared brokers) versus dedicated clusters or KRaft-based isolation.

**H3.** Zero-downtime cluster upgrade and broker replacement: rolling restart honoring `min.insync.replicas` (never take down two ISR members of the same partition simultaneously), controlled shutdown to migrate leadership before stopping, `unclean.leader.election=false` throughout, and partition reassignment throttling (`--throttle`) so rebalancing data movement doesn't saturate the network and starve production traffic.

**H4.** What do you instrument? UnderReplicatedPartitions (should be 0), OfflinePartitionsCount, ActiveControllerCount (exactly 1), consumer group **lag** per partition (the single most important SLO for consumers), request-handler idle ratio, ISR shrink/expand rate, and end-to-end latency via message timestamps. Alert thresholds and why lag (not throughput) is your leading indicator of consumer health.

**H5.** "Undo a bad decision": you launched a topic with 12 partitions; a hot key and growth mean you need 48. You can't just bump partition count without breaking `hash(key)%N` ordering and reshuffling keys. Tell the migration story: create a new topic with 48 partitions and the corrected key, dual-write or use a stream-processing job (Kafka Streams/Flink) to migrate and re-key with offset checkpointing, cut consumers over once caught up, and drain/retire the old topic — all without downtime or reordering in-flight per-key streams.

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. Conflating "acked" with "durable/fsynced." `acks=all` means replicated to ISR in page cache — Kafka relies on replication, not fsync, for durability. Explain the HWM.
2. Assuming you can freely increase partition count. Partitioning is fixed modulo, so adding partitions reshuffles keys and breaks per-key ordering — a one-way door you must plan for.
3. Ignoring rebalance cost. Treating consumer scaling as free; not mentioning cooperative/static membership leads to rebalance storms that dominate every deploy.

**The one insight that makes an answer truly impressive:**
The system stores its own control state using its own primitives: consumer offsets live in a compacted `__consumer_offsets` topic and transaction state in `__transaction_state`. Kafka is "turtles all the way down" — a log that manages itself with logs — and the page cache + zero-copy `sendfile` (no app-level cache, no data copies between kernel and user space) is *why* it's fast, not clever code.

**Suggested follow-up reading:**
- "Kafka: a Distributed Messaging System for Log Processing" (Kreps, Narkhede, Rao, LinkedIn) and Jay Kreps' "The Log: What every software engineer should know about real-time data's unifying abstraction."
- KIP-405 (Tiered Storage), KIP-429 (Cooperative Rebalancing), and KIP-500 (KRaft: replacing ZooKeeper) design docs.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
