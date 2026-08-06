# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 52 of 63 — Phase 2: System Design Track (Design 24 of 35)
**Topic:** Collaborative Document Editor (Google Docs) — CRDT / OT
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
Real-time collaborative editing is the problem of letting N people type into the *same character stream simultaneously* and have every replica converge to an identical document — with sub-100ms local echo, offline support, and no lost keystrokes. Two families of algorithms solve it: **Operational Transformation (OT)**, which Google Docs uses (transform concurrent ops against each other so they compose correctly), and **CRDTs** (Conflict-free Replicated Data Types), which encode positions so concurrent inserts merge deterministically without a central transform. What makes it hard: concurrency is the *normal* case not the exception, intention preservation (what the user meant must survive a merge), and the classic correctness traps — the OT **TP2** puzzle and CRDT **interleaving/tombstone-growth** problems — are subtle enough that most naive implementations are provably wrong.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Networking / long-lived connections & WebSockets (taught in Phase 1, Lesson 7):** A persistent bidirectional channel lets the server push other users' edits as they happen. Every open document holds a WebSocket per collaborator to a session/edit server; this is the transport for the op stream in both directions.

**Consistency models / CAP / linearizability (taught in Phase 1, Lesson 2):** The hard requirement here is **strong eventual consistency** — replicas may diverge transiently but *must* converge to the same state once they've seen the same set of ops. This is a specific, provable property (the whole point of CRDTs) distinct from generic "eventual consistency."

**Leader election / single-writer ordering (taught in Phase 1, Lesson 17):** OT needs a **central sequencer** — one authority per document that assigns a total order (revision numbers) to incoming ops so all clients transform against the same history. That server is effectively the per-document leader.

**Kafka / event-driven & append-only log (taught in Phase 1, Lesson 21):** The document's edit history is an **append-only op log**; the current state is a fold/replay over that log. Persistence, replay for late-joiners, and recovery all come from treating edits as an ordered event stream.

**Caches / Redis (taught in Phase 1, Lesson 24):** The live, in-memory document state + recent op history + presence (cursors) sit in memory on the edit server (and Redis for cross-node sharing), because you can't hit disk per keystroke.

**Merkle trees / hashing (taught in Phase 1, Lesson 5):** Used to verify two replicas actually converged — hash the document state / op-set so a client and server can cheaply detect divergence and trigger reconciliation.

**Capacity estimation (taught in Phase 1, Lesson 28):** Ops/sec = concurrent editors × keystrokes/sec; memory = active docs × doc size + op history. We size the sequencer and fanout below.

> **Recap box:**
> - WebSocket per collaborator = the bidirectional op transport.
> - Strong eventual consistency = replicas converge given the same op set (not just "eventually-ish").
> - OT needs a central per-doc sequencer (leader) assigning revision numbers.
> - The doc is a fold over an append-only op log — state = replay(ops).
> - Merkle/state hash = cheap convergence check.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. Why can't you just use last-write-wins on the whole document (or even per-paragraph locking) for collaborative editing?
> **What a strong answer covers:** LWW on the whole doc means whoever saves last clobbers everyone else's edits — catastrophic data loss, the opposite of collaboration. Locking a paragraph kills the real-time feel (you'd wait for a lock to type) and doesn't handle two people editing the same sentence, which is the *common* case. Collaborative editing requires **merging concurrent character-level edits** so both users' intentions survive — that's what OT and CRDTs do. The unit of concurrency is the keystroke, and there is no "conflict dialog"; convergence must be automatic.
> **Common weak answer:** "Use a database transaction / optimistic locking on save" — that's document-versioning, not real-time co-editing; it forces a merge-conflict UX and can't do sub-second shared typing.
> **Mentor follow-up if they answer well:** Two users both type a character at position 5 at the same instant. What are the two possible converged results, and does it matter which one you pick as long as everyone agrees?

Q2. Estimate the op rate and fanout for a document with 20 active collaborators all typing.
> **What a strong answer covers:** A fast typist is ~5–8 keystrokes/sec; assume ~5 ops/sec/active-typist. If 20 are open but ~5 actively typing at once → ~25 ops/sec *inbound* to the sequencer for that doc. Each op must fan out to the other ~19 collaborators → ~25 × 19 ≈ **~475 outbound op-messages/sec** for one document. That's tiny per-doc, but a server hosting 10,000 active docs handles ~250K inbound + ~4.75M outbound ops/sec — so the design scales by **sharding documents across edit servers**, one sequencer per doc. Ops are small (a few dozen bytes: type, position, char, revision), so bandwidth is modest; the cost is connection count and per-doc CPU for transform.
> **Mentor follow-up:** Ops are tiny but chatty. How do you cut the message rate without hurting the sub-100ms feel? (Batching/coalescing keystrokes into ~50–200ms flushes.)

Q3. Local echo vs waiting for the server: when a user types, do you render immediately or after the server acks?
> **What a strong answer covers:** Render **immediately (optimistic local echo)** — the keystroke appears in <16ms, then the op is sent to the server asynchronously. This is non-negotiable for UX; waiting for a round-trip (50–200ms) per keystroke feels broken. The consequence is that the local state is *ahead* of the server, so incoming remote ops must be **transformed against the local unacknowledged ops** (OT) or merged by position identity (CRDT) before applying. The client keeps a buffer of sent-but-unacked ops for exactly this reason.
> **Red flag answer:** "Send to server, wait for confirmation, then render" — laggy, and ignores that the whole difficulty is reconciling optimistic local state with remote ops.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Draw the architecture for a real-time collaborative editor (OT-based, Google Docs style).
> **Key components expected:** Client editor (with local OT engine + unacked buffer), WebSocket gateway, per-document Edit/Session server (the sequencer, holds live doc state + op history), Op log / persistence store, Snapshot store, Presence service (cursors/selections), Document metadata + ACL service, Redis for live state sharing, offline sync path.
> **Architecture diagram (text):**
```
  Client A ──WS──┐                                   ┌── snapshot every N ops
  (local OT +    │                                    ▼
   unacked buf)  ▼                          ┌──────────────────┐
             ┌───────────────┐   op(rev)    │  Snapshot store   │
  Client B ──│  WS Gateway   │──────────────▶ (blob/DB)         │
  (local OT) │ (sticky by    │              └──────────────────┘
             │  doc_id)      │                        ▲
  Client C ──│               │                        │ periodic fold
             └──────┬────────┘                        │
                    │ route by doc_id            ┌─────┴────────────┐
                    ▼                            │  Op Log (append-  │
          ┌────────────────────────┐   append   │  only, per doc)   │
          │  Edit/Session Server    │──────────▶ │  Kafka/DB         │
          │  (SEQUENCER for doc D)   │            └──────────────────┘
          │  - assign revision #     │
          │  - transform(op, history)│ broadcast transformed op + new rev
          │  - live doc state (RAM)  │───────────▶ back to B, C (and ack A)
          │  - recent op history     │
          └───────┬─────────┬────────┘
                  │         │
          presence│         │ ACL / metadata
                  ▼         ▼
          ┌──────────┐  ┌───────────────┐
          │ Presence │  │ Doc metadata  │
          │ (cursors)│  │ + permissions │
          └──────────┘  └───────────────┘
```
> **What separates SDE2 from SDE3 here:** SDE2 draws the WebSocket + a server. SDE3 makes the Edit server an explicit **single sequencer per document** (leader) that (a) assigns a monotonic revision number, (b) transforms each incoming op against the ops the client hadn't yet seen, and (c) broadcasts the transformed op with the new revision. SDE3 also separates **live state (RAM, per keystroke)** from **durable op log + periodic snapshots (persistence)**, and explains how a late-joiner boots from the latest snapshot + replaying the tail of the op log.

Q5. Trace one keystroke from user A (who is 3 revisions behind the server) to convergence on all clients.
> **Expected trace:**
> 1. A types 'x' at position 5. Client **immediately renders** it and creates op `insert('x', pos=5, base_rev=42)` (42 = last revision A has seen). Op goes into A's **unacked buffer**; sent over WS.
> 2. Server is at revision 45 (ops 43,44,45 arrived from others since A's base_rev=42). It **transforms** A's op against ops 43–45 (adjusting position 5 for any inserts/deletes before it) → transformed op'.
> 3. Server applies op' to the master state, assigns it **revision 46**, appends to the op log, and broadcasts op' (rev 46) to B and C; sends A an **ack (rev 46)**.
> 4. A receives ack, removes the op from its unacked buffer, advances to rev 46. B and C receive op', **transform it against their own unacked ops**, apply, advance to 46.
> 5. All replicas now hold identical state at revision 46.
> **Tricky part:** The **double transform** — the server transforms A's op forward against history, *and* each receiving client transforms the incoming op against its own local unacked ops. If either side skips a transform, positions drift and replicas diverge. Candidates also forget `base_rev` on the op — without it the server doesn't know how far to transform.

Q6. Design the client↔server op protocol.
> **Expected API design:** Over WebSocket, message types:
> - `OP {doc_id, client_id, base_rev, ops:[{type:insert|delete|retain, pos, chars, attrs}], client_seq}` (client→server).
> - `ACK {doc_id, client_seq, server_rev}` (server→client, confirming the client's op landed at server_rev).
> - `BROADCAST {doc_id, server_rev, ops:[...], origin_client}` (server→other clients).
> - `PRESENCE {doc_id, client_id, cursor_pos, selection_range}` (ephemeral, best-effort).
> - REST for lifecycle: `GET /v1/docs/{id}` (returns latest snapshot + rev), `GET /v1/docs/{id}/ops?since=rev` (replay tail).
> **What to push on:** (1) **Idempotency / dedup** — `client_seq` lets the server dedupe a re-sent op after reconnect (at-least-once transport). (2) **base_rev is mandatory** — it's the transform anchor. (3) **Ordering** — ops from one client must be applied in client_seq order (per-client causal order). (4) Attributes/retain ops model rich text (bold/formatting) as a separate concern from position — formatting is `retain(n){bold:true}`, using the ProseMirror/Quill "delta" op model.

---

### Data Modeling (Medium–Hard)

Q7. Design how the document, its op history, and snapshots are stored.
> **Expected schema:**
```sql
-- Documents metadata + current pointer
CREATE TABLE documents (
  doc_id       uuid PRIMARY KEY,
  owner_id     bigint,
  title        text,
  current_rev  bigint,          -- latest applied revision
  snapshot_uri text,            -- latest snapshot blob
  snapshot_rev bigint,          -- revision the snapshot represents
  created_at   timestamp, updated_at timestamp
);

-- Append-only op log — the source of truth; state = snapshot + replay(ops after snapshot_rev)
CREATE TABLE doc_ops (
  doc_id    uuid,        -- partition key
  rev       bigint,      -- clustering key, monotonic per doc (the total order)
  client_id text,
  client_seq bigint,
  op        blob,        -- serialized transformed op (insert/delete/retain deltas)
  ts        timestamp,
  PRIMARY KEY (doc_id, rev)
) WITH CLUSTERING ORDER BY (rev ASC);

-- Access control
CREATE TABLE doc_acl (
  doc_id uuid, principal_id bigint,
  role text,               -- owner/editor/commenter/viewer
  PRIMARY KEY (doc_id, principal_id)
);
```
```
# Live state (RAM on edit server + Redis for sharing/failover)
live:{doc_id} -> { state: rope/piece-table, current_rev, recent_ops[tail], collaborators[] }
```
> **Index choices and why:** `doc_ops` clustered by `rev` gives an ordered range scan for replay (`WHERE doc_id=? AND rev > snapshot_rev`). No secondary index needed — access is always "this doc's ops in revision order." ACL indexed by doc_id (check on open) and reverse-indexed by principal_id (list a user's docs).
> **Partitioning key and why:** Everything partitioned by **doc_id** — a document is the unit of concurrency, locality, and sharding. All ops, the snapshot pointer, and the sequencer for one doc live together, so the single-sequencer-per-doc model maps to a single partition + single owning server. This is why one huge document (not many docs) is the scaling hard case.

Q8. A user opens a 200-page document with 50,000 revisions of history. How do you load it fast?
> **Expected answer:** Never replay 50,000 ops. Maintain **periodic snapshots** (fold the op log into a materialized document state every N ops, e.g., every 100–1000 revisions). On open: load the **latest snapshot** (rev 49,500) + replay only the **tail** (ops 49,501–50,000) → a few hundred ops, milliseconds. The snapshot is stored as an efficient text structure (a **rope** or **piece table**) so inserts/deletes are O(log n), not O(n) string copies. The client then subscribes to the live op stream from `current_rev`. Old op-log entries beyond the snapshot can be compacted/archived (kept for version history / audit, but not on the hot path).
> **Trap:** Storing only the op log and replaying from rev 0 on every open — O(history) load time that degrades linearly forever. Snapshots bound it. Also: naive string concatenation for a 200-page doc is O(n) per keystroke — you need a rope/piece-table.

Q9. What consistency guarantee does this system actually require, and where does CAP bite?
> **Expected answer:** **Strong eventual consistency (SEC):** all replicas that have applied the same set of ops are byte-identical, regardless of order of receipt (CRDT) or via transform against a common total order (OT). During a partition, a client can keep editing **offline (AP choice)** — availability of editing wins; convergence happens on reconnect by exchanging missed ops. You do *not* need linearizability of the document content (there's no single "correct" interleaving of two simultaneous inserts — any agreed result is fine, as long as intention is preserved). Where CP-like strictness *is* required: **ACL/permission checks** (a revoked editor must not have ops accepted) and the **revision counter monotonicity** on the sequencer.
> **Mentor pushback:** Two users edit offline for an hour and both delete-then-retype the same paragraph differently. On reconnect, OT must transform a long divergent history — does it still converge and preserve intention? → OT convergence requires the transform functions satisfy **TP1** (and TP2 for peer-to-peer); Google Docs sidesteps TP2 by funneling everything through the central sequencer so there's always a single linear history to transform against. Offline divergence is bounded by replaying each side's ops through the sequencer's order on reconnect.

---

### Low-Level Design (Hard)

Q10. Design the core concurrency-resolution engine. Contrast OT and CRDT and pick one.
> **Problem statement:** Two (or N) clients concurrently issue character ops against the same base state; every replica must converge to identical content while preserving each user's intention, with no central lock.
> **Naive solution:** Apply ops in arrival order by raw index. If A inserts at index 5 and B deletes index 3 concurrently, applying B's delete shifts everything, so A's "index 5" now points at the wrong character → divergence and corruption.
> **Why naive fails at scale:** Raw indices are not stable under concurrent edits; positions are relative to a state the other op didn't see. Without transformation (OT) or position-identity (CRDT), replicas diverge — the document literally differs per user.
> **Expected optimal approach:**
> - **OT (Google Docs):** Keep a central sequencer + a `transform(op_a, op_b)` function. When op_a and op_b are concurrent, transform op_a against op_b to get op_a' that has the *same effect* on the state that already includes op_b. Requires **TP1**: `apply(apply(S, a), transform(b,a)) == apply(apply(S, b), transform(a,b))`. Positions are adjusted (an insert before your position shifts you right; a delete before you shifts you left). Compact (ops are small), but transform logic is subtle and history-dependent.
> - **CRDT (e.g., RGA / Logoot / Yjs):** Give every inserted character a **globally unique, totally-ordered position identifier** (e.g., a fractional index or a dense identifier between its neighbors + a site-id tiebreaker). Inserts carry their own position, so concurrent inserts merge deterministically by comparing identifiers — **no transform, no central sequencer needed** (works P2P). Deletes are **tombstones** (mark, don't remove). Trade-off: metadata/tombstone growth.
> - **Pick:** For a *centralized* Google-Docs-style service, **OT** is the pragmatic choice (compact ops, single sequencer already exists). For **offline-first / P2P / local-first** (Figma-ish, Yjs-based apps), **CRDT** wins because it needs no central authority.
> **Pseudo-code or class diagram:**
```
# OT sequencer core
on_op(op, client):
    # transform incoming op against every server op the client hasn't seen
    for hist_op in history[op.base_rev + 1 : current_rev + 1]:
        op = transform(op, hist_op)          # TP1-correct transform
    apply(master_state, op)
    op.rev = ++current_rev
    log.append(op); ack(client, op.rev)
    broadcast(op, exclude=client)

transform(a, b):                              # a against b (both insert example)
    if a.type==INSERT and b.type==INSERT:
        if a.pos < b.pos or (a.pos==b.pos and a.site < b.site): return a       # a unaffected
        else: return a.shift(+len(b.chars))                                    # b was before a
    # ... delete/insert, delete/delete cases (deletes shrink positions) ...

# CRDT insert (RGA-style) — no server transform needed
insert_between(left_id, right_id, ch, site_id, clock):
    new_id = dense_id_between(left_id, right_id)   # unique, ordered position
    element = {id:new_id, ch, site:site_id, clock, deleted:false}
    merge(element)                                  # deterministic by id ordering
```

Q11. Concurrency: two users insert at the *exact same position* at the same revision. Resolve deterministically.
> **Scenario:** Both A and B, at base_rev 42, insert a character at position 5. Their ops are concurrent — neither saw the other. If clients broke the tie differently, A might get "AB" and B might get "BA" → permanent divergence.
> **Expected fix:** A deterministic **total-order tiebreaker** that every replica computes identically. In OT: the sequencer imposes an order (whichever op it processes first gets the lower revision; the second is transformed to insert *after*), so all clients see the same sequencer order and converge. In CRDT: the position identifiers include a **site-id (and logical clock)**; when two ids would collide, the site-id breaks the tie the same way on every replica (e.g., lower site-id goes left). The key property: the tiebreak is a pure function of op metadata, not of arrival order at any given replica.
> **Follow-up:** What if the sequencer (leader) for the doc dies mid-op? Recover by electing a new edit server that loads the latest snapshot + op log and **resumes at the last durably-logged revision**; unacked client ops are re-sent (dedup by client_seq) and re-sequenced. Because the op log is the source of truth and appends are durable before ack, no acked op is lost.

Q12. Failure deep-dive: a user edits offline for 30 minutes, then reconnects while the doc advanced 400 revisions.
> **Scenario:** Client's `base_rev` is 42; server is now at 442. The client has 60 local unacked ops queued.
> **Expected handling:** On reconnect: (1) client sends its queued ops with `base_rev=42` and client_seqs. (2) The sequencer **transforms each client op forward against revisions 43–442** (and each subsequent client op against the accumulating history), applying and assigning new revisions — this is exactly the normal OT flow, just with a large gap. (3) Server sends the client the missed ops (43–442) which the client transforms against its still-unacked local ops. (4) Both converge. Practically: cap offline op buffers, dedup via client_seq (reconnect may resend), and if transform cost is huge, fall back to a **3-way merge from the last common snapshot**. For deletes of content another user also touched, tombstones/transforms ensure no crash — worst case an intention-preservation compromise, never corruption. If the doc was *deleted* or the user's access *revoked* while offline, reject the ops at the ACL gate and surface a merge/permission error.

---

### Scaling to 10x / 100x (Hard)

Q13. At 10x, where does this break first?
> **Expected answer:** Two places: (1) a **single hyper-collaborative document** (hundreds of simultaneous editors on one doc) — because OT funnels through **one sequencer per doc**, that single server's transform CPU and fanout become the bottleneck (you can't shard one doc's sequencing without losing the total order). (2) **Connection count** across many docs — millions of open WebSockets. The op *data* volume is trivial (tiny ops); it's per-doc CPU and connection fanout that hurt.
> **Numbers to ground the answer:** One doc with 200 active editors: ~5 ops/sec × (say) 30 concurrent typists ≈ 150 inbound ops/sec, each transformed against a growing history and fanned to 199 others ≈ ~30K outbound msgs/sec on one sequencer — plus transform is O(history-gap) per op. That's the wall. Across docs: 10M concurrent editors ÷ ~50K WS/server = ~200 gateway servers; the sequencers shard by doc_id.

Q14. How do you shard, and what's the hotspot?
> **Expected sharding strategy:** Shard by **doc_id** via consistent hashing — each document is owned by exactly one edit/sequencer server (its leader). This gives natural horizontal scale across the *catalog* of documents and preserves the single-total-order-per-doc invariant OT needs. WebSocket gateways are sticky-routed by doc_id so all of a doc's collaborators land on the same sequencer.
> **Hot spot problem:** The hotspot is **one extremely popular document** (a viral shared doc, a company all-hands notes doc with thousands of viewers). You cannot shard its sequencing. Mitigations: (1) **read/write split** — most "collaborators" are actually *viewers*; only broadcast full ops to active editors, and serve viewers a throttled/coalesced stream (or periodic snapshot diffs) so fanout isn't N². (2) **op batching/coalescing** (50–200ms flushes) cuts message rate. (3) cap simultaneous *editors* (Google Docs historically limited concurrent editors ~100) while allowing unlimited viewers on a cheaper pull path. (4) Presence (cursors) is best-effort and can be sampled/throttled independently.

Q15. Design the caching layers and the hardest invalidation problem.
> **Expected layered cache design:** L1 = client-local document state + unacked op buffer (authoritative for the local view, sub-16ms echo). L2 = the sequencer's **in-RAM live doc state + recent op tail** (per keystroke; never disk). L3 = Redis for sharing live state across nodes and for fast sequencer failover. Durable = op log + periodic snapshots in the store. Snapshots are the "cache" of folded history.
> **Cache invalidation trap:** **Snapshot vs live-tail coherence** and **presence staleness**. When you take a snapshot at rev N, in-flight ops at rev N+1 must not be lost or double-counted — the snapshot must record its exact `snapshot_rev` and the loader must replay strictly `rev > snapshot_rev` (off-by-one here duplicates or drops an op). Presence/cursor caches go stale the instant someone disconnects ungracefully — solved with short TTL + heartbeat so a dead cursor vanishes in ~seconds rather than lingering. The genuinely hard one: after a sequencer **failover**, the new leader must not accept an op that the old leader already acked-and-logged (dedup by client_seq) nor skip one that was logged-but-not-broadcast (re-broadcast from the log).

Q16. How do you keep this efficient at scale?
> **Expected answer:** (1) **Snapshots + op-log compaction** — bound load time and storage; archive ancient ops to cold storage (version history is rarely accessed). (2) **Op coalescing/batching** — merge a burst of keystrokes into one op before sequencing/fanout, cutting message and transform volume dramatically. (3) **Tombstone garbage collection (CRDT)** — the big one for CRDTs: deleted characters leave tombstones that grow unboundedly; GC them once all replicas have acknowledged (causal stability), or the doc's metadata bloats past the actual content. (4) **Idle-doc eviction** — unload docs with no active editors from RAM to storage; rehydrate on next open. (5) **Viewer/editor split** so passive readers don't cost N² fanout. (6) Efficient text structure (rope/piece table) so per-op cost is O(log n), not O(n).

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)

**H1.** State and defend the **OT transform properties TP1 and TP2**. TP1 (convergence for a pair of concurrent ops applied in either order) is required by any OT system; TP2 (transform-order independence for 3+ concurrent ops in a decentralized setting) is *notoriously* hard to satisfy and many published OT algorithms were later shown to violate it. How does Google Docs avoid the TP2 problem entirely? (Answer: a **central sequencer** imposes a single linear history, so you only ever transform against a totally-ordered sequence — you never need TP2's peer-to-peer property. This is the core architectural reason Docs is centralized.)

**H2.** CRDT internals: explain how a sequence CRDT (RGA or Logoot/LSEQ) assigns **position identifiers** so concurrent inserts converge without a server, why identifiers can grow in size (interleaving / the "identifier explosion" in fractional indexing), and the **interleaving anomaly** where two users typing adjacent words can get their characters interleaved. How do modern CRDTs (Yjs, Automerge, Fugue) mitigate it?

**H3.** Rich text + comments + suggestions: it's not just characters. How do you model **formatting** (bold spanning a range), **comments anchored to a range**, and **suggested edits** so they survive concurrent edits that move/delete the anchored text? (Answer: formatting as `retain(n){attrs}` deltas; comment/suggestion anchors as **stable position markers / relative positions** that transform along with the text, so a comment stays attached to its sentence even as characters are inserted before it — and gracefully "orphans" if its whole anchor range is deleted.)

**H4.** What do you instrument? Op round-trip latency (keystroke→ack p50/p99), sequencer transform time per op, per-doc fanout size, WebSocket connection churn, **convergence verification** (periodic state-hash/Merkle comparison across replicas — alert on any divergence, which is a correctness bug not a perf one), snapshot lag, offline-reconnect merge duration. The one SLO that matters: **local echo <16ms and remote-op apply p99 <200ms** with **zero divergence**.

**H5.** "Undo a bad decision": you shipped an OT implementation whose delete/insert transform violates TP1 in a rare case, and some documents are silently diverging between collaborators. How do you detect, contain, and migrate? (Answer: deploy periodic **state-hash comparison** to detect divergence in the wild; on mismatch, treat the server's op-log-replayed state as canonical and **force-resync** clients from the latest snapshot; fix the transform function behind a version flag; backfill-verify historical docs by replaying the log through the corrected transform and comparing hashes. Consider migrating to a CRDT for classes of docs where correctness proofs are cleaner.)

---

### Mentor's Closing Notes

**Top 3 things most candidates get wrong on this topic:**
1. Treating concurrency as an edge case with "conflict resolution" — in collaborative editing, concurrency is the *normal path* and must converge automatically with no user-facing merge dialog.
2. Using raw character indices without transformation/position-identity — the single most common way to silently corrupt the document under concurrent edits.
3. Ignoring the central-sequencer requirement of OT (and thus TP2) — hand-waving "each client transforms" without a total order, which doesn't converge for 3+ peers.

**The one insight that makes an answer truly impressive:**
The document is a **fold over a totally-ordered op log**, and OT vs CRDT is really a choice about *where the total order comes from* — OT borrows it from a central sequencer (so it can keep ops tiny and skip TP2), CRDT bakes it into globally-ordered position identifiers (so it needs no server, at the cost of metadata/tombstone growth). Every other decision — snapshots, offline sync, failover, viewer/editor split — is downstream of that one choice. Naming that trade-off explicitly is what separates a memorized answer from an engineer.

**Suggested follow-up reading:**
- Ellis & Gibbs original OT paper ("Concurrency Control in Groupware Systems") and the "Jupiter" collaboration system (Nichols et al.) — the direct ancestor of the Google Docs model.
- Shapiro et al. "A Comprehensive Study of Convergent and Commutative Replicated Data Types" (CRDTs), plus the Yjs and Automerge (and Fugue/Peritext) writeups for modern sequence-CRDT and rich-text handling.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
