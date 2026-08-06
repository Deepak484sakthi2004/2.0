# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 15 of 63 — Phase 1: Foundations (Module 15 of 28)
**Module:** Cluster & Big Data Architectures
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
There's a hard physical wall: one machine can only hold so much disk, so much RAM, so many cores. When your data is petabytes — every web page Google crawls, every event Facebook logs — no single computer works. The entire modern data industry rests on one idea born at Google: take thousands of *cheap, unreliable* commodity machines and make them behave as one *reliable* giant. That idea produced GFS, then Hadoop, then Spark, and ultimately the data platforms behind every recommendation engine and analytics dashboard you'll design in Phase 2. Understand this lineage and you understand why "big data" is an architecture problem, not a database problem.

## Learning Objectives
By the end of this lesson you can:
- Explain what a cluster is and why commodity-scale designs assume failure as normal.
- Describe GFS's master + chunkservers design and its 64 MB chunk / 3x replication numbers.
- Map GFS/HDFS onto the Hadoop architecture (NameNode, DataNodes, YARN).
- Trace a MapReduce job (the classic word-count) end to end with real data flow.
- Explain why Spark's in-memory RDD model is 10–100x faster than MapReduce and when Hadoop still wins.

## The Lesson

### Basics of Cluster Architecture
**What it is (plain English):** A cluster is many independent computers (nodes) networked together and coordinated to act as a single system. Instead of one giant expensive mainframe (scale *up*), you use hundreds of cheap machines (scale *out*). Like replacing one super-strong worker with a large, coordinated crew.

**The problem it solves:** Single machines cap out — the biggest server maxes around dozens of TB of RAM and a few hundred TB of disk, and doubling its power costs exponentially more. Clusters scale *linearly and cheaply*: need 2x capacity, add 2x commodity nodes. Critically, they turn unreliable parts into a reliable whole.

**How it works (mechanics):** Most clusters use a **coordinator + workers** pattern (also master/worker):
```
         ┌──────────────┐
         │ Coordinator  │  assigns work, tracks health via heartbeats
         └──────┬───────┘
        ┌───────┼───────┬────────┐
     [Worker] [Worker] [Worker] [Worker]   ← do the actual work + store data
```
The key design assumption: **failure is normal, not exceptional**. With 1,000 commodity machines each having a ~3-year lifespan, you'll see a machine die roughly *every day*. So the software must detect dead nodes (missed heartbeats, e.g. after 30 s) and re-replicate or reschedule automatically.

**Trade-offs / when NOT to use it:** Distributed systems add coordination overhead, network as a bottleneck, partial-failure complexity, and the CAP trade-offs. For data that fits comfortably on one machine (say < 1 TB, < 64 GB RAM), a single beefy server is simpler and often faster — don't build a cluster to count a spreadsheet.

**Where you'll see it:** Every big-tech backend — Google, Meta, Netflix — plus Kafka, Cassandra, Elasticsearch, and the Hadoop/Spark stacks we cover next.

### GFS — The Google File System
**What it is (plain English):** GFS is a distributed file system Google built (2003 paper) to store enormous files across thousands of commodity machines, tolerating constant disk failures. It's the ancestor of HDFS. Think of it as a filing cabinet spread across a whole warehouse, with three photocopies of every document in different rooms.

**The problem it solves:** Google needed to store the entire crawled web — files far bigger than any single disk — on hardware cheap enough to buy thousands of, which meant hardware that *fails constantly*. Existing filesystems assumed reliable disks and small files. GFS assumed the opposite.

**How it works (mechanics):** One logical **Master** + many **Chunkservers**:
```
        metadata only (which chunk is where)
Client ───▶ [ GFS Master ]  keeps filename → chunk map in RAM
   │              
   │  reads/writes actual data directly to:
   ▼
[Chunkserver A] [Chunkserver B] [Chunkserver C] ...
   each stores 64 MB CHUNKS, every chunk replicated 3x
```
Files are split into fixed **64 MB chunks** (huge, to minimize metadata and favor sequential throughput). Each chunk is **replicated 3 times** on different machines/racks. The Master holds only *metadata* (never the data itself), so it isn't a data bottleneck; clients talk to chunkservers directly for bulk transfer. Chunkservers send heartbeats; if one dies, the Master re-replicates its chunks elsewhere to restore 3x. Worked number: a 1 GB file = 16 chunks × 3 replicas = 48 chunk copies spread across the cluster.

**Trade-offs / when NOT to use it:** Optimized for *large files* and *append/sequential* reads, not tiny files or random small writes — millions of small files bloat the Master's in-RAM metadata (a classic HDFS "small files problem"). The single logical Master is a scaling ceiling and a SPOF (mitigated by shadow masters).

**Where you'll see it:** Google's internal storage (later Colossus), and directly reincarnated as **HDFS** in Hadoop.

### Hadoop Cluster Architecture
**What it is (plain English):** Hadoop is the open-source implementation of Google's ideas: **HDFS** (a GFS clone for storage) + **YARN** (a resource/job scheduler) + a compute engine (MapReduce). It let ordinary companies run Google-style big data on commodity hardware.

**The problem it solves:** Google's papers were public but the code wasn't. Hadoop gave everyone else a batteries-included cluster: reliable petabyte storage plus a way to run distributed computation over it, without building it from scratch.

**How it works (mechanics):** Two planes — storage and compute:
```
STORAGE (HDFS):
  [ NameNode ]  ← metadata (file → block map), like GFS Master
      │
  [DataNode][DataNode][DataNode]  ← store 128 MB blocks, 3x replication

COMPUTE (YARN):
  [ ResourceManager ]  ← cluster-wide scheduler
      │
  [NodeManager] runs [Containers] executing your job's tasks
```
HDFS uses **128 MB blocks** (bigger than GFS's 64 MB, tuned for throughput), 3x replication by default, rack-aware placement (2 replicas one rack, 1 another) so a whole-rack failure still leaves a copy. **YARN** decouples "who has free CPU/RAM" from the job logic, so MapReduce, Spark, and others can all share the cluster. Worked number: a 10-node cluster with 12 disks × 4 TB each = 480 TB raw ÷ 3 replication ≈ 160 TB usable.

**Trade-offs / when NOT to use it:** MapReduce-on-Hadoop is *batch*, high-latency (minutes to hours) — useless for interactive queries. The NameNode is a SPOF/memory bottleneck (small-files problem again). Cloud object stores (S3) + Spark have largely displaced self-managed HDFS for new builds.

**Where you'll see it:** Yahoo (Hadoop's birthplace), early Facebook/LinkedIn data warehouses, and the substrate under Hive, HBase, and Spark-on-YARN.

### MapReduce
**What it is (plain English):** A programming model for processing huge datasets in parallel by expressing the job as two functions: **Map** (transform each record into key-value pairs) and **Reduce** (aggregate all values for each key). The framework handles the hard distributed parts — splitting, scheduling, shuffling, fault tolerance — so you write only two functions.

**The problem it solves:** Parallelizing computation across 1,000 machines by hand is brutal: partitioning, failures, retries, network shuffles. MapReduce hides all of it behind two functions and gives automatic fault tolerance (a failed task just re-runs).

**How it works (mechanics):** The canonical word-count over "the cat sat / the dog sat":
```
INPUT split across nodes
  MAP:      "the"→1 "cat"→1 "sat"→1 | "the"→1 "dog"→1 "sat"→1
  SHUFFLE:  group by key across the network
            the→[1,1]  cat→[1]  sat→[1,1]  dog→[1]
  REDUCE:   sum each     the=2   cat=1     sat=2     dog=1
```
Steps: (1) input split into blocks, a Map task per block runs *where the data lives* (data locality — move compute to data, not data to compute); (2) Map emits key-value pairs; (3) the **shuffle** sorts and sends all values for a key to one Reducer (the expensive, network-heavy step); (4) Reduce aggregates. If a Map task's node dies, the framework re-runs just that task elsewhere. Worked scale: counting words in 1 TB of logs across 1,000 mappers ≈ 1 GB each in parallel.

**Trade-offs / when NOT to use it:** Every stage writes intermediate results to *disk* (HDFS), so multi-step or iterative jobs (like ML training that loops 100 times) re-read/write disk each pass — cripplingly slow. High latency, batch-only. Not for interactive or iterative work — that's exactly the gap Spark fills.

**Where you'll see it:** Google's original indexing pipeline, early Hadoop ETL, log aggregation. Increasingly legacy, but the *model* (map/shuffle/reduce) lives on inside Spark.

### Apache Spark
**What it is (plain English):** A distributed compute engine that keeps intermediate data **in memory** instead of writing to disk between steps, making it 10–100x faster than MapReduce for many jobs. Its core abstraction is the **RDD** (Resilient Distributed Dataset) — a partitioned, fault-tolerant collection you transform with operations like `map`, `filter`, `join`.

**The problem it solves:** MapReduce's disk-per-stage penalty kills iterative and interactive workloads. Spark caches data in RAM across stages, so a 100-iteration ML algorithm reads the data from memory each pass instead of re-reading disk 100 times.

**How it works (mechanics):** Spark builds a **DAG** (directed acyclic graph) of transformations and executes it lazily:
```
RDD(logs) ─filter─▶ RDD ─map─▶ RDD ─reduceByKey─▶ result
     └── DAG scheduler splits into stages, pipelines in memory ──┘
Driver ──▶ Executors (hold RDD partitions in RAM across a cluster)
```
Fault tolerance without replication: each RDD remembers its **lineage** (the recipe of transformations that built it), so a lost partition is *recomputed* from its parent, not restored from a replica. Transformations are *lazy* (nothing runs until an *action* like `count()` triggers the DAG), letting Spark optimize the whole plan. Worked number: a job re-reading a 200 GB dataset 20 times runs the read once into RAM instead of 20 disk scans — roughly the origin of the "up to 100x faster" claim.

**Trade-offs / when NOT to use it:** In-memory means RAM-hungry — if your working set doesn't fit and spills to disk, the speed advantage shrinks toward MapReduce. For a single-pass batch job over data far larger than cluster RAM, plain MapReduce can be comparably efficient and cheaper. Spark clusters cost more (RAM) to run.

**Where you'll see it:** Databricks (founded by Spark's creators), Netflix and Uber analytics, Spark SQL, Spark Streaming, MLlib; the default modern batch/stream engine.

### Hadoop (MapReduce) vs Spark
**What it is (plain English):** The two dominant big-data compute engines. Hadoop MapReduce is the older, disk-based batch workhorse; Spark is the newer in-memory engine that handles batch, streaming, SQL, and ML in one framework.

**The problem it solves:** Choosing between them is a real design decision: cost vs speed, batch vs iterative, maturity vs versatility.

**How it works (mechanics):** The core difference is *where intermediate data lives*:
```
MapReduce: Map ─disk─▶ Shuffle ─disk─▶ Reduce ─disk─▶ ...  (durable, slow)
Spark:     transform ─RAM─▶ transform ─RAM─▶ action        (fast, RAM-bound)
```
Worked comparison: a 100-iteration logistic regression on 100 GB — MapReduce writes/reads ~100 GB to disk each iteration (≈ hours); Spark caches it in RAM and iterates (≈ minutes), the textbook ~100x case. But a single pass filter over 500 TB that won't fit in RAM: both stream from disk and finish in comparable time, with MapReduce using less memory.

**Trade-offs / when NOT to use it:** Spark wins on speed, developer productivity, and unified batch+stream+ML — the default for new work. Hadoop MapReduce wins on memory frugality for one-pass jobs on data far exceeding RAM, and on rock-solid maturity for massive nightly ETL where latency doesn't matter. Note: Spark often *runs on top of* HDFS/YARN — it replaces MapReduce the engine, not Hadoop the storage.

**Where you'll see it:** New analytics/ML → Spark (Databricks). Legacy petabyte nightly ETL → still MapReduce in places. Both frequently coexist on the same Hadoop cluster.

## Comparison Table

| Dimension | Hadoop MapReduce | Apache Spark |
|---|---|---|
| Intermediate data | Disk (HDFS) each stage | In-memory (RDD) |
| Speed (iterative) | Slow (disk per pass) | 10–100x faster |
| Latency | Minutes–hours (batch) | Sub-second–minutes |
| Fault tolerance | Re-run tasks (disk data) | Recompute via lineage |
| Memory need | Low | High (RAM-bound) |
| Workloads | Batch ETL | Batch + stream + SQL + ML |

**Verdict:** Default to Spark for speed and versatility; keep MapReduce for memory-frugal single-pass jobs over data far larger than cluster RAM.

## Common Misconceptions
- **Myth:** Spark replaced Hadoop entirely. → **Reality:** Spark replaced MapReduce (the compute engine); it often still runs on HDFS/YARN (the storage/scheduler).
- **Myth:** Big data needs expensive high-end servers. → **Reality:** The whole point is *commodity* machines made reliable by software and replication.
- **Myth:** The GFS/HDFS master stores the file data. → **Reality:** It stores only metadata (which block is where); data lives on chunkservers/DataNodes.
- **Myth:** 3x replication is wasteful overkill. → **Reality:** With daily machine failures at scale, it's the cheapest way to guarantee durability; erasure coding trades CPU for less space later.
- **Myth:** MapReduce is a database. → **Reality:** It's a batch *programming model* for parallel processing, not a queryable store.

## Real-World Case
Facebook's data warehouse grew from tens of terabytes to over 300 petabytes in the 2010s on Hadoop/HDFS. Engineers hit a wall: analysts needed SQL, but writing raw MapReduce Java for every query was untenable, so Facebook built **Hive** — SQL that compiles down to MapReduce jobs — democratizing the cluster for non-programmers. But Hive-on-MapReduce queries took *minutes*, killing interactivity. That pain drove the industry toward in-memory engines (Presto, Spark) and columnar formats. The through-line: the storage layer (HDFS) scaled beautifully, but the *compute* layer's disk-bound latency became the bottleneck — exactly the gap Spark was built to close. The lesson: at scale, storage and compute evolve on separate clocks, and you optimize the one that's currently hurting.

## Self-Test (answers at the bottom)
1. Why do cluster designs treat machine failure as a normal, expected event rather than an exception?
2. What does the GFS Master store, and what does it deliberately *not* store? Why does that matter for scaling?
3. Trace word-count MapReduce on the input "sun sun rain": what does Map emit, and what does Reduce output?
4. Give the core architectural reason Spark can be ~100x faster than MapReduce on an iterative ML job.
5. Design sketch: You must process 500 TB of clickstream logs nightly (single pass, simple aggregation) and also train a recommendation model that iterates 50 times over a 200 GB feature set. Which engine(s) for each job and why?

## Interview Soundbites
- "The big-data insight is turning thousands of *unreliable* commodity machines into one *reliable* system via replication and automatic failure detection — failure is the normal case, not the exception."
- "The GFS/HDFS master holds only metadata; keeping data off the master is what lets it scale, and it's why millions of tiny files break it."
- "Spark beats MapReduce by keeping intermediate data in RAM across stages and recovering lost partitions via lineage instead of disk replication — that's the 10–100x on iterative jobs."

## Mini-Assignment
On paper (~30 min): (1) Design an HDFS cluster to usably store 300 TB of data at 3x replication — how much raw disk do you provision, and across how many nodes if each holds 48 TB raw? (2) Write pseudocode Map and Reduce functions for computing the *average* purchase amount per user from a log of `(user_id, amount)` records, and note where the shuffle happens. (3) Explain in three lines which stage would benefit most from moving this job from MapReduce to Spark, and why.

## Recap & Tomorrow
- **Clusters:** scale out with commodity nodes; design for constant failure via heartbeats and replication.
- **GFS:** master (metadata) + chunkservers; 64 MB chunks, 3x replication; ancestor of HDFS.
- **Hadoop:** HDFS (NameNode + DataNodes, 128 MB blocks) + YARN scheduler + compute engines.
- **MapReduce:** map → shuffle → reduce; disk between stages; batch, fault-tolerant, but slow for iteration.
- **Spark:** in-memory RDDs + lineage + lazy DAG; 10–100x faster; RAM-bound.
- **Hadoop vs Spark:** disk-frugal batch vs fast in-memory versatility; Spark usually wins, often *on* HDFS.

Tomorrow, **Lesson 16 — Kubernetes Deep Dive**: from the container basics of Lesson 13 to the control plane, pods, services, and autoscaling that orchestrate these clusters in the cloud.

## Self-Test Answers
1. Because at scale (thousands of commodity machines with ~3-year lifespans) statistically a machine fails roughly every day, so software must assume and automatically handle failure via heartbeats, re-replication, and task re-runs rather than treating each failure as an emergency.
2. It stores only metadata — the mapping of files to chunks and which chunkservers hold each chunk — kept in RAM. It does *not* store the actual file data, which lives on chunkservers. This keeps the master off the data path so it isn't a throughput bottleneck, though its RAM limits how many files/chunks (and thus how many small files) the system handles.
3. Map emits: sun→1, sun→1, rain→1. Shuffle groups: sun→[1,1], rain→[1]. Reduce outputs: sun=2, rain=1.
4. MapReduce writes intermediate results to disk between every stage, so an iterative job re-reads/writes the dataset from disk each pass; Spark caches the dataset in memory (RDDs) and iterates over RAM, eliminating the repeated disk I/O — the dominant cost in iterative workloads.
5. Nightly 500 TB single-pass aggregation → MapReduce (or Spark tuned for disk): it's one pass and likely exceeds cluster RAM, so the disk-frugal batch engine is cost-effective and latency doesn't matter. 50-iteration model training on 200 GB → Spark: it fits in cluster RAM and iterates, so in-memory caching gives a large speedup over re-reading disk 50 times.
