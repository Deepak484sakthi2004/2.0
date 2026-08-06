# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 25 of 63 — Phase 1: Foundations (Module 25 of 28)
**Module:** Security in Architecture
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
Every design you draw in Phase 2 sits on a public internet where someone is always probing it. A single unauthenticated endpoint, an expired TLS cert, or a missing rate limit is how companies end up on the news — Equifax lost 147M records to one unpatched app-layer bug; Dyn was knocked offline in 2016 by a ~1.2 Tbps DDoS. When an interviewer asks "how do you secure this?", a strong candidate names the exact layer (network vs application), the exact control (firewall, WAF, mTLS), and the exact failure it prevents. Today we build that vocabulary from first principles so security is a design property, not a bolt-on.

## Learning Objectives
By the end of this lesson you can:
- Distinguish a network firewall from a WAF and say exactly which attacks each stops.
- Explain the TLS 1.3 handshake, why it costs ~1 RTT, and what a cert actually proves.
- Draw the line between network-level (L3/L4) and application-level (L7) security.
- Explain Zero Trust / ZTNA and why "inside the network" is no longer trusted.
- Describe layered DoS defense with real numbers (SYN cookies, rate limits, Anycast scrubbing).
- Say what SASE bundles and when converging network + security makes sense.

## The Lesson

### Firewalls
**What it is (plain English):** A firewall is a gatekeeper on network traffic that allows or drops packets based on rules — source/destination IP, port, and protocol. Think of a building lobby guard checking "which door (port) are you headed to, and are you on the list (allowed IP range)?"
**The problem it solves:** Without it, every port on every host is reachable from the entire internet. An exposed database port (5432 Postgres, 3306 MySQL, 6379 Redis) is a direct invitation — unauthenticated Redis instances have been mass-compromised for exactly this reason.
**How it works (mechanics):** Two generations matter. A *stateless* packet filter checks each packet against ACL rules independently. A *stateful* firewall tracks connection state in a table so it can allow return traffic for a connection your host initiated.
```
Rule table (evaluated top-down, first match wins):
  ALLOW tcp any -> 10.0.1.5 : 443     (HTTPS in)
  ALLOW tcp 10.0.0.0/16 -> any : 5432 (DB only from VPC)
  DENY  any any -> any                (default deny)
```
The last rule is the golden principle: **default deny**, then allow the minimum. A stateful table entry looks like `{src:203.0.113.9:51000, dst:10.0.1.5:443, state:ESTABLISHED}` — return packets matching it pass without a separate rule.
**Trade-offs / when NOT to use it:** A firewall operates on IP/port; it cannot see *inside* an HTTPS payload, so it cannot stop SQL injection riding on port 443. State tables also have limits (hundreds of thousands to millions of entries) — a flood can exhaust them.
**Where you'll see it:** AWS Security Groups (stateful) and Network ACLs (stateless), iptables/nftables on Linux, cloud VPC firewalls.

### WAF in Detail
**What it is (plain English):** A Web Application Firewall inspects HTTP/HTTPS *content* — URLs, headers, cookies, body — and blocks requests that look like application attacks. A network firewall guards the door; a WAF reads the letters coming through it.
**The problem it solves:** The OWASP Top 10 — SQL injection, XSS, path traversal, RCE. These travel inside legitimate-looking HTTPS requests on port 443, so a network firewall waves them through. The WAF is the layer that sees `?id=1' OR '1'='1` and drops it.
**How it works (mechanics):** A WAF sits as a reverse proxy in front of your app, terminates or inspects TLS, and matches requests against rules. Two modes:
- *Negative model (blocklist):* signatures/regex for known-bad patterns (e.g., `UNION\s+SELECT`, `<script>`). Managed rule sets like OWASP CRS ship thousands of these.
- *Positive model (allowlist):* only known-good shapes pass (e.g., `id` must be `^\d{1,10}$`).
It also does rate limiting, bot scoring, and geo rules.
```
Request: GET /product?id=1;DROP TABLE users
  WAF rule CRS-942100 (SQLi) → MATCH → 403 Forbidden (logged)
```
**Trade-offs / when NOT to use it:** WAFs are tuned against *false positives* — too aggressive and you block real users; too loose and attacks slip by. They add ~1–5 ms latency and can be bypassed by novel encodings. A WAF is defense-in-depth, not a substitute for parameterized queries and input validation in code.
**Where you'll see it:** AWS WAF, Cloudflare WAF, Akamai Kona, ModSecurity (open source) with the OWASP Core Rule Set.

### TLS
**What it is (plain English):** TLS (Transport Layer Security, the "S" in HTTPS) encrypts data in transit and proves you're talking to the real server, not an impostor. It gives you three things: confidentiality (eavesdroppers see gibberish), integrity (tampering is detected), authentication (the cert proves identity).
**The problem it solves:** On plain HTTP, anyone on the path — coffee-shop Wi-Fi, a rogue ISP — can read your password and inject content. TLS makes the channel opaque and tamper-evident.
**How it works (mechanics):** TLS 1.3 handshake, **1-RTT**:
```
Client                                   Server
  | ClientHello (key_share, ciphers) --->|
  |<-- ServerHello (key_share),          |
  |    {Certificate, Finished}(encrypted)|
  | {Finished} ------------------------->|
  |====== application data (AES-GCM) ====|
```
Both sides run **ECDHE** (Elliptic Curve Diffie-Hellman Ephemeral) on the exchanged `key_share` values to derive the same shared secret without ever sending it — that's *forward secrecy*: stealing the server's long-term key later can't decrypt old traffic. The certificate, signed by a CA (chain to a root your OS trusts), binds the domain to the server's public key. TLS 1.3 cut the handshake from 2 RTTs (1.2) to 1, and 0-RTT resumption exists for repeat visits.
**Trade-offs / when NOT to use it:** Handshake CPU + one extra RTT (~20–100 ms on first connect depending on distance). 0-RTT data is replayable, so don't use it for non-idempotent requests. Certs expire — an expired cert is a self-inflicted outage (it has taken down major services).
**Where you'll see it:** Every HTTPS site, gRPC, database TLS, and service mesh mTLS (Istio, Linkerd) where *both* sides present certs.

### Network-level vs Application-level Security
**What it is (plain English):** Two different altitudes. Network-level security operates on packets and connections (L3/L4: IPs, ports, TCP state). Application-level security operates on the meaning of requests (L7: users, roles, HTTP semantics, business rules).
**The problem it solves:** Each layer is blind to the other. A firewall can't tell an admin API call from a public one (both are TCP:443); your app code can't cheaply drop a 1 Tbps packet flood before it arrives. You need controls at both.
**How it works (mechanics):**
```
L3/L4 (network): firewall, IP allowlist, SYN-flood protection, TLS transport
      ▲ sees: 203.0.113.9 -> 10.0.1.5 : 443, TCP flags
L7 (application): authN (who), authZ (allowed?), WAF, rate-limit per user,
                  input validation, output encoding
      ▲ sees: POST /transfer {from:A, to:B, amt:5000}, JWT for user 42
```
Concretely: the network layer decides *whether the packet may arrive*; the application layer decides *whether this user may do this action*. A payment API needs both — a firewall limiting source ranges **and** authorization checking that user 42 owns account A.
**Trade-offs / when NOT to use it:** Relying only on network controls is the old "hard shell, soft center" mistake — once inside, an attacker roams freely. Relying only on app controls leaves you exposed to volumetric floods. Defense-in-depth means both, plus data-level (encryption at rest, row-level access).
**Where you'll see it:** Any cloud stack: Security Groups (L4) + API gateway authN/authZ + WAF (L7). This split is the backbone of the Zero Trust model next.

### Zero Trust (ZTNA)
**What it is (plain English):** Zero Trust says "never trust, always verify" — no request is trusted just because it came from inside the corporate network. Every request re-proves identity and authorization. The old model trusted anything past the firewall; Zero Trust treats the internal network as hostile too.
**The problem it solves:** The classic breach pattern: attacker phishes one laptop, lands *inside* the VPN, and then moves laterally to unprotected internal services because "inside = trusted." Zero Trust removes that free lateral movement.
**How it works (mechanics):** ZTNA (Zero Trust Network Access) brokers every connection through a policy engine that checks identity + device posture + context *per request*:
```
User+device -> Identity Provider (verify MFA, device compliance)
            -> Policy Engine: "user 42, role=analyst, device=managed,
               geo=IN, resource=reports-api → ALLOW (least privilege)"
            -> connect ONLY to that one service (no network-wide access)
```
Pillars: strong identity (SSO + MFA), least-privilege per-resource access, micro-segmentation (services can't freely reach each other), and continuous verification (a session can be re-evaluated, not trusted for hours). Google's **BeyondCorp** is the reference implementation — employees access apps over the internet with no traditional VPN, authorized per request.
**Trade-offs / when NOT to use it:** Real operational cost — you need solid identity infrastructure, device management, and policy tooling; done poorly it adds latency and friction to every call. It's a multi-year journey, not a product you install.
**Where you'll see it:** Google BeyondCorp, Cloudflare Access, Zscaler Private Access, service mesh mTLS enforcing per-service identity.

### Endpoint Security
**What it is (plain English):** Endpoints are the devices that touch your system — laptops, servers, containers, phones. Endpoint security hardens and monitors them, because a compromised endpoint is often the attacker's first foothold. In Zero Trust, the device's health is part of the access decision.
**The problem it solves:** Credentials and keys live on endpoints. Phishing, malware, or an unpatched OS turns a trusted device into an attacker's remote hands *inside* your trust boundary. Firewalls and WAFs don't help once the attacker is operating as a legitimate logged-in user.
**How it works (mechanics):** Layered controls on each device:
```
Device posture check → disk encryption on? OS patched? EDR agent healthy?
EDR (Endpoint Detection & Response): watches process/behavior,
    e.g., "powershell spawned by Word → download → encrypt files"
    = ransomware pattern → isolate host, alert SOC
Least privilege: no local admin; app allowlisting
```
EDR uses behavioral detection, not just signatures — it correlates process trees and can auto-isolate a machine from the network in seconds. For servers/containers: minimal base images, no shell, read-only filesystems, and CVE scanning in CI (a container with a known-critical CVE fails the build).
**Trade-offs / when NOT to use it:** EDR agents consume CPU/RAM and can generate alert fatigue; overly strict allowlisting frustrates developers. Endpoint security reduces but never eliminates risk — assume breach and layer with network + app controls.
**Where you'll see it:** CrowdStrike Falcon, Microsoft Defender for Endpoint, SentinelOne; container scanning via Trivy/Snyk in CI/CD.

### Preventing DoS Attacks
**What it is (plain English):** A Denial-of-Service attack tries to exhaust a resource — bandwidth, connections, CPU — so real users can't get through. DDoS is the same, but from many machines (a botnet) at once. Defense is layered: absorb the volume, filter the junk, and rate-limit what's left.
**The problem it solves:** One attacker with a botnet can generate traffic no single server survives. In 2016, the Mirai botnet hit Dyn (DNS) at ~1.2 Tbps and took down Twitter, Reddit, and Netflix indirectly. GitHub weathered a 1.35 Tbps memcached-amplification attack in 2018 — survived because it had scrubbing in place.
**How it works (mechanics):** Match the defense to the attack layer:
```
Volumetric (L3/L4, e.g. UDP flood, amplification):
   → Anycast spreads load across many PoPs; scrubbing centers
     drop bad packets; provider absorbs Tbps you can't.
Protocol (SYN flood — half-open TCP connections exhaust the table):
   → SYN cookies: server encodes state in the SYN-ACK seq number,
     allocates NO memory until the final ACK proves a real client.
Application (L7, e.g. flood of expensive /search requests):
   → rate limit (token bucket: 100 req/min/IP), CAPTCHA,
     WAF bot scoring, caching so cheap requests never hit origin.
```
A token bucket: capacity 100, refill 100/60s ≈ 1.67 tokens/sec; each request costs 1 token; empty bucket → 429 Too Many Requests.
**Trade-offs / when NOT to use it:** Aggressive rate limits and CAPTCHAs hurt legitimate users (a shared corporate NAT IP can trip per-IP limits). Scrubbing services add cost and a little latency. You can't self-host your way out of a Tbps flood — you need Anycast + a provider.
**Where you'll see it:** Cloudflare, AWS Shield (Standard/Advanced), Akamai Prolexic, Google Cloud Armor.

### SASE
**What it is (plain English):** SASE (Secure Access Service Edge, say "sassy") converges networking (SD-WAN) and security (SWG, CASB, ZTNA, FWaaS) into one cloud-delivered service at the network edge, close to users. Instead of backhauling all traffic to a corporate data center for inspection, the security lives in the cloud PoP nearest the user.
**The problem it solves:** In a remote-work, SaaS-everywhere world, the old model — VPN everything back to HQ, inspect there, then out to the internet — adds huge latency and a central bottleneck. SASE inspects and enforces policy at an edge PoP milliseconds from the user.
**How it works (mechanics):**
```
User (anywhere) → nearest SASE PoP (one pass, identity-driven):
    SWG (web filtering) + CASB (SaaS/data controls) +
    ZTNA (per-app access) + FWaaS + DLP
  → then direct to SaaS app / internet / private app
Single policy: "user 42, analyst, managed device → allow Salesforce,
   block risky uploads (DLP), no access to finance app"
```
The win is one identity-aware policy enforced globally at ~one PoP hop, versus stitching five separate appliances and backhauling.
**Trade-offs / when NOT to use it:** You concentrate trust in one vendor's cloud (a single point of policy failure) and depend on their PoP coverage near your users. Migration from legacy appliances is a big project. For a small, single-site org, it can be overkill.
**Where you'll see it:** Zscaler, Palo Alto Prisma Access, Cloudflare One, Netskope, Cisco+ SASE.

## Comparison Table

| Dimension | Firewall (L3/L4) | WAF (L7) | Zero Trust / ZTNA |
|---|---|---|---|
| Operates on | IP, port, TCP state | HTTP content (URL, body, headers) | Identity + device + context per request |
| Stops | Unauthorized ports, IP ranges | SQLi, XSS, OWASP Top 10 | Lateral movement, over-trust of "inside" |
| Blind to | Payload inside HTTPS | Volumetric floods, network scans | Nothing by design (verifies each request) |
| Latency added | ~sub-ms | ~1–5 ms | ~ms (policy check) + infra cost |
| Example | AWS Security Group | Cloudflare WAF | Google BeyondCorp |

**Verdict:** These are complementary layers, not alternatives — a firewall decides if the packet arrives, a WAF decides if the request is malicious, and Zero Trust decides if this user/device may do this action. Real systems run all three.

## Common Misconceptions
- **Myth:** A firewall protects against SQL injection. → **Reality:** Firewalls work on IP/port and can't see inside an encrypted HTTPS payload; you need a WAF plus parameterized queries.
- **Myth:** HTTPS means my app is secure. → **Reality:** TLS only secures data *in transit*. It does nothing against injection, broken authorization, or an expired cert taking you offline.
- **Myth:** Once traffic is inside the VPN/network, it's trusted. → **Reality:** That "hard shell, soft center" model is exactly what Zero Trust abolishes; assume the internal network is hostile.
- **Myth:** A WAF replaces secure coding. → **Reality:** A WAF is defense-in-depth; novel encodings bypass it. Fix the vulnerability in code too.
- **Myth:** You can absorb any DDoS by adding servers. → **Reality:** A 1+ Tbps volumetric flood saturates your uplink before servers matter; you need Anycast + scrubbing.

## Real-World Case
In 2017, Equifax was breached and ~147M consumer records were exfiltrated. The root cause was mundane and application-level: an unpatched vulnerability (CVE-2017-5638) in Apache Struts, a web framework, that allowed remote code execution via a crafted `Content-Type` header. No amount of network firewalling helped — the malicious request rode ordinary HTTPS on port 443 to a legitimate endpoint. A WAF rule or, better, timely patching would have stopped it; the fix had been available months earlier. Compounding it, once inside, attackers moved laterally for 76 days undetected — a textbook argument for Zero Trust segmentation and endpoint monitoring. The lesson every architect should internalize: most catastrophic breaches are application-layer and patch-hygiene failures, not exotic network attacks.

## Self-Test (answers at the bottom)
1. Name one attack a network firewall stops and one it cannot, and say why.
2. In the TLS 1.3 handshake, what does ECDHE give you that RSA key exchange did not, and how many round trips does the handshake take?
3. Your app is fine against volumetric floods but keeps getting SQL-injection attempts through HTTPS. Which control do you add, and at which layer?
4. Explain why "trusted because it's on the corporate VPN" is dangerous, and how ZTNA changes it.
5. Design sketch: A public payment API expects 5,000 QPS legitimately but is being hit by a 200 Gbps flood plus L7 request floods and injection attempts. Lay out a layered defense (network → transport → application) with at least one concrete number per layer.

## Interview Soundbites
- "A firewall decides if the packet may arrive; a WAF decides if the request is malicious; authorization decides if this user may do this action — three different layers, all required."
- "TLS 1.3 is one round trip and gives forward secrecy via ECDHE, so stealing the server key later can't decrypt yesterday's traffic."
- "Zero Trust means the network is always hostile: every request re-proves identity, device health, and least-privilege authorization — no free lateral movement."

## Mini-Assignment
Pick any web system you've used (say, an online banking login). On paper (~30 min), draw its request path from browser to database and annotate every security control at each hop: DNS/Anycast → DDoS scrubbing → WAF → TLS termination → API gateway (authN/authZ + rate limit) → app (input validation) → network firewall/Security Group → DB (encryption at rest, least-privilege user). For each control, write the one attack it stops. Then circle the single layer whose failure would be most catastrophic and justify why.

## Recap & Tomorrow
- **Firewalls:** L3/L4 gatekeeping on IP/port with default-deny; can't see inside HTTPS.
- **WAF:** L7 inspection of HTTP content; stops OWASP Top 10 like SQLi/XSS.
- **TLS:** 1-RTT handshake, ECDHE forward secrecy, confidentiality + integrity + authentication.
- **Network vs application security:** packets vs meaning — you need both, defense-in-depth.
- **Zero Trust/ZTNA:** never trust, always verify; per-request identity and least privilege.
- **Endpoint security:** harden and monitor devices; EDR detects behavior, not just signatures.
- **DoS prevention:** layered — Anycast/scrubbing (volumetric), SYN cookies (protocol), rate limits (L7).
- **SASE:** cloud-edge convergence of SD-WAN + ZTNA + SWG + CASB under one identity policy.

Tomorrow, **Lesson 26 — The Great Trade-off Review**: a rapid-fire consolidation of every major architectural fork you've met — consistency, load balancing, replication, VMs vs containers — each distilled into a crisp decision rule.

## Self-Test Answers
1. Stops: an attempt to connect to an exposed database port (e.g., blocking TCP:6379 Redis from the public internet) — it filters on IP/port. Cannot stop: SQL injection over HTTPS, because that rides port 443 as legitimate-looking traffic and the payload is encrypted/inside the app layer, invisible to an L3/L4 filter.
2. ECDHE gives *forward secrecy*: the session key is derived from ephemeral Diffie-Hellman key shares that are never transmitted, so compromising the server's long-term private key later cannot decrypt previously captured sessions (classic RSA key exchange could). TLS 1.3 completes in 1 round trip (0-RTT for resumption).
3. Add a WAF at the application layer (L7). It inspects HTTP content — terminating/reading TLS — and matches request bodies/params against SQLi signatures (e.g., OWASP CRS), returning 403 on patterns like `' OR '1'='1`. Pair it with parameterized queries in code.
4. If "on the VPN" equals trusted, one phished laptop lands an attacker inside the trust boundary and lets them move laterally to unprotected internal services (as in the Equifax dwell time). ZTNA removes implicit trust: every request re-verifies identity + device posture and grants access to only the one authorized resource (least privilege, micro-segmentation), so a foothold doesn't become network-wide access.
5. Layered: (Network) Anycast + provider scrubbing (AWS Shield/Cloudflare) to absorb the 200 Gbps flood across many PoPs — you cannot self-host this. (Transport) TLS 1.3 termination at the edge; SYN cookies to survive SYN floods without exhausting the connection table. (Application) API gateway with per-user token-bucket rate limiting (e.g., 100 req/min/user, hard cap well above the 5,000 QPS legit baseline but below abuse levels), WAF with OWASP CRS for injection, bot scoring/CAPTCHA on suspicious clients, plus authN (JWT/MFA) and authorization that user X owns account Y, and parameterized queries at the DB with a least-privilege DB user.
