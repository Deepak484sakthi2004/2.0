# Summary

[Preface: How to Read This Book](preface.md)

# Part I — Why Rust Exists

- [Part I Overview](part-01-why-rust/README.md)
  - [1.1 The Problem: Who Frees This Memory?](part-01-why-rust/ch01-the-problem.md)
  - [1.2 Five Languages, Five Bets](part-01-why-rust/ch02-five-languages.md)
  - [1.3 Rust's Bet](part-01-why-rust/ch03-rusts-bet.md)
  - [1.4 The Honest Cost](part-01-why-rust/ch04-honest-cost.md)
  - [Part I Review: Architecture Review & Interview Mode](part-01-why-rust/review.md)

# Part II — Rust From First Principles

- [Part II Overview](part-02-first-principles/README.md)
  - [2.1 The Toolchain: rustup, rustc, cargo, and What a Crate Is](part-02-first-principles/ch01-toolchain.md)
  - [2.2 Cargo.toml, Dependencies, Features, and Build Profiles](part-02-first-principles/ch02-cargo-profiles.md)
  - [2.3 Bindings, Mutability, and Scalar Types: Does `x` Exist at Runtime?](part-02-first-principles/ch03-bindings-scalars.md)
  - [2.4 Compound Types, Functions, Expressions, and Statements](part-02-first-principles/ch04-compound-functions.md)
  - [2.5 Control Flow and Pattern Matching](part-02-first-principles/ch05-control-flow-patterns.md)
  - [2.6 Structs, Enums, and Methods](part-02-first-principles/ch06-structs-enums-methods.md)
  - [2.7 Modules, Visibility, and Crate Architecture](part-02-first-principles/ch07-modules-visibility.md)
  - [Project Level 1: `logstat`, a Production-Grade CLI Tool](part-02-first-principles/project-01-logstat.md)
  - [Part II Review: Architecture Review & Interview Mode](part-02-first-principles/review.md)

# Part III — Ownership: The Core of Rust

- [Part III Overview](part-03-ownership/README.md)
  - [3.1 Ownership: One Owner, One Drop](part-03-ownership/ch01-ownership.md)
  - [3.2 Move, Copy, and Clone](part-03-ownership/ch02-move-copy-clone.md)
  - [3.3 Borrowing: Shared and Mutable References](part-03-ownership/ch03-borrowing.md)
  - [3.4 Slices, String, and &str](part-03-ownership/ch04-slices-strings.md)
  - [3.5 Drop, RAII, and Deterministic Destruction](part-03-ownership/ch05-drop-raii.md)
  - [3.6 Ownership for Graphs: Rc, Weak, Arenas, and Indices](part-03-ownership/ch06-graphs.md)
  - [Project Level 2: `redact`, a Streaming Secret-Masking Filter](part-03-ownership/project-02-redact.md)
  - [Part III Review: Architecture Review & Interview Mode](part-03-ownership/review.md)

# Part IV — The Borrow Checker

- [Part IV Overview](part-04-borrow-checker/README.md)
  - [4.1 Aliasing XOR Mutation](part-04-borrow-checker/ch01-aliasing-xor-mutation.md)
  - [4.2 Non-Lexical Lifetimes, Liveness, and Reborrowing](part-04-borrow-checker/ch02-nll-reborrowing.md)
  - [4.3 Lifetime Annotations and Elision](part-04-borrow-checker/ch03-lifetime-annotations.md)
  - [4.4 Variance and Subtyping](part-04-borrow-checker/ch04-variance.md)
  - [4.5 Higher-Ranked Trait Bounds](part-04-borrow-checker/ch05-hrtb.md)
  - [4.6 Reading Borrow-Checker Errors as Ownership Proofs](part-04-borrow-checker/ch06-reading-errors.md)
  - [Part IV Review: Borrow-Error Triage & Interview Mode](part-04-borrow-checker/review.md)

# Part V — Types as Architecture

- [Part V Overview](part-05-types-architecture/README.md)
  - [5.1 Algebraic Data Types: Structs, Enums, Option, Result](part-05-types-architecture/ch01-algebraic-data-types.md)
  - [5.2 Type Layout: Size, Alignment, Padding, and Niches](part-05-types-architecture/ch02-type-layout.md)
  - [5.3 Newtypes, Zero-Sized Types, PhantomData, and Markers](part-05-types-architecture/ch03-newtypes-zst-phantom.md)
  - [5.4 The Type-State Pattern](part-05-types-architecture/ch04-type-state.md)
  - [Part V Review: Type-Driven Design Capstone & Interview Mode](part-05-types-architecture/review.md)

# Part VI — Traits

- [Part VI Overview](part-06-traits/README.md)
  - [6.1 Traits, Bounds, and Default Methods](part-06-traits/ch01-traits-bounds.md)
  - [6.2 Associated Types, Generic Traits, and Blanket Impls](part-06-traits/ch02-associated-generic-blanket.md)
  - [6.3 Coherence and the Orphan Rule](part-06-traits/ch03-coherence-orphan.md)
  - [6.4 Trait Objects, vtables, and dyn Compatibility](part-06-traits/ch04-trait-objects.md)
  - [6.5 Static vs Dynamic Dispatch: The Architect's Decision](part-06-traits/ch05-static-vs-dynamic.md)
  - [Part VI Review: Trait-Design Review & Interview Mode](part-06-traits/review.md)

# Part VII — Generics and Monomorphization

- [Part VII Overview](part-07-generics/README.md)
  - [7.1 Generics from Call Site to Binary](part-07-generics/ch01-generics-call-site-to-binary.md)
  - [7.2 Monomorphization vs Java Type Erasure](part-07-generics/ch02-monomorphization-vs-erasure.md)
  - [7.3 Code Bloat, Compile Time, and How to Control Them](part-07-generics/ch03-code-bloat-compile-time.md)
  - [Part VII Review: Instance Accounting & Interview Mode](part-07-generics/review.md)

# Part VIII — Error Handling

- [Part VIII Overview](part-08-error-handling/README.md)
  - [8.1 Result, Option, and the ? Operator](part-08-error-handling/ch01-result-option.md)
  - [8.2 Designing Error Types: Libraries vs Applications](part-08-error-handling/ch02-error-types.md)
  - [8.3 Panics, Unwinding, and Abort](part-08-error-handling/ch03-panics.md)
  - [8.4 Errors at Service Boundaries](part-08-error-handling/ch04-service-boundaries.md)
  - [Part VIII Review: The Refunds PR & Interview Mode](part-08-error-handling/review.md)

# Part IX — Collections and Memory

- [Part IX Overview](part-09-collections/README.md)
  - [9.1 Vec<T>: Pointer, Length, Capacity](part-09-collections/ch01-vec.md)
  - [9.2 String and Text Encoding](part-09-collections/ch02-string.md)
  - [9.3 HashMap and HashSet](part-09-collections/ch03-hashmap.md)
  - [9.4 VecDeque, BTreeMap, BinaryHeap — and Why Not LinkedList](part-09-collections/ch04-deque-btree-heap.md)
  - [9.5 Choosing Collections by Cache Behavior](part-09-collections/ch05-cache-behavior.md)
  - [Interlude: The Trade-off Engine — BFS vs DFS, Down to the Stack Page](part-09-collections/interlude-bfs-dfs.md)
  - [Part IX Review: A Collections Design Review & Interview Mode](part-09-collections/review.md)

# Part X — Closures, Iterators, and Zero-Cost Abstractions

- [Part X Overview](part-10-iterators/README.md)
  - [10.1 Closures: Fn, FnMut, FnOnce, and Capture](part-10-iterators/ch01-closures.md)
  - [10.2 The Iterator Trait and Laziness](part-10-iterators/ch02-iterator-trait.md)
  - [10.3 How an Iterator Chain Compiles](part-10-iterators/ch03-chain-compiles.md)
  - [10.4 Rust Iterators vs Java Streams](part-10-iterators/ch04-iterators-vs-streams.md)
  - [Part X Review: The Settlement Report PR & Interview Mode](part-10-iterators/review.md)

# Part XI — Concurrency

- [Part XI Overview]()
  - [11.1 Threads and the OS]()
  - [11.2 Send and Sync]()
  - [11.3 Arc, Mutex, RwLock, and Condvar]()
  - [11.4 Interior Mutability: Cell, RefCell, OnceCell, UnsafeCell]()
  - [11.5 Channels and Message Passing]()
  - [11.6 Scoped Threads and Data Parallelism with Rayon]()
  - [11.7 The Concurrency Decision Matrix]()
  - [Project Level 3: A Multithreaded HTTP Server from Raw TCP]()
  - [Project Level 4: A Concurrent Key-Value Store (Ferrite v1)]()
  - [Part XI Review]()

# Part XII — Async Rust

- [Part XII Overview]()
  - [12.1 Why Async Exists: From C10K to C10M]()
  - [12.2 The Future Trait and poll]()
  - [12.3 async fn Becomes a State Machine]()
  - [12.4 Pin and Self-Referential Futures]()
  - [12.5 Wakers and Executors: Build One from Scratch]()
  - [12.6 OS Threads vs Green Threads vs Async Tasks]()
  - [Part XII Review]()

# Part XIII — Tokio and Production Async

- [Part XIII Overview]()
  - [13.1 Tokio's Architecture: Scheduler, I/O Driver, Timers]()
  - [13.2 Tasks, Spawning, and Blocking Code]()
  - [13.3 Async Channels and Synchronization]()
  - [13.4 Cancellation and Structured Concurrency]()
  - [13.5 Backpressure, Timeouts, and Load Shedding]()
  - [Project Level 5: An Async TCP Server (Ferrite v2)]()
  - [Part XIII Review]()

# Part XIV — Memory Model and Atomics

- [Part XIV Overview](part-14-memory-model/README.md)
  - [14.1 Why Memory Ordering Exists: Store Buffers and Reordering](part-14-memory-model/ch01-why-ordering.md)
  - [14.2 Happens-Before](part-14-memory-model/ch02-happens-before.md)
  - [14.3 Relaxed, Acquire/Release, and SeqCst](part-14-memory-model/ch03-orderings.md)
  - [14.4 Compare-and-Swap, Fences, and Lock-Free Building Blocks](part-14-memory-model/ch04-cas-fences-lock-free.md)
  - [14.5 Rust Atomics vs Java volatile and VarHandle](part-14-memory-model/ch05-rust-vs-java.md)
  - [Part XIV Review: The SPSC Ring PR & Interview Mode](part-14-memory-model/review.md)

# Part XV — Unsafe Rust

- [Part XV Overview]()
  - [15.1 What unsafe Means: Soundness and Invariants]()
  - [15.2 Raw Pointers, Provenance, and Aliasing Models]()
  - [15.3 MaybeUninit, ManuallyDrop, and UnsafeCell]()
  - [15.4 Building a Safe Abstraction: Implementing Vec<T>]()
  - [15.5 Custom Allocators]()
  - [15.6 Verifying unsafe: Miri, Sanitizers, and Loom]()
  - [Part XV Review]()

# Part XVI — FFI and Systems Programming

- [Part XVI Overview]()
  - [16.1 ABIs, Calling Conventions, and repr(C)]()
  - [16.2 Calling C from Rust]()
  - [16.3 Calling Rust from C (and from Java via FFM)]()
  - [16.4 Ownership and Allocation Across Language Boundaries]()
  - [Part XVI Review]()

# Part XVII — Compilers

- [Part XVII Overview]()
  - [17.1 The Compiler Pipeline End to End]()
  - [17.2 Lexing]()
  - [17.3 Parsing: Recursive Descent and Pratt Parsing]()
  - [17.4 ASTs, Symbol Tables, and Name Resolution]()
  - [17.5 Type Checking and Type Inference]()
  - [17.6 Intermediate Representations, CFGs, and SSA]()
  - [17.7 Optimization]()
  - [17.8 Code Generation and Register Allocation]()
  - [Part XVII Review]()

# Part XVIII — How rustc Works

- [Part XVIII Overview]()
  - [18.1 rustc's Architecture: Queries and Incremental Compilation]()
  - [18.2 Macro Expansion, Name Resolution, and HIR Lowering]()
  - [18.3 Type Checking and Trait Solving]()
  - [18.4 THIR and MIR]()
  - [18.5 Borrow Checking on MIR]()
  - [18.6 Monomorphization and Codegen Backends (LLVM, Cranelift)]()
  - [18.7 Tracing `let x = foo();` Through the Compiler]()
  - [Part XVIII Review]()

# Part XIX — Binary, Linker, and OS

- [Part XIX Overview]()
  - [19.1 Object Files, Symbols, and Relocations]()
  - [19.2 Static and Dynamic Linking]()
  - [19.3 Executable Formats: ELF, PE, Mach-O]()
  - [19.4 From Executable to Process]()
  - [19.5 Virtual Memory, Pages, and mmap]()
  - [19.6 Syscalls, File Descriptors, Processes, and Threads]()
  - [Part XIX Review]()

# Part XX — Performance Engineering

- [Part XX Overview]()
  - [20.1 Performance as a Measurement Discipline]()
  - [20.2 Benchmarking Without Lying to Yourself]()
  - [20.3 CPU Profiling and Flame Graphs]()
  - [20.4 Allocation and Memory Profiling]()
  - [20.5 Caches, Branch Prediction, and False Sharing]()
  - [20.6 SIMD and Vectorization]()
  - [20.7 Contention, NUMA, and Tail Latency]()
  - [Part XX Review]()

# Part XXI — Networking

- [Part XXI Overview]()
  - [21.1 Sockets, TCP, and UDP]()
  - [21.2 HTTP/1.1, HTTP/2, and HTTP/3]()
  - [21.3 TLS]()
  - [21.4 Timeouts, Retries, and Circuit Breakers]()
  - [21.5 Connection Pooling and Load Balancing]()
  - [21.6 Building Services with Hyper, Axum, and Serde]()
  - [Part XXI Review]()

# Part XXII — The Rust Backend Ecosystem

- [Part XXII Overview]()
  - [22.1 Choosing Crates as an Architect]()
  - [22.2 Serde]()
  - [22.3 Tower, Hyper, Axum, and Actix Web]()
  - [22.4 Databases: SQLx and Diesel]()
  - [22.5 Observability with tracing]()
  - [22.6 clap, reqwest, and Rayon]()
  - [22.7 Testing: Unit, Integration, Property, Fuzz, and Concurrency Tests]()
  - [Project Level 6: A Database Client/Server Protocol]()
  - [Part XXII Review]()

# Part XXIII — Databases and Storage

- [Part XXIII Overview]()
  - [23.1 Files, fsync, and mmap]()
  - [23.2 Write-Ahead Logging]()
  - [23.3 B-Trees]()
  - [23.4 LSM Trees and Compaction]()
  - [23.5 Serialization Formats and Zero-Copy]()
  - [23.6 Concurrency Control and Transactions]()
  - [Project Level 7: A Persistent Storage Engine (Ferrite v3)]()
  - [Part XXIII Review]()

# Part XXIV — Distributed Systems in Rust

- [Part XXIV Overview]()
  - [24.1 Failure Models and Time]()
  - [24.2 From TCP Server to Replicated Store]()
  - [24.3 Leader/Follower Replication]()
  - [24.4 Consensus: Raft]()
  - [24.5 A Distributed Rate Limiter]()
  - [Project Level 8: A Distributed Cache]()
  - [Project Level 9: A Message Broker]()
  - [Project Level 10: A Mini Database (Ferrite v4)]()
  - [Project Level 11: A Mini Distributed Database (Ferrite v5)]()
  - [Part XXIV Review]()

# Part XXV — Blockchain and Infrastructure

- [Part XXV Overview]()
  - [25.1 Rust in Node Infrastructure: Networking, Storage, Execution]()
  - [25.2 Cryptographic Engineering in Rust]()
  - [25.3 Consensus Implementations]()
  - [25.4 WASM Execution Environments]()
  - [Part XXV Review]()

# Part XXVI — Building a Language

- [Part XXVI Overview]()
  - [26.1 Stage 1: Lexer, Parser, AST, Interpreter]()
  - [26.2 Stage 2: Variables, Functions, Types]()
  - [26.3 Stage 3: Structs, Enums, Pattern Matching]()
  - [26.4 Stage 4: Ownership, Borrowing, Lifetimes]()
  - [26.5 Stage 5: Type Checker and Borrow Checker]()
  - [26.6 Stage 6: A MIR-like IR]()
  - [26.7 Stage 7: LLVM Code Generation]()
  - [Project Level 12: Toy Programming Language]()
  - [Project Level 13: Toy Compiler]()
  - [Final Capstone: Design a Systems Language]()

# Appendices

- [A. Answer Keys]()
  - [Part I Answers](appendix/answers-part-01.md)
  - [Part II Answers](appendix/answers-part-02.md)
  - [Part III Answers](appendix/answers-part-03.md)
  - [Part IV Answers](appendix/answers-part-04.md)
  - [Part V Answers](appendix/answers-part-05.md)
  - [Part VI Answers](appendix/answers-part-06.md)
  - [Part VII Answers](appendix/answers-part-07.md)
  - [Part VIII Answers](appendix/answers-part-08.md)
  - [Part IX Answers](appendix/answers-part-09.md)
  - [Part X Answers](appendix/answers-part-10.md)
  - [Part XIV Answers](appendix/answers-part-14.md)
- [B. Glossary]()
- [C. Sources and Further Reading]()
