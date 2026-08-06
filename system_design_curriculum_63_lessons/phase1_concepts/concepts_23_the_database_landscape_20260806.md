# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 23 of 63 — Phase 1: Foundations (Module 23 of 28)
**Module:** The Database Landscape
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
"Which database would you use?" is asked in nearly every system-design interview, and the wrong answer sinks the whole design. Pick Postgres for a 100,000-writes-per-second event firehose and you'll drown; pick Cassandra for a workload full of ad-hoc joins and analysts will revolt. The senior engineer doesn't have a favorite database — they have a decision framework grounded in access patterns, the read/write ratio, consistency needs, and the physics of storage (RAM vs SSD vs disk). Today you learn the whole menu — relational, wide-column, document, graph, search, time-series — the two storage-engine families (B-tree vs LSM) that underlie all of them, and how to choose deliberately instead of by hype.

## Learning Objectives
By the end of this lesson you can:
- Articulate the real differences between SQL and NoSQL beyond "tables vs no tables."
- Describe when Cassandra, MongoDB, a graph DB, Elasticsearch, or a time-series DB is the right tool.
- Quote the latency of RAM vs SSD vs HDD reads and reason about their impact on design.
- Explain the B-tree vs LSM-tree trade-off and which workloads favor each.
- Apply a structured framework to pick a database from access patterns and scale numbers.

## The Lesson

### SQL vs NoSQL
**What it is (plain English):** SQL (relational) databases store data in tables with a fixed schema and relationships, queried with SQL and guaranteeing ACID transactions. NoSQL is an umbrella for non-relational stores — key-value, document, wide-column, graph — that relax some guarantees (often strict schema and multi-row ACID) to gain horizontal scale and flexibility.

**The problem it solves:** SQL solves data integrity and complex querying: joins, constraints, transactions ("move ₹500 from A to B, all-or-nothing"). NoSQL solves the scale ceiling and rigidity of a single relational node — when you have 50,000 writes/sec or wildly variable document shapes, a single Postgres primary and rigid schema become the bottleneck.

**How it works (mechanics):** SQL enforces a schema and normalizes data across tables joined at query time, scaling primarily *vertically* (bigger box) plus read replicas. NoSQL typically denormalizes, partitions (**shards**) data across many nodes, and scales *horizontally*.
```
SQL (normalized):     users | orders | order_items  (joined at read)
NoSQL (document):     { user, orders:[ {items:[...]} ] }  (one read)
Scale: SQL = vertical + replicas; NoSQL = horizontal sharding
```
**Trade-offs / when NOT to use it:** SQL's joins/transactions are gold for integrity but hard to shard across nodes; NoSQL scales writes but pushes join/consistency logic into your application and often offers only eventual consistency. Don't default to NoSQL for a modest app that needs transactions — modern Postgres handles tens of thousands of TPS and JSON columns.

**Where you'll see it:** SQL: Postgres, MySQL (banking, orders, anything transactional). NoSQL: DynamoDB, Cassandra, MongoDB, Redis (scale-out, flexible).

### Cassandra (Wide-Column)
**What it is (plain English):** Cassandra is a distributed wide-column store built for enormous write throughput and no single point of failure. Every node is equal (masterless), data is spread by a hash ring, and it's tuned for "write a lot, read by known key."

**The problem it solves:** When you must ingest a firehose — say **1,000,000 writes/second** of time-stamped events across data centers — and never go down, a single-primary SQL DB can't keep up. Cassandra's masterless design means any node accepts writes and there's no failover gap.

**How it works (mechanics):** Data is partitioned by a hash of the partition key onto a **consistent-hashing ring**; each row is replicated to N nodes (RF=3). Writes are tunable-consistency: `QUORUM` = majority of replicas ack. Its storage engine is an **LSM-tree** (append-only, write-optimized). You design tables *around your queries* — one table per query pattern, denormalized.
```
Ring: nodeA[0-85] nodeB[86-170] nodeC[171-255]
write(key="sensor_9") -> hash -> nodeB (+replicas C,A)
QUORUM write ack when 2 of 3 replicas confirm
```
**Trade-offs / when NOT to use it:** No joins, limited ad-hoc queries — if you didn't design a table for a query, you can't run it efficiently. Eventual consistency by default; secondary indexes are weak. Terrible fit for transactional, relational, or exploratory-analytics workloads.

**Where you'll see it:** Netflix (viewing history), Discord (messages, trillions of rows), Apple, Uber — write-heavy, always-on, key-based access.

### MongoDB (Document)
**What it is (plain English):** MongoDB stores flexible JSON-like documents (BSON) grouped in collections, with no fixed schema — each document can have different fields. It's the go-to when your data is naturally hierarchical and your schema evolves fast.

**The problem it solves:** When your entity is a rich nested object (a product with variants, specs, reviews) and the shape changes often, forcing it into rigid relational tables with many joins is painful. A document keeps the whole object together for a single-read fetch, and schema changes need no migration.

**How it works (mechanics):** Documents are indexed with B-tree indexes; sharding partitions collections by a shard key; replica sets (primary + secondaries) provide HA. Reads by `_id` or indexed field are fast; a rich query language and aggregation pipeline support filtering and grouping.
```
{ _id: 42, name:"Shoe", variants:[{size:9,stock:12}], reviews:[...] }
find({_id:42}) -> one document, no joins, ~1 ms
shard key = _id  -> partitions across cluster
```
**Trade-offs / when NOT to use it:** Multi-document ACID transactions exist but are slower and less natural than in SQL; heavy cross-document joins (`$lookup`) are awkward. Denormalized documents can bloat and duplicate data. Poor shard-key choice creates hotspots. Not ideal for highly relational, transaction-heavy domains.

**Where you'll see it:** Content management, product catalogs, user profiles, IoT configs, fast-moving startups. eBay, Adobe, and many CMS-driven sites use it.

### Graph Databases
**What it is (plain English):** A graph database stores data as nodes and relationships (edges) as first-class citizens, so traversing connections ("friends of friends who like hiking") is a direct pointer walk rather than a pile of joins.

**The problem it solves:** Relationship-heavy queries kill relational databases. "Find all people within 3 hops of Alice" in SQL means 3 self-joins on a huge table — cost explodes exponentially with depth. A graph DB follows edges directly, so a 3-hop traversal touches only the connected subgraph.

**How it works (mechanics):** Nodes store direct pointers to adjacent nodes (**index-free adjacency**), so hopping to a neighbor is O(1) regardless of total graph size. Queried with Cypher/Gremlin.
```
(Alice)-[:FRIEND]->(Bob)-[:FRIEND]->(Carol)
MATCH (a)-[:FRIEND*1..3]->(x)  // 3-hop traversal
SQL equivalent: 3 self-joins, cost ~ rows^3
```
For a 3-hop query on a large social graph, a graph DB stays in milliseconds while the SQL join can take seconds to minutes.

**Trade-offs / when NOT to use it:** Poor fit for aggregate/tabular analytics ("total sales by region") and for simple key-value or document access — you pay for graph machinery you don't use. Scaling a graph across many machines is genuinely hard (graphs resist clean partitioning).

**Where you'll see it:** Neo4j, Amazon Neptune, TigerGraph. LinkedIn's degrees of connection, fraud-ring detection, recommendation engines, knowledge graphs.

### Elasticsearch (Search)
**What it is (plain English):** Elasticsearch is a distributed search and analytics engine built on an inverted index — it's what powers fast full-text search ("find every document containing 'refund policy'") and log analytics, not a primary system of record.

**The problem it solves:** `SELECT ... WHERE description LIKE '%refund%'` in SQL scans every row and can't rank by relevance or handle typos/synonyms. Elasticsearch pre-indexes every term so a keyword search is near-instant over billions of documents, with relevance scoring.

**How it works (mechanics):** An **inverted index** maps each term → list of documents containing it (like a book index). At query time it intersects term lists instead of scanning documents. Data is sharded across nodes; each shard is a Lucene index.
```
Inverted index:
  "refund"  -> [doc3, doc17, doc981]
  "policy"  -> [doc17, doc44]
query "refund policy" -> intersect -> doc17 (ranked by TF-IDF/BM25)
Searches billions of docs in ~tens of ms
```
**Trade-offs / when NOT to use it:** Near-real-time, not strongly consistent (a ~1 s refresh interval before new docs are searchable); not ACID; expensive on RAM. Don't use it as your source of truth — pair it with a primary DB and index into it. Weak for transactions and precise up-to-the-millisecond reads.

**Where you'll see it:** Site search (Wikipedia, GitHub code search), the ELK stack for log analytics, observability platforms, e-commerce product search.

### Time-Series Databases
**What it is (plain English):** A time-series database (TSDB) is optimized for data that is a stream of timestamped measurements — metrics, sensor readings, stock ticks — where you append constantly and query by time ranges and aggregations.

**The problem it solves:** Metrics workloads are append-heavy and time-ordered: millions of points/second in, queries like "average CPU per minute over the last 24 hours." General databases waste space and time on this; TSDBs exploit the structure with time-partitioning, heavy compression, and downsampling.

**How it works (mechanics):** Data is partitioned into time chunks; because consecutive timestamps and values are similar, **delta-of-delta** timestamp encoding and compression (e.g., Gorilla) shrink storage ~10x. Old data is **downsampled** (keep per-second for a day, per-minute for a month) and expired via retention policies.
```
raw: 1s resolution for 24h, then roll up to 1m for 30 days, drop after
Compression: Facebook Gorilla ~ 1.37 bytes/point (from 16) -> ~12x
Query "avg cpu / 5min last 6h" -> scan one time-partition, fast
```
**Trade-offs / when NOT to use it:** Bad for random updates/deletes and relational access — it assumes append-mostly, immutable, time-ordered data. High-cardinality tag explosions (millions of unique label combos) can wreck performance.

**Where you'll see it:** Prometheus, InfluxDB, TimescaleDB, Amazon Timestream. Monitoring/observability, IoT telemetry, financial ticks.

### Speed of Reading: Memory vs SSD vs Hard Disk
**What it is (plain English):** Where data lives determines how fast you can read it. RAM is fastest, SSD (flash) is much slower, spinning hard disk (HDD) is slowest by far. Every database is, at heart, a strategy for keeping hot data in the fast tiers.

**The problem it solves:** Understanding this hierarchy is *the* basis of caching, indexing, and engine design. If you don't know an HDD seek is ~10 ms while RAM is ~100 ns, you can't reason about why a cache miss to disk destroys latency.

**How it works (mechanics):** Approximate read latencies (the numbers every senior engineer memorizes):
```
L1 cache:        ~1 ns
Main memory RAM: ~100 ns          (1x baseline)
SSD random read: ~100 us (100,000 ns)  ~1,000x slower than RAM
HDD seek+read:   ~10 ms (10,000,000 ns) ~100,000x slower than RAM
Network (same DC round-trip): ~0.5 ms
```
So one HDD random seek ≈ the time of 100,000 RAM reads. SSDs also have no seek penalty (no moving head), giving huge random-read advantages over HDD.

**Trade-offs / when NOT to use it:** RAM is fast but volatile and expensive (~₹ per GB high); HDD is cheap and huge but slow at random access (fine for sequential/archival). SSDs sit in between and wear out after finite writes. You place data by heat: hot in RAM, warm on SSD, cold on HDD/object storage.

**Where you'll see it:** Every cache (Redis in RAM), tiered storage in databases, "cache miss to disk" latency cliffs, why sequential I/O designs (LSM, logs) beat random I/O.

### B-tree vs LSM-Tree Storage Engines
**What it is (plain English):** These are the two dominant ways a database physically stores and updates data on disk. **B-trees** update data in place in a balanced tree (great for reads); **LSM-trees** (Log-Structured Merge) append writes to memory then flush sorted files to disk, merging them later (great for writes).

**The problem it solves:** The core tension: reads want data sorted and in place (B-tree); writes want to avoid slow random disk writes (LSM turns them into fast sequential appends). Your engine choice biases the DB toward read- or write-heavy workloads.

**How it works (mechanics):**
- **B-tree:** a balanced tree of fixed-size pages; a lookup is O(log n) page reads; a write finds the page and updates it *in place* (random I/O, plus a write-ahead log).
- **LSM:** writes go to an in-memory **memtable**; when full, it's flushed as a sorted **SSTable** file; background **compaction** merges SSTables. A read may check the memtable + several SSTables (Bloom filters skip most).
```
B-tree write: seek page, update in place  (random I/O)
LSM write:    append to memtable -> sequential flush (fast)
LSM read:     memtable + SSTable_1..n (Bloom filter helps)  -> read amp
```
**Trade-offs / when NOT to use it:** B-trees give predictable fast reads but slower random writes and write amplification via page updates. LSM gives blazing write throughput and great compression but suffers **read amplification** (check multiple files) and background compaction that steals I/O. Read-heavy/OLTP → B-tree; write-heavy/ingest → LSM.

**Where you'll see it:** B-tree: Postgres, MySQL/InnoDB, MongoDB (WiredTiger default). LSM: Cassandra, RocksDB, LevelDB, HBase, ScyllaDB, and RocksDB-backed engines everywhere.

### How to Pick the Right Database
**What it is (plain English):** A repeatable decision process: describe the data and its access patterns, then match to the store whose strengths fit — rather than picking by popularity.

**The problem it solves:** Picking by hype ("everyone uses Mongo") leads to painful migrations. A framework forces you to state read/write ratio, query shape, consistency needs, and scale up front.

**How it works (mechanics):** Ask, in order:
```
1. Structured + transactional + joins?        -> SQL (Postgres)
2. Massive write throughput, key access, HA?   -> Cassandra (LSM)
3. Flexible nested docs, evolving schema?      -> MongoDB
4. Relationships/traversals central?           -> Graph (Neo4j)
5. Full-text search / relevance / logs?        -> Elasticsearch
6. Timestamped metrics, append-mostly?         -> TSDB (Prometheus)
7. Simple key-value, ultra-low latency?        -> Redis/DynamoDB
```
Worked example: "Store 500k IoT readings/sec, query by device + time range, keep 90 days." → write-heavy + time-ranged → **TSDB or Cassandra (LSM)**, not Postgres. "Bank ledger, strict consistency, joins" → **Postgres**.

**Trade-offs / when NOT to use it:** Real systems use **several** databases (polyglot persistence) — Postgres for orders, Elasticsearch for search, Redis for cache, a TSDB for metrics — but each store you add is operational burden. Don't multiply stores without a clear access-pattern justification.

**Where you'll see it:** Every serious architecture review. Amazon, Uber, and Netflix all run polyglot persistence, choosing per-service.

## Comparison Table

| Database | Data model | Best workload | Engine | Consistency |
|---|---|---|---|---|
| Postgres/MySQL | Relational tables | Transactions, joins | B-tree | Strong (ACID) |
| Cassandra | Wide-column | Huge writes, key reads, HA | LSM | Tunable/eventual |
| MongoDB | Documents | Flexible nested objects | B-tree | Strong per-doc |
| Neo4j (graph) | Nodes + edges | Deep relationship traversal | Native graph | ACID |
| Elasticsearch | Inverted index | Full-text search, logs | Lucene/inverted | Near-real-time |
| TSDB (Prometheus) | Time series | Metrics, append-mostly | LSM-like | Eventual |

**Verdict:** There is no "best" database — there's the one whose data model and storage engine match your dominant access pattern and scale. Pick from patterns, not popularity.

## Common Misconceptions
- **Myth:** NoSQL is faster than SQL. → **Reality:** It's differently scalable; for its query patterns SQL is often faster, and modern Postgres does tens of thousands of TPS with JSON.
- **Myth:** NoSQL means "no schema." → **Reality:** The schema moves into your application code; you still model it, just implicitly and per-query.
- **Myth:** SSD and RAM are about the same speed. → **Reality:** RAM is ~1,000x faster than SSD random reads and ~100,000x faster than HDD seeks.
- **Myth:** LSM-trees are strictly better than B-trees. → **Reality:** LSM wins writes but suffers read amplification and compaction cost; B-trees win predictable reads.
- **Myth:** Elasticsearch can be your primary database. → **Reality:** It's near-real-time and not a durable system of record; back it with a real primary store.

## Real-World Case
Discord's message store is a textbook database-selection saga. They started on MongoDB, hit scaling pain as messages exploded, and migrated to **Cassandra** to handle the write-heavy, append-mostly, "read by channel + time" pattern at billions of messages — a natural LSM/wide-column fit. Years later, as they crossed **trillions of messages** and Cassandra's JVM garbage-collection pauses and compaction overhead caused latency spikes, they migrated again to **ScyllaDB** (a C++ rewrite of Cassandra's model with no GC pauses), cutting tail latencies dramatically and shrinking their node count. The throughline: the *data model* (wide-column/LSM) stayed right for the access pattern the whole time; what changed was the implementation's operational behavior at extreme scale. Choosing the right model early saved them from a far more painful re-architecture.

## Self-Test (answers at the bottom)
1. Name two concrete things a SQL database gives you that a typical NoSQL store makes you handle in application code.
2. Roughly how much slower is an HDD random seek than a RAM read, and than an SSD read?
3. Why does an LSM-tree achieve higher write throughput than a B-tree, and what does it pay for it on reads?
4. A team wants full-text product search with typo tolerance and relevance ranking over 200M products. Which store, and why not just SQL `LIKE`?
5. Design sketch: You're building telemetry for 2M IoT devices each sending a reading every 10 s (~200k writes/sec), queried as "avg per device per 5 min over last 7 days," retained 90 days. Which database(s) and storage engine, how do you handle retention/compression, and why not Postgres alone?

## Interview Soundbites
- "I don't pick a database by popularity; I pick by access pattern — read/write ratio, query shape, consistency, and scale — then match to the engine, B-tree for reads, LSM for writes."
- "RAM is ~1,000x faster than SSD and ~100,000x faster than HDD seeks; that hierarchy is why caching and sequential-write engines exist."
- "Cassandra is masterless and LSM-backed, so it eats a write firehose and never has a failover gap, but you design tables per query because there are no joins."

## Mini-Assignment
Pick three real products you use (e.g., a chat app, a maps app, a monitoring dashboard). For each, in ~30 minutes: (1) List its dominant access patterns and rough read/write ratio. (2) Choose a primary database and justify it with the framework's questions. (3) Identify one place it would also need a second store (search, cache, or metrics) — polyglot persistence. (4) State whether the primary store is B-tree or LSM-backed and why that fits.

## Recap & Tomorrow
- **SQL vs NoSQL:** integrity/joins/ACID vs horizontal scale and flexible schema — pick by access pattern.
- **Cassandra / MongoDB:** wide-column LSM for write-heavy key access; documents for flexible nested objects.
- **Graph / Elasticsearch / TSDB:** traversals, full-text search, and append-mostly metrics each need a purpose-built store.
- **Storage speed:** RAM ~100 ns, SSD ~100 µs, HDD ~10 ms — the hierarchy behind caching and engine design.
- **B-tree vs LSM:** in-place reads-optimized vs append-only writes-optimized.
- **Picking a DB:** derive it from patterns and scale, and expect polyglot persistence.

Tomorrow, **Lesson 24 — Caches**: Memcached vs Redis, cache-aside vs write-through vs write-back, eviction policies, TTL and invalidation, and thundering-herd protection — the layer that keeps that RAM-vs-disk gap from ever hurting your users.

## Self-Test Answers
1. Multi-row ACID transactions (all-or-nothing money transfers) and declarative joins/foreign-key integrity across tables. In most NoSQL stores you must denormalize and enforce consistency/relationships yourself in application code.
2. An HDD seek (~10 ms) is roughly 100,000x slower than a RAM read (~100 ns) and about 100x slower than an SSD random read (~100 µs).
3. LSM turns writes into fast sequential appends to an in-memory memtable, then batched sequential flushes — avoiding the random in-place page writes a B-tree does. It pays with read amplification: a read may check the memtable plus multiple on-disk SSTables (mitigated by Bloom filters and compaction), and compaction consumes background I/O.
4. Elasticsearch (or a search engine). SQL `LIKE '%term%'` scans every row, can't rank by relevance, and can't do typo/synonym handling; Elasticsearch's inverted index pre-maps terms to documents for near-instant, relevance-scored search over hundreds of millions of docs.
5. Use a time-series DB (Prometheus/InfluxDB/TimescaleDB) or Cassandra — both LSM-based, write-optimized for the ~200k/sec append firehose. Partition by device+time, apply delta/Gorilla-style compression, downsample raw readings to 5-minute rollups, and expire data past 90 days via retention policies. A single Postgres primary is B-tree/in-place and would struggle with the random-write pressure and would waste storage without time-partitioning and columnar compression.
