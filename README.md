# The Atlassian Backend SDE 2 Prep Book

**28 days · 7 hours a day · 196 hours. One self-contained study source.**

This book is the companion text to the `Atlassian SDE2 Study Plan.xlsx` tracker. Every day in the spreadsheet has a matching section here containing the theory you were assigned to learn, the full solution code for what you were assigned to build, worked answers for the DSA drills, and the review material for the evening block. Work the plan top to bottom; open this book when a block starts.

## How to use this book

Each day follows the same rhythm as the tracker:

| Block | Time | What's in the book for it |
|---|---|---|
| Deep Work | 09:00–11:30 | Theory chapter — read actively, take your own notes |
| Build Lab | 12:30–14:30 | Reference solution — attempt first, then compare |
| DSA Drills | 15:00–16:30 | Problems with key idea + full solution |
| Review & Notes | 17:00–18:00 | Flashcards, checklists, retro prompts |

**The one rule that matters:** for every Build Lab and timed exercise, attempt it cold *before* reading the solution. The solutions exist to grade yourself against, not to read first. Copy-typing a reference implementation teaches you almost nothing; failing for 30 minutes and then seeing the fix teaches you everything.

## Contents

- **[Week 1 — Concurrency & Java Foundations](week1-concurrency-foundations.md)** (Days 1–7): memory model, executors, locks, atomics, async composition, safe publication, and a warm-up mock.
- **[Week 2 — Low-Level Design](week2-low-level-design.md)** (Days 8–14): the four canonical Atlassian machine-coding problems — rate limiter, KV store with TTL, pub-sub broker, job scheduler — each with a complete, thread-safe Java solution and the interview narration to go with it.
- **[Week 3 — System Design & the Data Layer](week3-system-design-and-data.md)** (Days 15–21): replication, partitioning, transactions, DynamoDB modeling, caching, Kafka vs SQS, and full walkthroughs of the three classic Atlassian design prompts.
- **[Week 4 — Observability, Mocks & the Values Round](week4-observability-mocks-values.md)** (Days 22–28): SLOs and tracing, resilience patterns, four full mocks with prompts and grading rubrics, STAR stories mapped to Atlassian's values, and 60 rapid-fire flashcards.

## Ground rules for the four weeks

Pick **one language** (this book uses Java; every solution translates to Kotlin directly) and use it for everything. **Record every mock** — audio at minimum — and review the recording the same day; the gap between how you think you narrated and how you actually narrated is where offers are lost. **Ship every deliverable** listed in the tracker to a real repo; by day 27 that repo is your portfolio. And protect the breaks — the 7 hours are focused hours, and they only stay focused if 11:30–12:30 is genuinely off.

## A note on sources

This book is written to stand alone for the 28-day plan. Where a day's tracker row references outside material (*Java Concurrency in Practice*, *Designing Data-Intensive Applications*), the corresponding section here summarizes the ideas you need in the author's own words — deeper reading of those books strengthens week 3 especially, but nothing in the plan requires a source other than this book.
