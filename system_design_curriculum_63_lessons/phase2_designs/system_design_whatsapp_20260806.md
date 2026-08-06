# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 47 of 63 — Phase 2: System Design Track (Design 19 of 35)
**Topic:** WhatsApp — End-to-End Messaging at Scale
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
WhatsApp moves ~100B+ messages/day with a team that famously ran on ~50 engineers and a few hundred Erlang/FreeBSD boxes, each holding millions of long-lived TCP connections. The core problem is not throughput — it's **presence + delivery semantics over unreliable mobile networks**: exactly-once *display*, at-least-once *transport*, ordered per-conversation, with end-to-end encryption (Signal protocol) so the server is a blind router. What makes it hard: a message must survive the recipient being offline for days, arrive in order, be deduplicated, fan out to group members and multiple devices, all while the server can't read the payload, and the whole thing must feel instant (sub-second delivery when both parties are online).

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Networking / OSI / long-lived connections (taught in Phase 1, Lesson 7):** TCP gives you an ordered, reliable byte stream; the cost is a stateful connection and a server that must hold a socket per client. Mobile clients keep a persistent connection (originally XMPP-derived, later a custom binary protocol over TCP/TLS, with WebSocket fallbacks) so the server can *push* without polling. Here, every online phone owns one long-lived socket to a "chat connection" server; that socket is the delivery channel and the presence signal.

**Kafka / event-driven & durable queues (taught in Phase 1, Lesson 21):** An append-only, partitioned log decouples producer from consumer and survives consumer downtime. Here the pattern (WhatsApp historically used Erlang mnesia + custom queues, but conceptually a durable per-user mailbox) is: an inbound message is written to the recipient's durable queue *before* an ack goes back to the sender, so an offline recipient loses nothing.

**Consistent hashing (taught in Phase 1, Lesson 19):** Map users and nodes onto a ring so a session/mailbox lives on a deterministic node, and adding capacity remaps only 1/N. Used to shard the connection registry ("which server holds user X's socket?") and the per-user message store.

**HA / replication / quorum (taught in Phase 1, Lesson 3):** Leader-follower replication with quorum acks prevents message loss when a node dies. The undelivered-message store is replicated (e.g., 3 replicas, W+R>N) so a crashed mailbox node doesn't drop pending messages.

**Caches / Redis (taught in Phase 1, Lesson 24):** In-memory KV/pubsub for hot, ephemeral state. Presence (online/last-seen), the routing table (user → connection-server), and typing indicators live in Redis/pubsub, not in durable storage — they're cheap to rebuild and change constantly.

**CAP / consistency (taught in Phase 1, Lesson 2):** Messaging is an AP system for *availability* but demands strong *per-conversation ordering*. You accept that global order across conversations is meaningless; you enforce a total order within a single chat via monotonic sequence numbers.

**Capacity estimation (taught in Phase 1, Lesson 28):** QPS = users × msgs/day ÷ 86,400 × peak factor; connection count = concurrent online users; storage = only *undelivered* messages (E2E means the server doesn't keep delivered plaintext). We size all three below.

> **Recap box:**
> - Long-lived TCP socket per online device = the push channel AND presence signal.
> - Durable per-recipient mailbox written before sender ack = no loss when offline.
> - Consistent hashing routes "who holds user X's socket / mailbox."
> - Redis holds presence + routing (ephemeral); replicated store holds undelivered messages (durable).
> - Ordering is per-conversation via sequence numbers, not global.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. Why does WhatsApp keep a persistent connection per device instead of having clients poll or use plain request/response HTTP?
> **What a strong answer covers:** Push latency and battery. Polling for new messages every N seconds means average latency N/2, wasted radio wakeups (battery killer on mobile), and massive redundant load. A persistent TCP/TLS socket lets the server push a message the instant it arrives (sub-second), and the same socket doubles as the liveness/presence signal — if the socket is up, the user is "online." The trade-off is server statefulness: each connection server must hold millions of open sockets (WhatsApp famously tuned FreeBSD/Erlang to ~2M+ connections per box).
> **Common weak answer:** "Use HTTP long-polling" — workable as a fallback but ignores that you still need a durable mailbox for offline users, and long-polling doesn't give you free presence.
> **Mentor follow-up if they answer well:** The socket is up but the TCP connection silently died (NAT timeout on a mobile carrier). How do you detect a dead peer, and what's your heartbeat interval trade-off vs battery?

Q2. Estimate concurrent connections and message QPS. Assume 2B users, 50% online at peak, 60 messages sent/user/day.
> **What a strong answer covers:** Concurrent online = 2B × 0.5 = **1B simultaneous TCP connections**. At ~2M connections/box that's ~500 connection servers minimum (call it 1000+ with headroom/redundancy). Messages/day = 2B × 60 = 120B/day ≈ **1.4M msgs/sec average**, peak factor ~3 → **~4M msgs/sec**. Each message triggers a delivery attempt + an ack + often a group fanout, so internal event rate is several × that. Storage: server stores only *undelivered* — if 95% deliver instantly, you persist ~7B messages transiently/day, but they're deleted on delivery, so steady-state undelivered backlog is small (GBs–low TBs), not petabytes.
> **Mentor follow-up:** Now add multi-device (avg 1.5 devices/user) and groups (avg group message hits 8 recipients). Recompute the *delivery* event rate vs the *send* rate.

Q3. Would you store delivered message plaintext on the server? Defend the choice in the context of E2E encryption.
> **What a strong answer covers:** No. With Signal-protocol E2E, the server never has plaintext — it stores/forwards ciphertext blobs it cannot read, and deletes them once delivered and acked by the recipient device. The message history of record lives **on the devices**, not the server. This is a deliberate product+privacy+cost decision: it shrinks server storage from "petabytes of history" to "a transient queue of undelivered ciphertext," and makes the server a blind relay. Trade-off: no server-side search, and multi-device history sync becomes a hard client-side problem (encrypted key transfer / companion-device linking).
> **Red flag answer:** "Store all messages server-side for history/search" — breaks the E2E privacy model and explodes storage; that's Telegram-cloud-chat semantics, not WhatsApp.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Draw the end-to-end architecture for sending a 1:1 message where the recipient may be online or offline.
> **Key components expected:** Client, LB/edge (TLS termination + connection routing), Connection/Chat servers (hold sockets), Session/Routing registry (user→server, in Redis), Message Router service, durable Message Queue/Mailbox store (replicated), Presence service, Ack/receipt pipeline, Push notification gateway (APNs/FCM) for offline wake, Media service + blob store/CDN.
> **Architecture diagram (text):**
```
   Device A ──TLS/persistent socket──▶ ┌──────────────────┐
                                       │ Connection Server │ (holds A's socket)
                                       │   (edge, region)  │
                                       └─────────┬─────────┘
                                                 │ inbound msg (ciphertext)
                                                 ▼
                                       ┌──────────────────┐   lookup: where is B?
                                       │  Message Router   │──────────┐
                                       └───┬───────────┬───┘          ▼
                          B online? push   │           │        ┌───────────────┐
                                           │           │        │ Session/Route │
                          ┌────────────────┘           │        │ Registry(Redis)│
                          ▼                             │        │ user→connSrv  │
                ┌──────────────────┐                    │        └───────────────┘
                │ Connection Server│  push to B's socket │  B offline: persist
                │  (holds B's sock)│◀───────────────────┘          │
                └────────┬─────────┘                               ▼
                         │ deliver                          ┌───────────────┐
                         ▼                                  │ Mailbox Store │ (replicated,
                   Device B ──ack──▶ (back through router)  │ per-user queue│  W+R>N)
                                                            └───────┬───────┘
                                                                    │ B offline?
                                                                    ▼
                                                            ┌───────────────┐
                                                            │ Push Gateway  │──▶ APNs/FCM
                                                            └───────────────┘  (wake device)
   Media path: A ─▶ Media Svc ─▶ encrypted blob ─▶ Blob store/CDN ; message carries pointer+key
```
> **What separates SDE2 from SDE3 here:** SDE2 draws online push. SDE3 nails the **write-before-ack invariant**: the router must durably persist to the recipient's mailbox (or confirm live delivery) *before* the sender gets a server-ack (the single grey check). SDE3 also separates the *server-received* ack (✓) from *delivered-to-device* (✓✓) from *read* (blue ✓✓) as three distinct receipts flowing back through the pipeline, and handles the multi-device fanout (deliver to all of B's linked devices, each with its own Signal session).

Q5. Trace a message from A to B when B is offline, then comes online 2 hours later.
> **Expected trace:**
> 1. A encrypts payload for B's device(s) using the established Signal session (Double Ratchet), sends ciphertext over its socket to A's connection server.
> 2. Connection server hands to Message Router; router queries Session Registry → B has no live socket (offline).
> 3. Router writes ciphertext to **B's durable mailbox** (replicated, quorum ack), assigns a per-conversation sequence number, then returns **server-ack (✓)** to A.
> 4. Router asks Push Gateway to send a silent/normal push via APNs/FCM to wake B's device.
> 5. Two hours later B opens the app → establishes socket → connection server registers B in the Session Registry and triggers **mailbox drain**: pulls queued messages in sequence order, pushes to B.
> 6. B's device decrypts, sends **delivered-receipt (✓✓)**; router relays it to A and **deletes the message from B's mailbox**. When B reads, a **read-receipt** flows the same way (if enabled).
> **Tricky part:** Candidates forget the mailbox delete is gated on the *delivered* ack, not the push. If they delete on send, a crash between push and device-receipt loses the message. Also: ordering — the sequence number, not arrival time at the connection server, defines delivery order when draining.

Q6. Design the core protocol messages / API between client and server.
> **Expected API design:** This is a stateful binary protocol over the socket, not REST, but framed as message types:
> - `SEND {client_msg_id, conversation_id, seq_hint, ciphertext, recipient_device_ids[]}` → server responds `ACK {client_msg_id, server_msg_id, server_seq, ts}`.
> - `DELIVER {server_msg_id, conversation_id, seq, ciphertext}` (server→recipient device).
> - `RECEIPT {server_msg_id, type: delivered|read, from_device}`.
> - `PRESENCE {user_id, status: online|offline|typing, last_seen?}`.
> - Media over HTTPS REST: `POST /v1/media` (upload encrypted blob) → `{media_id, url}`; message body carries `media_id` + decryption key (encrypted E2E).
> **What to push on:** (1) **Idempotency** — `client_msg_id` (UUID generated on device) dedupes retries; a flaky network makes SEND retries the norm, and the server must map a repeated `client_msg_id` to the same `server_msg_id`. (2) **Ordering** — server assigns authoritative `server_seq` per conversation. (3) No offset pagination for history (history is client-local); server pagination only applies to mailbox drain, done in seq order. (4) Versioning of the binary protocol via a handshake version field.

---

### Data Modeling (Medium–Hard)

Q7. Design the primary stores: the session/routing registry, the per-user mailbox, and conversation metadata.
> **Expected schema:**
```
# Session / routing registry — Redis, ephemeral, sharded by user_id (consistent hashing)
route:{user_id} -> { device_id: {conn_server_id, socket_id, last_hb}, ... }  # TTL, refreshed by heartbeat
presence:{user_id} -> {status, last_seen_ts}                                  # TTL'd

# Undelivered mailbox — replicated durable store (wide-column / LSM), partition by recipient user_id
CREATE TABLE mailbox (
  recipient_id  bigint,     -- partition key
  server_seq    bigint,     -- clustering key, monotonic per recipient conversation
  server_msg_id bigint,
  conversation_id bigint,
  sender_id     bigint,
  ciphertext    blob,       -- server cannot decrypt
  created_at    timestamp,
  PRIMARY KEY (recipient_id, server_seq)
) WITH CLUSTERING ORDER BY (server_seq ASC);

# Group metadata — who's in the group (server needs this for fanout, NOT the content)
CREATE TABLE group_members (
  group_id  bigint,   -- partition key
  user_id   bigint,
  role      text,     -- admin/member
  joined_at timestamp,
  PRIMARY KEY (group_id, user_id)
);
```
> **Index choices and why:** Mailbox clustered by `server_seq` gives a natural in-order range scan for drain (`WHERE recipient_id=? ORDER BY server_seq`). No secondary indexes needed — the access pattern is always "drain my queue in order." Group membership partitioned by group_id gives a single-partition read of the full member list for fanout.
> **Partitioning key and why:** Mailbox by recipient_id → each user's undelivered queue co-located, drained by the one connection server holding their socket; no cross-partition scatter. Registry by user_id via consistent hashing so lookups are O(1) and rebalancing is minimal. Group by group_id so the member list is one partition read.

Q8. B has 3 linked devices (phone + 2 companions). How do you deliver and ack a message across all of them efficiently?
> **Expected answer:** Multi-device in Signal is **sender-side fanout of ciphertext**: A maintains a separate Signal session (and thus separate ciphertext) per recipient *device*, plus per sender-device. On SEND, A includes N ciphertext copies, one per B-device. The router looks up all of B's live devices in the registry and delivers each its own copy; offline devices get their copy in *their* mailbox row (mailbox is keyed by device, not just user, in the multi-device model). A message is considered "delivered" (✓✓) when the *first* device acks; "read" tracks per-device but is usually surfaced once. The phone is the primary; companion devices sync via encrypted history handshake at link time.
> **Trap:** Encrypting once and broadcasting the same ciphertext to all devices — breaks Signal's per-session ratchet; each device has a distinct key state, so you need per-device ciphertext. Fanning out plaintext server-side would break E2E entirely.

Q9. Where is eventual consistency acceptable, and where must ordering/consistency be strict?
> **Expected answer:** Eventual/best-effort: **presence and last-seen** (a few seconds stale is fine, and it's ephemeral Redis state), typing indicators (fire-and-forget, no durability), read receipts (can lag). Strict: **per-conversation message order** (enforced by monotonic server_seq — a total order within one chat) and **at-least-once delivery with dedup** (client_msg_id + server_msg_id) so nothing is lost or shown twice. Cross-conversation global order is explicitly *not* maintained — meaningless and expensive. It's an AP system for availability (you can always send; it queues), but the *sequence number* gives per-chat linearizable ordering on drain.
> **Mentor pushback:** Two messages from A to B are sent milliseconds apart over a lossy link; the first times out and is retried *after* the second arrives. Now B could see them out of order. → The server assigns server_seq on first receipt; the retried message carries the same client_msg_id, so the server returns the *original* seq (dedup), preserving order. The client also renders by server_seq, not arrival order.

---

### Low-Level Design (Hard)

Q10. Design group message fanout for a 256-member (and later 1024-member) group with E2E encryption.
> **Problem statement:** One group send must reach up to 1024 members × their devices, in order, encrypted such that the server can't read it, without the sender doing 1024 pairwise encryptions per message.
> **Naive solution:** Sender pairwise-encrypts the message once per recipient device (Signal 1:1 to each) — for a 1024-member group with 1.5 devices avg, that's ~1500 ciphertexts *per message*, and the ratchet cost is O(members) on the sender's phone.
> **Why naive fails at scale:** Sender-side CPU and bandwidth blow up linearly with group size; a chatty 1024-member group makes phones melt and burns mobile data. It also makes the server fanout O(members × devices) deliveries.
> **Expected optimal approach:** **Sender Keys** (Signal group protocol). Each sender generates a symmetric *sender key* (chain key + signature key), distributes it *once* to each member via the existing pairwise Signal channels. Thereafter, the sender encrypts each group message **once** with its sender key and the server fans out that single ciphertext to all members. Members decrypt with the cached sender key and ratchet it forward. Key rotation happens on membership change (a member leaving forces new sender keys so they can't read future messages — "forward secrecy on membership"). Server fanout is still O(members) *deliveries* but only 1 encryption.
> **Pseudo-code or class diagram:**
```
# One-time per (sender, group), or on membership change:
sender_key = generate_sender_key()               # chain_key + signing_key
for member_device in group.member_devices():
    pairwise_channel(member_device).send(SENDER_KEY_DISTRIBUTION, sender_key)

# Per message (steady state):
send_group(group_id, plaintext):
    ct = encrypt(sender_key.chain, plaintext)     # ONE encryption
    sig = sign(sender_key.signing, ct)
    server.fanout(group_id, {ct, sig, sender_key_id})
    sender_key.chain = ratchet(sender_key.chain)  # advance so past keys can't decrypt future

server.fanout(group_id, msg):
    members = group_members(group_id)             # one-partition read
    for m in members:
        for dev in devices(m):
            route_or_enqueue(dev, msg)            # same 1:1 delivery machinery

# On member removal:
rotate_all_sender_keys(group_id)                  # everyone re-distributes; ex-member locked out
```

Q11. Concurrency: a user's phone reconnects to a *different* connection server (roaming/failover) while their old socket is still half-open on the previous server. Two servers think they own the session.
> **Scenario:** Old connection server S1 still shows a live socket for user U; U's phone (new IP) connects to S2 and registers `route:U → S2`. A message for U could be pushed to the stale S1 socket and lost, or delivered twice (once to each server's belief).
> **Expected fix:** The Session Registry write is a **compare-and-set / fencing token**: registration stamps a monotonically increasing `session_epoch`. S2's registration bumps the epoch; the router only delivers to the socket matching the *current* epoch. S1's stale entry has a lower epoch and is ignored; S1 gets a "you've been superseded" signal and closes the socket. Heartbeat TTL on the registry entry also reaps S1's stale route. Delivery to the (now-current) S2 socket; if that fails, fall back to mailbox persist. This is leader-election-per-session (Lesson 17) via fencing tokens.
> **Follow-up:** What if the socket holder (connection server) dies mid-delivery, before the delivered-ack? Message is still in the mailbox (not deleted until ✓✓), so on the next connect it re-drains. At-least-once + client-side dedup by server_msg_id prevents a duplicate render.

Q12. Failure deep-dive: the push-notification path (APNs/FCM) is down, or a poison message repeatedly crashes the mailbox drain for one user.
> **Scenario:** (a) APNs outage means offline recipients never get woken. (b) A malformed/oversized message causes the drain routine to throw every time B connects, blocking *all* of B's queued messages.
> **Expected handling:** (a) Push is only a *wake* optimization, not the delivery guarantee — messages are safely in the durable mailbox regardless. When B eventually opens the app (or a periodic background refresh fires), the mailbox drains. Degrade gracefully: retry push with backoff, and rely on the client's own reconnect. (b) Poison message → the drain must be resilient: process per-message with try/catch, and after N failures move the offending message to a **DLQ** and continue draining the rest (head-of-line blocking is the enemy). Cap message size at ingest (e.g., media goes to blob store, only a pointer is in the message) so an oversized payload never enters the mailbox in the first place. Circuit-break the push gateway (Lesson 10) so its failure doesn't back-pressure the router.

---

### Scaling to 10x / 100x (Hard)

Q13. At 10x users, where does WhatsApp break first?
> **Expected answer:** The **connection tier** (open sockets) and the **routing-registry lookup/update rate**, not message storage (E2E keeps storage tiny). Each online user is a held socket + a registry heartbeat; 10x online users = 10x sockets = 10x connection servers, and the registry takes a heartbeat write per device every ~30s. Secondary: group fanout amplification — a 10x growth in large groups super-linearly increases delivery events.
> **Numbers to ground the answer:** 1B concurrent → 10B concurrent sockets. At 2M/box you go from ~500 to ~5000 connection boxes. Registry heartbeats: 10B devices ÷ 30s = ~330M writes/sec of ephemeral presence/route churn — this is why presence lives in sharded Redis with TTLs, not a durable DB. Message send at 4M/s → 40M/s peak; each group send multiplies into member-count deliveries.

Q14. How do you shard the connection registry and the mailbox store, and where are the hotspots?
> **Expected sharding strategy:** Registry and mailbox both sharded by **user_id via consistent hashing** with virtual nodes — a user's route entry and undelivered queue are deterministic-locatable and co-located regionally. Connection servers are stateless-ish (they hold sockets but the source of truth is the registry), so you add boxes and the ring absorbs new users. Route users to the *geographically nearest* region's edge to cut RTT.
> **Hot spot problem:** A **mega-group** (a 1024-member announcement group with a celebrity poster) creates a fanout hotspot and a hot group-membership partition. Detect via per-group delivery-rate counters. Fix: (1) fanout workers batch and parallelize across members; (2) rate-limit posting in huge groups (WhatsApp restricts frequent forwarding / large-group posting); (3) treat very large "broadcast" groups as a distinct one-to-many channel with pull semantics rather than push to every mailbox. Also a **single user with a runaway device loop** (buggy client reconnecting 100×/sec) is a registry hotspot — rate-limit reconnects per device (Lesson 11).

Q15. Design the caching/ephemeral-state layering and the hardest invalidation problem.
> **Expected layered cache design:** L1 = the connection server's in-memory map of *its own* live sockets (authoritative for locally-connected users). L2 = the sharded Redis Session Registry (global "who is where") + presence. L3 = a short-TTL cache of group membership lists on fanout workers (memberships change rarely relative to message rate). The durable mailbox store is the source of truth for undelivered content. Media is CDN-cached (encrypted blobs are cacheable by media_id since they're immutable ciphertext).
> **Cache invalidation trap:** **Presence and the route table** are the hard part because they change constantly and a stale entry causes *misdelivery*, not just staleness. If `route:U` still points at a dead connection server, messages are pushed into the void. Solution: short TTLs refreshed by heartbeat (self-healing — a dead server stops heartbeating and its routes expire in ~2 TTLs), plus epoch/fencing so a superseded route is ignored even before it expires. Group-membership cache invalidation matters for **security**: a removed member must not receive future messages — so membership change triggers immediate cache purge + sender-key rotation, and fanout re-reads membership on rotation.

Q16. How do you keep this cost-efficient at massive scale?
> **Expected answer:** (1) **E2E means you don't store history** — the single biggest cost lever; server storage is a transient queue, and messages are deleted on delivered-ack. (2) **Delete-on-delivery** keeps the mailbox tiny; only offline-user backlogs consume space. (3) **Media dedup + CDN**: identical forwarded media (encrypted with the same key for a forward chain) can be stored once and pointer-referenced; large media never enters the message path, only a `media_id`. (4) **Efficient runtime** — WhatsApp's Erlang/BEAM choice packs millions of lightweight processes (one per connection) per box, minimizing per-connection memory; this is why they served huge scale on few servers. (5) **Batch registry heartbeats** and use TTL expiry instead of explicit deletes to avoid write amplification. (6) Silent-push coalescing to avoid one APNs/FCM call per message when several arrive close together.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)

**H1.** Explain the **Double Ratchet** algorithm the server is oblivious to: the combination of a Diffie-Hellman ratchet (new DH keypair each message round-trip → forward secrecy + post-compromise "self-healing") and a symmetric-key (KDF) ratchet advancing a chain key per message. Why does this give forward secrecy (stealing today's key can't decrypt yesterday's messages) *and* break-in recovery? And what does it mean for the server's job that message N and N+1 have unrelated-looking ciphertext keys — how do you still deliver in order? (Answer: server orders by server_seq metadata it *can* see; the crypto is opaque to it.)

**H2.** Multi-device history sync under E2E: a user links a new laptop (companion device). It has *none* of the past message history, and the server can't provide plaintext. How do you securely transfer history? (Answer: primary device encrypts a history bundle to the new device's public key over an authenticated channel established via QR-code key exchange; the server relays ciphertext only. Discuss the security boundary and why the QR scan is the trust root.)

**H3.** Deploy a new version of the binary chat protocol / connection server fleet with **zero dropped messages** while millions of sockets are open. (Answer: connection draining — stop accepting new sockets on old boxes, let clients reconnect to new boxes on their own reconnect cadence, ensure mailbox persistence covers the reconnect gap; protocol negotiated by version handshake so old and new clients coexist; never delete a mailbox message until re-acked post-migration.)

**H4.** What do you instrument? Per-conversation **delivery latency** (send→✓✓ p50/p99), connection count per box & socket churn rate, registry heartbeat write rate and staleness, mailbox depth per user (alert on growing backlogs = drain stuck), push-gateway success rate, group-fanout amplification factor, DLQ rate. The one SLO that matters: **online-to-online delivery p99 under 1s**.

**H5.** "Undo a bad decision": you launched with a single global connection region and now cross-continent users see 400ms+ send latency and a US outage takes everyone down. Migrate to multi-region edge without breaking existing sessions. (Answer: introduce regional edges + regional registries with a global directory; route users to nearest edge; make mailbox replication regional with async cross-region for roaming; migrate users gradually as they reconnect; keep a compatibility path so an in-flight message to a user who just moved regions follows them via the global directory.)

---

### Mentor's Closing Notes

**Top 3 things most candidates get wrong on this topic:**
1. Treating it as a storage problem ("where do we store all the messages?") — E2E + delete-on-delivery means the interesting problem is *routing and delivery guarantees*, not storage volume.
2. Forgetting the **write-before-ack** invariant and the three distinct receipts (server-received ✓ / delivered ✓✓ / read blue) — they collapse them into one and lose the durability guarantee.
3. Hand-waving group fanout as "encrypt per member" — missing Sender Keys and the O(members) → O(1) encryption reduction, and missing key rotation on membership change.

**The one insight that makes an answer truly impressive:**
The server is a *blind, best-effort router in front of a durable per-recipient queue* — every hard requirement (offline delivery, ordering, dedup, multi-device, E2E) is a property of **metadata the server can see (seq, msg_id, device, mailbox)** layered *around* a payload it cannot read. Once you separate "the ordered, durable envelope machinery" from "the opaque encrypted content," presence, receipts, fanout, and history-sync all decompose cleanly.

**Suggested follow-up reading:**
- Signal Protocol specs — "The Double Ratchet Algorithm" and "The Sesame/Sender Keys" documents (Marlinspike & Perrin).
- WhatsApp/Erlang scaling talks — "That's 'Billions' with a B: Scaling to the Next Level at WhatsApp" (Rick Reed) and the FreeBSD tuning writeups on 2M+ connections/host.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
