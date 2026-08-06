# System Design Interview Prep — L5 / Senior (Google · Microsoft · Meta)

Every doc follows the same interview-shaped structure:

1. **Problem Statement & Scope** — requirements + back-of-envelope math
2. **Brute-Force Design** — the naive version and exactly where it breaks
3. **Evolving the Design** — bottleneck → fix, narrated like the interview
4. **Protocol & Tech Choices** — why this, why not that, when the alternative wins
5. **HLD** — mermaid architecture diagram, read/write paths, schema, APIs
6. **LLD** — machine-coding-style class diagrams (Strategy/Repository/Factory patterns)
7. **Deep Dives & Failure Modes** — hot keys, thundering herd, idempotency, retries
8. **Trade-off Summary & Interview Soundbites**

## Foundations + Strong Starters

| # | Topic | File |
|---|-------|------|
| 1 | URL Shortener | [foundations/01-url-shortener.md](foundations/01-url-shortener.md) |
| 2 | Rate Limiter | [foundations/02-rate-limiter.md](foundations/02-rate-limiter.md) |
| 3 | Key-Value Store | [foundations/03-key-value-store.md](foundations/03-key-value-store.md) |
| 4 | Consistent Hashing | [foundations/04-consistent-hashing.md](foundations/04-consistent-hashing.md) |
| 5 | SQL vs NoSQL Framework | [foundations/05-sql-vs-nosql.md](foundations/05-sql-vs-nosql.md) |
| 6 | Caching Strategy System | [foundations/06-caching-strategy.md](foundations/06-caching-strategy.md) |
| 7 | Message Queue Architecture | [foundations/07-message-queue.md](foundations/07-message-queue.md) |
| 8 | CDN + Static Asset Delivery | [foundations/08-cdn-static-assets.md](foundations/08-cdn-static-assets.md) |
| 9 | Auth System Design | [foundations/09-auth-system.md](foundations/09-auth-system.md) |
| 10 | File Storage System | [foundations/10-file-storage.md](foundations/10-file-storage.md) |
| 11 | Search Indexing Pipeline | [foundations/11-search-indexing.md](foundations/11-search-indexing.md) |
| 12 | Distributed ID Generation | [foundations/12-distributed-id.md](foundations/12-distributed-id.md) |
| 13 | API Gateway Design | [foundations/13-api-gateway.md](foundations/13-api-gateway.md) |

## Product Thinking Projects

| # | Topic | File |
|---|-------|------|
| 1 | Twitter/X Feed | [product-design/01-twitter-feed.md](product-design/01-twitter-feed.md) |
| 2 | Ride-Sharing System (Uber) | [product-design/02-ride-sharing.md](product-design/02-ride-sharing.md) |
| 3 | Notification Service | [product-design/03-notification-service.md](product-design/03-notification-service.md) |
| 4 | Distributed Job Scheduler | [product-design/04-job-scheduler.md](product-design/04-job-scheduler.md) |
| 5 | Real-time Leaderboard | [product-design/05-leaderboard.md](product-design/05-leaderboard.md) |
| 6 | E-Commerce Checkout | [product-design/06-ecommerce-checkout.md](product-design/06-ecommerce-checkout.md) |
| 7 | Typeahead / Autocomplete | [product-design/07-typeahead.md](product-design/07-typeahead.md) |
| 8 | Multi-Tenant SaaS | [product-design/08-multi-tenant-saas.md](product-design/08-multi-tenant-saas.md) |
| 9 | Event Sourcing + CQRS | [product-design/09-event-sourcing-cqrs.md](product-design/09-event-sourcing-cqrs.md) |
| 10 | Video Streaming Pipeline | [product-design/10-video-streaming.md](product-design/10-video-streaming.md) |
| 11 | Collaborative Editing (OT/CRDT) | [product-design/11-collaborative-editing.md](product-design/11-collaborative-editing.md) |
| 12 | Data Warehouse Ingestion | [product-design/12-data-warehouse-ingestion.md](product-design/12-data-warehouse-ingestion.md) |
| 13 | Feature Flag Service | [product-design/13-feature-flags.md](product-design/13-feature-flags.md) |
| 14 | Observability Stack | [product-design/14-observability.md](product-design/14-observability.md) |

> Diagrams are Mermaid — GitHub renders them natively.
