# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 51 of 63 — Phase 2: System Design Track (Design 23 of 35)
**Topic:** Google Drive / Dropbox — distributed file storage, sync, and sharing
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
Dropbox stores exabytes for ~700M+ registered users; Google Drive is comparable. The deceptively hard part is not storing a file — object stores do that — it's *sync*: keeping the same file consistent across a laptop, a phone, and the cloud while the user edits offline, on flaky networks, with 50GB of existing data, without re-uploading the whole file when they change one byte. Add sharing/permissions and cross-device conflict resolution and you have one of the richest consistency problems in system design. This session is about block-level sync, metadata, and the notification/delta protocol.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Object storage & durability (taught in Phase 1, Lesson 15 — GFS/HDFS lineage):** Chunked, replicated, immutable blob storage with high durability (S3/GCS ≈ 11 nines). Dropbox stores file *blocks* in such a store (historically S3, later their own "Magic Pocket"). Files are split into blocks; blocks are the unit of storage and dedup. GFS's chunk+master model is the direct ancestor of the block-store + metadata-server split you'll design today.

**Content-addressed storage & hashing / Merkle trees (taught in Phase 1, Lesson 5):** Hash a block → its address; identical content → identical hash → automatic dedup. Merkle trees let you compare two file/tree states cheaply and find exactly which blocks changed. This is the core of delta sync and cross-user/global deduplication.

**Consistent hashing (taught in Phase 1, Lesson 19):** Distributes blocks across storage nodes with minimal reshuffle on scaling. Used to place blocks in the block store and to shard the metadata DB by user/namespace.

**Caching (taught in Phase 1, Lesson 24):** Local disk is itself a cache of cloud state; add a metadata cache and hot-block cache. LRU eviction for selective sync / smart sync (placeholder files).

**Long-polling / notifications / pub-sub (taught in Phase 1, Lesson 21):** Clients must learn about remote changes fast without hammering the server. A notification service (long-poll or WebSocket) tells a client "your namespace changed, come fetch the delta." Kafka-style event log drives fan-out to a user's other devices and to sharees.

**CAP / consistency & LWW (taught in Phase 1, Lesson 2):** Offline edits on multiple devices → conflicts. You need a versioning scheme (version vectors / file version chains) and a defined conflict policy (create "conflicted copy" rather than silently lose data).

**SQL/NoSQL & B-tree vs LSM (taught in Phase 1, Lesson 23):** Metadata (file tree, versions, permissions) is a huge, highly-relational, transactional workload → sharded relational (Dropbox's metadata on sharded MySQL) or a strongly-consistent store. Blocks are immutable → object store. The split is fundamental.

> **Recap box:** Files = immutable content-addressed *blocks* in an object store (L15, L5). Merkle/hash diff = delta sync + dedup (L5). Blocks/metadata sharded by consistent hashing (L19). Notification service pushes "namespace changed" (L21). Conflicts → version vectors + conflicted copies, never silent loss (L2). Metadata in sharded relational; blocks in object store (L23).

---

## Part 2 — The Interview Session

### Warm-Up Questions (Easy)

Q1. Why split a file into blocks (e.g., 4 MB chunks) instead of storing it as one object? Give three concrete benefits.
> **What a strong answer covers:** (1) *Delta sync*: change one byte → only the affected block(s) re-upload, not the whole 2 GB file. (2) *Deduplication*: content-addressed blocks (hash = address) mean identical blocks — across versions or across users — are stored once. (3) *Parallel + resumable transfer*: upload/download blocks in parallel and resume after a network drop without restarting. Bonus: bounded memory (stream block-by-block) and easier durability (replicate small blocks). Fixed-size vs content-defined chunking matters — content-defined (rolling hash / Rabin fingerprint) avoids the "insert one byte shifts every subsequent block" problem.
> **Common weak answer:** "So big files upload faster" with no mention of dedup, delta, or the insert-shift problem.
> **Mentor follow-up if they answer well:** Fixed 4 MB blocks vs content-defined chunking — if a user inserts one byte at the start of a file, what happens to delta sync with fixed blocks, and how does a rolling-hash boundary fix it?

Q2. Estimate storage and metadata scale: 500M users, average 50 GB used, average file 1 MB. Estimate raw storage, number of files, and metadata rows.
> **What a strong answer covers:** Raw = 500M × 50 GB = 25 EB logical. After dedup + compression, physical is far less — dedup can cut 20–50%+ for shared/common content. Files: 25 EB / 1 MB ≈ 25e18/1e6 = 2.5e13 = 25 *trillion* files → 25 trillion metadata rows minimum (plus a row per block, per version, per permission). This is why metadata is the hard scaling problem: it's a massive, transactional, relational dataset that must be sharded (Dropbox shards MySQL by user/namespace). Blocks: at 4 MB/block, ~6e18 blocks pre-dedup. The point: *metadata volume rivals or exceeds the engineering difficulty of the blob storage*.
> **Mentor follow-up:** Which grows faster and hurts more — block storage or metadata rows? (Metadata: it's transactional, indexed, relational, and cross-referenced; blobs are append-only and embarrassingly parallel.)

Q3. Where do file *contents* live vs file *metadata* (name, path, permissions, version history)? Why different stores?
> **What a strong answer covers:** Contents → immutable content-addressed blocks in an object/block store (append-only, huge, cheap, high-durability). Metadata → sharded relational DB (MySQL) or strongly-consistent store: the file tree, versions, sharing ACLs, and the mapping file→ordered list of block-hashes. Different because metadata is small, hot, transactional, relational, and needs strong consistency (moves/renames/permission changes must be atomic); blocks are large, immutable, and only need durability + dedup.
> **Red flag answer:** "Store files and their metadata together in one NoSQL document." Loses transactional path operations, dedup, and independent scaling.

---

### High-Level Design (Medium)

Q4. Draw the architecture: client sync engine, metadata service, block service, notification service, sharing.
> **Key components expected:** Client sync engine (watcher + chunker + local DB), API/edge, Metadata service (file tree, versions, block lists) on sharded MySQL, Block service → object/block store, Notification/long-poll service, Sharing/ACL service, Presence of changes via an event log.
> **Architecture diagram (text):**
```
  ┌─────────────── Client (per device) ───────────────┐
  │ FS watcher → Chunker(rolling hash) → Local index DB │
  │ Uploader/Downloader (block-level, resumable)        │
  └───────┬──────────────────────────────┬─────────────┘
          │ metadata (commit/list delta)  │ block get/put (by hash)
          ▼                               ▼
   [API / Edge LB] ──> Metadata Service      Block Service ──> Object/Block Store (S3/Magic Pocket)
          │            (sharded MySQL:                         (content-addressed, replicated)
          │             file tree, versions,
          │             block lists, ACLs)
          │                 │
          │                 ├──> Sharing/ACL Service
          │                 └──> writes change-event ──> [Event Log / Kafka]
          │                                                     │
          ▼                                                     ▼
   [Notification Service] <── long-poll/WebSocket ── other devices & sharees ("namespace N changed")
```
> **What separates SDE2 from SDE3 here:** SDE2 draws client→server→S3. SDE3 (a) separates metadata (strongly consistent, sharded relational) from blocks (immutable object store), (b) makes upload a *two-phase* flow — "which blocks do you already have?" (dedup check) then upload only missing blocks then atomically commit metadata, and (c) adds the notification service so devices learn of changes via push, not polling. SDE3 also names the "commit" as the atomic point that makes a new file version visible.

Q5. Trace an offline edit syncing up, and a remote change syncing down.
> **Expected trace (upload):**
> 1. FS watcher detects a modified file; chunker splits it (content-defined boundaries) and hashes each block → list of block-hashes.
> 2. Client diffs against its last-known block list (Merkle/hash compare) → set of *changed* block-hashes.
> 3. Client asks metadata service `has_blocks?([hashes])` → server returns which already exist (dedup). Client uploads only missing blocks to the block service (parallel, resumable).
> 4. Client commits: `PUT file version = {path, ordered block-hash list, parent_version}` — metadata service validates parent version (optimistic concurrency), writes the new version atomically, emits a change event.
> **Expected trace (download):**
> 5. Event log fans the change out; notification service tells the user's *other* devices "namespace N changed since cursor C."
> 6. Those devices call `list_delta(cursor)` → get changed file entries + new block-hash lists → fetch only blocks they lack → reconstruct file locally → advance cursor.
> **Tricky part:** Candidates skip the dedup pre-check and the parent-version optimistic-concurrency check on commit, and they make download a full re-list instead of a *cursor-based delta*. The cursor (a monotonic per-namespace version) is what makes sync incremental.

Q6. Design the sync/metadata API.
> **Expected API design:**
> - `POST /blocks/check` `{hashes:[...]}` → `{missing:[...]}` (dedup gate before upload).
> - `PUT /blocks/{hash}` (idempotent — content-addressed; re-put of same hash is a no-op).
> - `POST /commit` `{path, block_hashes:[...], parent_version}` → `{new_version}` or `409 Conflict` if parent stale.
> - `GET /delta?namespace=N&cursor=C` → `{entries:[...], new_cursor, has_more}` (paginated delta list).
> - `GET /longpoll?namespace=N&cursor=C` → blocks until change or timeout, returns "changed" so client calls `/delta`.
> - `POST /shares` `{path, principal, role}` → grant/revoke ACL.
> **What to push on:** Idempotency (block PUT is naturally idempotent by hash; commit must be idempotent by a client-supplied request id so a retried commit doesn't create two versions). Pagination of `/delta` (a namespace with 1M files can't return everything at once — cursor + `has_more`). Versioning the API and the *file version chain* (parent_version enables optimistic concurrency and conflict detection). Long-poll timeout tuning (30–60s) to balance connection count vs freshness.

---

### Data Modeling (Medium–Hard)
Q7. Model the metadata: files, versions, blocks, and namespaces.
> **Expected schema:**
```sql
-- Namespace = a sync root (a user's root, or a shared folder). Sharding boundary.
Namespace ( ns_id BIGINT PRIMARY KEY, owner_id BIGINT, latest_cursor BIGINT );

-- File node in the tree (current state). Sharded by ns_id.
FileNode (
  ns_id       BIGINT,
  file_id     BIGINT,
  parent_id   BIGINT,           -- tree structure
  name        VARCHAR(255),
  is_dir      BOOLEAN,
  cur_version BIGINT,
  deleted     BOOLEAN,
  PRIMARY KEY (ns_id, file_id),
  UNIQUE KEY  (ns_id, parent_id, name)   -- no two siblings with same name
);

-- Immutable version: an ordered list of block hashes. Append-only history.
FileVersion (
  ns_id      BIGINT,
  file_id    BIGINT,
  version    BIGINT,            -- monotonic per file
  size       BIGINT,
  created_at TIMESTAMP,
  PRIMARY KEY (ns_id, file_id, version)
);
FileVersionBlock (
  ns_id BIGINT, file_id BIGINT, version BIGINT,
  seq   INT, block_hash CHAR(64),
  PRIMARY KEY (ns_id, file_id, version, seq)
);

-- Global block index (content-addressed, dedup). Sharded by hash.
BlockIndex ( block_hash CHAR(64) PRIMARY KEY, size INT, refcount BIGINT, store_loc TEXT );

-- Change log per namespace drives delta sync.
NamespaceChange (
  ns_id BIGINT, cursor BIGINT, file_id BIGINT, change_type TEXT,
  PRIMARY KEY (ns_id, cursor)
);
```
> **Index choices and why:** `UNIQUE(ns_id,parent_id,name)` enforces filesystem semantics and speeds path lookups. `NamespaceChange` clustered by `(ns_id,cursor)` gives O(log n) "changes since cursor C" for delta sync. `BlockIndex` PK on hash = point-lookup dedup + refcount for GC.
> **Partitioning key and why:** Shard *metadata* by `ns_id` — a namespace (user root or shared folder) is the unit of sync and permission, so colocating it keeps commits/delta reads single-shard and transactional. Shard *blocks* by `block_hash` (consistent hashing) — independent of user, enables global dedup and even distribution. Never shard metadata by `file_id` alone (cross-shard tree operations).

Q8. `list_delta(namespace, cursor)` — return everything that changed since the client last synced, efficiently, for a namespace with millions of files.
> **Expected answer:** Maintain a monotonic per-namespace `cursor` (logical clock) incremented on every commit; each change writes a `NamespaceChange(ns_id, cursor, file_id, type)` row. Delta = `SELECT * FROM NamespaceChange WHERE ns_id=? AND cursor > C ORDER BY cursor LIMIT K`, then join to current FileNode/FileVersion for the changed files. Return `new_cursor` + `has_more` for pagination. The client only fetches block-hash lists for changed files, then only the blocks it lacks. This is O(changes), not O(files).
> **Trap:** Recomputing the delta by scanning the entire file tree and comparing timestamps — O(files) per sync, catastrophic for a 1M-file namespace synced every few minutes. Also a trap: using wall-clock timestamps as the cursor (clock skew, ties) instead of a monotonic logical counter.

Q9. Two devices edit the same file offline, then both come online. What's the consistency model and conflict resolution? Apply CAP (Lesson 2).
> **Expected answer:** Metadata commits are strongly consistent *per namespace* (single-shard transaction) — so commits are serialized, but *offline* edits produce concurrent versions with the same `parent_version`. Detection: optimistic concurrency — device B's commit with `parent_version = v5` fails with 409 because device A already advanced the file to v6. Resolution policy: NEVER silently overwrite. Create a "conflicted copy" (`report (Device B's conflicted copy).ext`) preserving both versions, and let the user reconcile. Track causality with version vectors (per-device counters) so you can tell true conflicts from stale-but-ancestor updates. Downloads are eventually consistent (delta propagation has lag); the file tree converges once all deltas apply.
> **Mentor pushback:** A *rename* on device A and an *edit* on device B of the same file — is that a conflict? Not necessarily: rename touches the FileNode (name), edit creates a FileVersion (content); they can merge (renamed file with new content) if you model metadata and content changes independently rather than as one monolithic "file changed" event. But a rename-vs-rename or move-into-deleted-folder *is* a structural conflict requiring a policy (e.g., resurrect folder, or conflicted copy). Good candidates separate *content* conflicts from *tree/structural* conflicts.

---

### Low-Level Design (Hard)
Q10. Design delta sync so editing one byte in a 2 GB file uploads only kilobytes.
> **Problem statement:** A user changes a few bytes in the middle of a large file. Re-uploading 2 GB is unacceptable; you must transfer only what changed and dedup against what's already stored.
> **Naive solution:** Fixed-size 4 MB blocks: hash each block, upload blocks whose hash changed.
> **Why naive fails at scale:** Fixed blocks break on *insertion*: insert one byte near the start and every subsequent block boundary shifts, so every block's hash changes → you re-upload the whole file despite a 1-byte edit. Fixed blocks only work for in-place overwrites, not insert/delete.
> **Expected optimal approach:** *Content-defined chunking* with a rolling hash (Rabin–Karp / Rabin fingerprint): slide a window over the bytes, declare a chunk boundary when the rolling hash matches a pattern (e.g., low bits == 0), giving ~variable-size chunks (target avg 4 MB, min/max bounded). Boundaries are content-anchored, so inserting bytes only changes the *one* chunk containing the insertion (plus re-anchoring nearby) — the rest of the chunks keep identical hashes and are already stored. Then Merkle-diff the chunk-hash lists to find the minimal changed set, dedup via `blocks/check`, upload only missing chunks, commit new version. This is exactly the rsync/CDC family.
> **Pseudo-code or class diagram:**
```
def content_defined_chunks(bytes):
    chunks=[]; start=0; h=RollingHash()
    for i,b in enumerate(bytes):
        h.roll_in(b)
        if i-start >= MIN and (h.value & MASK)==0 or i-start >= MAX:  # boundary
            chunks.append(sha256(bytes[start:i+1])); start=i+1
    if start < len(bytes): chunks.append(sha256(bytes[start:]))
    return chunks   # list of content-anchored block hashes

def delta_upload(local_bytes, last_known_hashes):
    new = content_defined_chunks(local_bytes)
    changed = set(new) - set(last_known_hashes)           # Merkle/set diff
    missing = metadata.check_blocks(list(changed))         # dedup gate
    for h in missing: block_store.put(h, chunk_bytes[h])   # only missing blocks
    metadata.commit(path, block_hashes=new, parent_version=last_version)
```

Q11. Concurrency/race: two of a user's devices commit new versions of the same file within milliseconds. Prevent lost updates and duplicate versions.
> **Scenario:** Device A and B both read parent_version=v5 and both POST /commit.
> **Expected fix:** Optimistic concurrency control on `parent_version` at the metadata shard: the commit is a single-shard transaction `IF cur_version = parent_version THEN cur_version := new; INSERT FileVersion`. First writer wins → cur_version=v6. Second writer's `parent_version=v5` no longer matches → 409 Conflict → client fetches v6, and since it has genuinely different content, creates a *conflicted copy* (Q9). Duplicate-commit protection: each commit carries a client `request_id`; the metadata service dedupes on `(ns_id, file_id, request_id)` so a network retry of the *same* commit returns the existing version instead of creating a second one (idempotency).
> **Follow-up:** What if the client dies after uploading blocks but before committing? No harm — blocks are content-addressed and orphaned; a background GC decrements refcounts and reclaims blocks with refcount 0 after a grace period. The metadata commit is the single atomic point of visibility, so a half-finished upload never corrupts a file version.

Q12. Edge case: a shared folder with 100 collaborators; one uploads a 500 MB file. How do the other 99 devices find out and sync without a thundering herd?
> **Scenario:** One commit to a shared namespace must notify 99 devices, each of which then pulls the delta and blocks.
> **Expected handling:** The commit emits ONE change event to the namespace change log; the notification service fans it out to the 99 devices' long-poll/WebSocket connections ("namespace changed to cursor C+1"). Devices then call `/delta` (cheap, returns just the new file entry + block list), then fetch the 500 MB of blocks *from the block store / CDN*, not from the metadata service — so metadata sees 99 cheap delta reads, and block downloads are served by the scalable object store/CDN with edge caching (the same blocks are cache-hot after the first fetch). Stagger with jitter to avoid a synchronized stampede. Circuit breaker (Lesson 10) on the block service; retries with backoff. This cleanly separates the *notification* (tiny, fan-out) from the *bulk transfer* (CDN-served, cache-friendly).

---

### Scaling to 10x / 100x (Hard)
Q13. Where does the system break first at scale?
> **Expected answer:** The *metadata layer*, not the blob store. Blocks are immutable, content-addressed, and served by an object store/CDN that scales horizontally and dedupes — near-linear. Metadata is transactional, relational (tree ops, versions, ACLs), and hot on every sync; at 25 trillion rows it's the bottleneck for commits, delta queries, and long-poll connection state. Second bottleneck: the notification/long-poll fleet — millions of persistent connections (one+ per active device) consume memory and file descriptors. Block storage is rarely the first thing to break.
> **Numbers to ground the answer:** ~500M users × ~2 devices actively syncing = up to ~1B long-poll connections at peak → at ~100k connections/server that's ~10k notification servers. Commit QPS: if 50M active users commit a few changes/hour, that's low-thousands to tens-of-thousands commit QPS spread across metadata shards — each shard handles a slice of namespaces.

Q14. Shard metadata and blocks. Apply consistent hashing (Lesson 19).
> **Expected sharding strategy:** Metadata sharded by `ns_id` (namespace = user root or shared folder) — the unit of transaction and permission, keeping commits/deltas single-shard. Use a shard-mapping (directory service) rather than pure hash so you can *split* a hot namespace's neighbors and rebalance; Dropbox used a lookup layer over sharded MySQL. Blocks sharded by `block_hash` via consistent hashing with virtual nodes — user-independent, enables global dedup and smooth rebalancing when adding storage nodes.
> **Hot spot problem:** A giant shared folder (a company's 10M-file namespace with thousands of collaborators) makes one metadata shard hot on both commits and delta reads. Detect via per-shard QPS/row-count. Fix: allow a namespace to be *split* into sub-namespaces, or move it to a dedicated shard; cache the hot file-tree/ACL in front of the shard; use read replicas for delta/list reads while commits go to the primary. For blocks, a "hot block" (a common file everyone stores) is naturally handled by CDN caching + replication.

Q15. Caching strategy across the sync stack.
> **Expected layered cache design:**
> - Client local disk = a cache of cloud state (selective/smart sync keeps placeholders, hydrates on access with LRU eviction).
> - Client local index DB caches block-hash lists so delta computation is offline and instant.
> - Metadata read cache (Redis/memcached) for hot file trees, ACLs, and namespace cursors — served in front of MySQL shards.
> - Block cache: CDN + edge cache for hot blocks (content-addressed = perfect cache key, immutable = never invalidate).
> **Cache invalidation trap:** Metadata is mutable (renames, moves, permission changes) so its cache *does* need invalidation — the trap is stale ACLs: revoke a user's access, but a cached permission check still lets them fetch a block. Fix: version the ACL cache with the namespace cursor and invalidate on any permission change; blocks themselves are content-addressed so anyone with the hash could fetch them — therefore access control must be enforced at the *metadata/authorization* layer (short-lived signed block URLs), not by hiding the block store. Immutable blocks = no invalidation ever; mutable metadata = invalidate precisely and enforce authz before issuing block URLs.

Q16. Cost/efficiency at scale.
> **Expected answer:** (1) Block-level dedup (global content-addressing) — store identical blocks once across all users/versions; huge savings for shared and common content. (2) Compression of blocks before storage. (3) Delta sync — transfer only changed chunks, saving bandwidth (the dominant client-side cost). (4) Cold-storage tiering — old file versions and rarely-accessed files move to cheaper archival tiers (Dropbox's Magic Pocket + cold tiers vs S3). (5) Smart/selective sync — don't materialize 50 GB on every device; keep placeholders and hydrate on demand. (6) Reference-counted GC to reclaim orphaned blocks. (7) Erasure coding instead of full 3× replication for durability at lower storage overhead.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)
**H1.** Content-defined chunking internals: explain the rolling hash (Rabin fingerprint), why you bound min/max chunk size, and the trade-off between average chunk size and dedup ratio vs metadata overhead. How does this compare to the rsync algorithm's weak+strong checksum approach?

**H2.** Sharing & authorization: blocks are content-addressed and technically fetchable by anyone with the hash. How do you enforce per-user ACLs, prevent "hash guessing" data leaks, handle inherited permissions on nested shared folders, and issue short-lived signed URLs so revocation actually takes effect? What about GDPR "delete my data" when blocks are deduped across users (refcount > 1)?

**H3.** Operational: migrate the block store from S3 to your own on-prem store (Dropbox's real "Magic Pocket" migration of exabytes) with zero downtime and zero data loss. How do you dual-write, verify with Merkle hashes, and cut over per-namespace behind a flag while sync traffic continues?

**H4.** Observability: what do you instrument for sync health? Sync latency (commit → visible on other devices), delta query latency, block upload/download throughput and resume rate, conflict/conflicted-copy rate, long-poll connection churn, dedup ratio, and metadata shard hot-spotting. How do you alert on "a namespace stopped converging"?

**H5.** 'Undo a bad decision': you shipped fixed-size chunking and dedup/bandwidth are poor because of the insert-shift problem. How do you migrate 25 trillion files to content-defined chunking incrementally (re-chunk lazily on next write, dual-index old+new block lists) without a flag day and without breaking existing version history?

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. Thinking the problem is "storing files." It's *sync and metadata*. The object store is a solved commodity; the delta protocol, conflict resolution, and 25-trillion-row metadata sharding are the actual engineering.
2. Using fixed-size chunks and not realizing insertion breaks delta sync entirely. Content-defined chunking with a rolling hash is the non-negotiable core insight.
3. Silently resolving conflicts (last-writer-wins overwrite). Real systems create conflicted copies and never lose user data; separate content conflicts from structural (tree) conflicts.

**The one insight that makes an answer truly impressive:**
The commit is the single atomic point of visibility, and everything else is idempotent around it: content-addressed block PUTs are idempotent by hash, commits are idempotent by request-id, and downloads are cursor-based deltas. This makes the whole sync protocol crash-safe and resumable at every step — a half-finished sync never corrupts state. Naming that invariant is a senior signal.

**Suggested follow-up reading:**
- Dropbox Engineering: "Streaming File Synchronization," "Rewriting the heart of our sync engine (Nucleus)," and the "Magic Pocket" exabyte-storage posts.
- The rsync algorithm paper (Tridgell) and LBFS (content-defined chunking, SOSP 2001).

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
