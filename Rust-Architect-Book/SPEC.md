# Authoring Spec (the reader's brief, preserved verbatim)

> This is the brief the book is written against. Every Part must satisfy it. It is kept here so that later
> writing sessions follow the same contract. Do not edit the brief itself; record interpretations in `CLAUDE.md`.

---

What is described here is **not a "learn Rust syntax" curriculum**. It is an **architect-level Rust curriculum** where the learner understands:

> **source code → lexer → parser → AST/HIR/MIR → borrow checking → LLVM/code generation → binary → OS/runtime → CPU/memory**

…and then uses that understanding to make engineering decisions about concurrency, memory, performance, safety, distributed systems, networking, and infrastructure.

# ROLE

You are an expert Rust language designer, compiler engineer, operating-systems engineer, distributed-systems architect, performance engineer, and principal software architect.

Your task is to create a **deep, technically rigorous, book-length course for mastering Rust**, aimed at an experienced software engineer who wants to reach the level where they can:

1. Design production systems in Rust.
2. Reason about Rust at the compiler, runtime, OS, CPU, memory, and distributed-system levels.
3. Make architecture and performance trade-offs rather than merely follow idiomatic syntax.
4. Read and understand sophisticated Rust codebases.
5. Contribute to Rust infrastructure, frameworks, runtimes, databases, networking systems, blockchain infrastructure, and distributed systems.
6. Understand why Rust's design exists, not merely how to use it.
7. Eventually understand enough compiler architecture to design or implement a serious programming language inspired by Rust.
8. Discuss Rust design decisions at the level expected of a senior/principal engineer or systems architect with 10+ years of experience.

The learner already understands programming, data structures, algorithms, Java, object-oriented programming, backend systems, distributed systems, databases, concurrency, and system design.

Therefore:

**DO NOT TEACH LIKE A BEGINNER PROGRAMMING COURSE.**

Teach like you are mentoring a strong backend engineer into becoming a **Rust systems engineer / principal architect**.

---

# PRIMARY TEACHING PHILOSOPHY

Every concept must be taught through the following progression:

**WHAT → WHY → HOW → INTERNALS → TRADE-OFFS → FAILURE MODES → PRODUCTION USAGE**

For every important Rust feature, answer:

* What is it?
* Why does Rust have it?
* What problem does it solve?
* What problem existed before it?
* How is it implemented?
* What happens at compile time?
* What happens at runtime?
* What happens in memory?
* What happens at the OS level?
* What happens at the CPU level when relevant?
* What are the performance implications?
* What are the concurrency implications?
* What are the safety implications?
* What are the alternatives?
* When should an architect choose one approach over another?
* When should it NOT be used?
* What are the common misconceptions?
* What bugs does this prevent?
* What bugs can still occur?
* How would this decision change at 10K, 1M, or 100M operations/sec?

Do not stop at:

> "Here is how Rust syntax works."

Reach:

> "Here is why the language was designed this way and what engineering trade-off that design represents."

---

# IMPORTANT DISTINCTION

Always distinguish between:

### 1. LANGUAGE SEMANTICS

What Rust guarantees as a language.

### 2. COMPILER IMPLEMENTATION

How rustc currently implements those guarantees.

### 3. RUNTIME BEHAVIOR

What actually happens when the program executes.

### 4. OPERATING SYSTEM BEHAVIOR

Threads, virtual memory, syscalls, scheduling, file descriptors, sockets, etc.

### 5. CPU / HARDWARE BEHAVIOR

Caches, registers, stack, heap, atomics, memory ordering, branch prediction, SIMD, NUMA, etc.

### 6. LIBRARY / ECOSYSTEM BEHAVIOR

Standard library, Tokio, Serde, Axum, tracing, Rayon, etc.

Never present implementation details as guaranteed language semantics.

When something depends on the compiler version, target architecture, operating system, allocator, optimization level, or library implementation, explicitly say so.

---

# BOOK STRUCTURE

Build the book in progressive layers.

Do NOT jump randomly between syntax, frameworks, and advanced concepts.

The dependency graph should approximately be:

```text
Programming Language Fundamentals
        ↓
Rust Syntax
        ↓
Ownership / Borrowing
        ↓
Memory Model
        ↓
Types / Traits
        ↓
Generics
        ↓
Lifetimes
        ↓
Error Handling
        ↓
Closures / Iterators
        ↓
Concurrency
        ↓
Async Rust
        ↓
OS / Networking
        ↓
Performance
        ↓
Unsafe Rust
        ↓
Compiler Internals
        ↓
Runtime / Binary / Linking
        ↓
Distributed Systems
        ↓
Production Rust
        ↓
Rust Ecosystem
        ↓
Architecture
        ↓
Language Design
```

---

# PART I — WHY RUST EXISTS

Start with the historical and engineering motivation.

Explain:

* C
* C++
* Java
* Go
* Rust

Compare their design philosophies.

Explain the trade-off between:

```text
Performance
Memory safety
Developer productivity
Runtime guarantees
Predictability
Concurrency safety
Control
Complexity
```

Explain why Rust attempts to occupy a different point in this design space.

Discuss:

* garbage collection
* manual memory management
* ownership
* deterministic destruction
* zero-cost abstractions
* compile-time safety
* runtime overhead
* explicit concurrency
* data races
* undefined behavior

Do not present Rust as universally superior.

Explain the trade-offs honestly.

---

# PART II — RUST FROM FIRST PRINCIPLES

Teach:

* cargo
* rustc
* crates
* modules
* packages
* binaries
* libraries
* editions
* `Cargo.toml`
* dependencies
* features
* build profiles

Then progressively teach:

* variables
* mutability
* scalar types
* compound types
* functions
* expressions
* statements
* control flow
* pattern matching
* structs
* enums
* methods
* modules

But every section must eventually connect to the compiler and generated program.

For example:

Do not merely say:

```rust
let x = 10;
```

Explain:

* type inference
* immutable binding
* stack representation
* optimization
* whether `x` necessarily exists at runtime
* SSA-like compiler representation
* register allocation
* debug vs release builds

---

# PART III — OWNERSHIP: THE CORE OF RUST

This must be one of the deepest sections in the entire book.

Explain:

* ownership
* move semantics
* copy semantics
* clone
* borrowing
* mutable borrowing
* references
* aliasing
* lifetimes
* slices
* ownership transfer
* destructors
* `Drop`

Build mental models.

For example:

```rust
let a = String::from("hello");
let b = a;
```

Explain exactly what changes conceptually:

```text
Stack
 ├── a ──X

Heap
 └── "hello"
```

and then:

```text
Stack
 ├── b ──→ Heap("hello")
```

Explain why Rust invalidates `a`.

Then connect it to:

* double free
* use-after-free
* dangling pointers
* aliasing
* concurrency
* cache behavior
* compiler optimization

---

# PART IV — BORROW CHECKER

Teach the borrow checker deeply.

Cover:

* immutable aliases
* mutable exclusivity
* lifetime inference
* non-lexical lifetimes
* reborrowing
* lifetime annotations
* variance where appropriate
* higher-ranked trait bounds
* lifetime elision

Do not merely say:

> "You cannot have multiple mutable references."

Explain **why**.

Connect the rule to:

```text
Aliasing XOR Mutation
```

and then to:

* compiler optimization
* data-race prevention
* memory safety
* iterator invalidation
* concurrent mutation
* CPU memory behavior

Explain common compiler errors by translating them into the underlying ownership rule.

---

# PART V — TYPES AS ARCHITECTURE

Teach:

* structs
* enums
* algebraic data types
* `Option`
* `Result`
* pattern matching
* newtype pattern
* zero-sized types
* phantom types
* marker types
* type-state pattern

Show how types can encode architectural invariants.

For example:

Instead of:

```text
Connection {
    connected: bool
}
```

show how Rust can represent:

```text
Disconnected
Connected
Authenticated
```

as different types/states.

Explain why this moves invalid states from runtime failures to compile-time errors.

---

# PART VI — TRAITS

Deeply explain:

* traits
* trait bounds
* associated types
* generic traits
* default implementations
* blanket implementations
* trait objects
* dynamic dispatch
* static dispatch
* object safety / dyn compatibility
* monomorphization

Every time static vs dynamic dispatch appears, compare:

```text
Generics
vs
dyn Trait
```

Discuss:

* compile-time cost
* binary size
* runtime dispatch
* cache locality
* extensibility
* ABI considerations
* plugin architectures

---

# PART VII — GENERICS AND MONOMORPHIZATION

Explain:

```rust
fn process<T>(x: T)
```

at multiple levels.

Show conceptual transformation:

```text
process::<i32>()
process::<String>()
process::<MyType>()
```

and explain monomorphization.

Discuss:

* code generation
* binary size
* compile time
* inlining
* optimization
* static dispatch
* dynamic dispatch

Compare with Java generics and type erasure.

The learner should understand this especially well because they come from Java.

---

# PART VIII — ERROR HANDLING

Teach:

* `Result`
* `Option`
* `?`
* custom errors
* error propagation
* error composition
* `panic!`
* recoverable vs unrecoverable failure
* library vs application error strategy

Compare:

```text
Java exceptions
Go error returns
Rust Result
```

Explain the architectural implications.

Discuss:

* error boundaries
* service boundaries
* retryable failures
* transient failures
* fatal failures
* observability
* distributed systems

---

# PART IX — COLLECTIONS AND MEMORY

Deeply explain:

* Vec
* String
* HashMap
* HashSet
* VecDeque
* BTreeMap
* slices
* arrays

For each structure explain:

```text
Memory layout
Growth strategy
Allocation behavior
Cache locality
Big-O
Constant factors
Ownership behavior
Concurrency considerations
```

For `Vec`, explain:

```text
pointer
length
capacity
```

Then explain:

* reallocations
* capacity growth
* reserve
* shrink
* stack vs heap
* cache locality

---

# PART X — ITERATORS AND ZERO-COST ABSTRACTIONS

Explain:

```rust
iter()
iter_mut()
into_iter()
map()
filter()
fold()
collect()
```

But go deeper.

Explain why Rust can make abstractions such as:

```rust
data.iter()
    .filter(...)
    .map(...)
    .collect()
```

perform similarly to carefully written loops in many cases.

Discuss:

* laziness
* inlining
* monomorphization
* LLVM optimization
* intermediate allocations
* iterator fusion

Then compare against Java Streams.

---

# PART XI — CONCURRENCY

This must be an architect-level section.

Teach:

* threads
* `Send`
* `Sync`
* `Arc`
* `Mutex`
* `RwLock`
* atomics
* channels
* scoped threads
* interior mutability

Explain:

```text
Ownership
        ↓
Borrowing
        ↓
Send / Sync
        ↓
Thread Safety
```

Explain why Rust can reject certain concurrency bugs at compile time.

Then compare:

```text
Java synchronized
Java Lock
Java volatile
AtomicInteger
Rust Mutex
Rust RwLock
Rust Atomic*
Rust channels
```

---

# CRITICAL TRADE-OFF SECTION

The book must repeatedly teach decisions like:

> "Why BFS instead of DFS?"

Do not simply give algorithmic complexity.

Analyze:

### DFS

Potential characteristics:

* recursive call stack
* explicit stack alternative
* stack depth
* stack overflow risk
* locality
* implementation simplicity

### BFS

Potential characteristics:

* queue
* heap allocation depending on data structure
* frontier width
* memory consumption
* shortest-path properties for unweighted graphs

Then explain that implementation details such as stack size are platform/runtime dependent rather than universal constants.

The same reasoning style must be applied throughout Rust.

Examples:

```text
Vec vs LinkedList
Mutex vs RwLock
Arc vs Rc
Box vs Vec
String vs &str
&[T] vs Vec<T>
enum vs trait object
static dispatch vs dynamic dispatch
sync vs async
Tokio task vs OS thread
channel vs shared state
Mutex vs atomics
clone vs borrow
Arc<Mutex<T>> vs message passing
HashMap vs BTreeMap
```

For every comparison include:

```text
Memory
CPU
Latency
Throughput
Contention
Cache behavior
Allocation
Complexity
Safety
Maintainability
Failure modes
Operational implications
```

---

# PART XII — ASYNC RUST

Deeply teach:

* Futures
* async/await
* executors
* runtimes
* wakers
* polling
* pinning
* `Future`
* `Pin`
* `Waker`
* task scheduling

Explain what actually happens when:

```rust
async fn foo() {}
```

is compiled.

Do not treat async/await as magic.

Build the conceptual pipeline:

```text
async function
      ↓
Future
      ↓
state machine
      ↓
poll()
      ↓
Pending / Ready
      ↓
Waker
      ↓
executor
```

Compare:

```text
OS thread
vs
green thread
vs
async task
```

Compare Rust async with:

* Java virtual threads
* Java CompletableFuture
* Go goroutines

---

# PART XIII — TOKIO AND PRODUCTION ASYNC

Teach Tokio deeply enough to build production systems.

Cover:

* runtime
* worker threads
* tasks
* scheduling
* channels
* timers
* TCP
* UDP
* synchronization
* cancellation
* backpressure

Explain where Tokio ends and the operating system begins.

---

# PART XIV — MEMORY MODEL AND ATOMICS

This should be treated as a senior/principal-engineer topic.

Teach:

* atomic operations
* memory ordering
* acquire
* release
* relaxed
* sequential consistency
* compare-and-swap
* fences
* happens-before
* data races
* visibility

Compare Rust atomics with Java:

```text
volatile
AtomicInteger
synchronized
VarHandle
```

Explain examples where weaker memory ordering is sufficient and why.

Never oversimplify memory ordering.

---

# PART XV — UNSAFE RUST

Do NOT teach unsafe as simply "dangerous Rust."

Explain:

```text
What safety guarantees does safe Rust provide?
What assumptions must unsafe code maintain?
```

Teach:

* raw pointers
* pointer arithmetic
* `unsafe`
* FFI
* `MaybeUninit`
* `ManuallyDrop`
* `UnsafeCell`
* custom allocators where appropriate
* aliasing rules
* invariants

Every unsafe example must include:

### Safety invariant

Explicitly state what must be true for the code to remain sound.

---

# PART XVI — FFI AND SYSTEM PROGRAMMING

Teach:

* C interoperability
* ABI
* calling conventions
* layout
* `repr(C)`
* pointers
* ownership across language boundaries
* memory allocation boundaries

Build a Rust ↔ C example.

Explain:

```text
Rust
 ↓
ABI
 ↓
C
 ↓
OS
```

---

# PART XVII — COMPILERS

Now transition from Rust user to language engineer.

Explain the compiler pipeline:

```text
Source
 ↓
Lexer
 ↓
Parser
 ↓
AST
 ↓
HIR
 ↓
Type Checking
 ↓
Borrow Checking
 ↓
MIR
 ↓
Optimization
 ↓
LLVM IR
 ↓
Machine Code
 ↓
Linker
 ↓
Executable
```

Explain each layer.

Introduce enough compiler theory to allow the learner to eventually build a programming language.

Cover:

* lexing
* parsing
* recursive descent
* Pratt parsing
* AST
* symbol tables
* name resolution
* type checking
* type inference
* intermediate representations
* control-flow graphs
* SSA
* optimization
* code generation
* linking

---

# PART XVIII — HOW RUSTC WORKS

Explain Rust compiler architecture in progressively deeper levels.

Cover concepts such as:

* rustc
* parser
* AST
* HIR
* THIR where relevant
* MIR
* borrow checking
* type checking
* trait solving
* code generation
* LLVM

Clearly distinguish stable public concepts from compiler implementation details that may change.

The learner should eventually be able to trace:

```rust
let x = foo();
```

through the conceptual compilation pipeline.

---

# PART XIX — BINARY / LINKER / OS

Explain what happens after compilation.

Teach:

* executable format
* ELF
* linking
* static linking
* dynamic linking
* shared libraries
* symbols
* relocation
* stack
* heap
* virtual memory
* pages
* mmap
* syscalls
* file descriptors
* processes
* threads

Explain:

```text
Rust source
→ machine code
→ executable
→ process
→ virtual address space
→ physical memory
→ CPU execution
```

---

# PART XX — PERFORMANCE ENGINEERING

Teach performance as a measurement discipline.

Cover:

* profiling
* benchmarking
* flame graphs
* CPU profiling
* memory profiling
* allocations
* cache misses
* branch prediction
* false sharing
* lock contention
* NUMA
* SIMD
* vectorization

Teach:

```text
Latency
Throughput
Tail latency
CPU utilization
Memory utilization
Allocation rate
GC pressure
Lock contention
```

Explain why premature optimization is dangerous.

Require benchmarks before making performance claims.

---

# PART XXI — NETWORKING

Teach production networking in Rust.

Cover:

* TCP
* UDP
* HTTP
* HTTP/2
* HTTP/3 conceptually
* TLS
* sockets
* connection pooling
* backpressure
* timeouts
* retries
* circuit breakers
* load balancing

Then build systems using:

* Tokio
* Hyper
* Axum or another major Rust web framework
* Serde

Do not teach frameworks before the underlying concepts.

---

# PART XXII — RUST WEB / BACKEND ECOSYSTEM

Teach the ecosystem as an architect.

Cover relevant categories such as:

```text
Web:
Axum / Actix Web

Async:
Tokio

Serialization:
Serde

Database:
SQLx / Diesel

Logging:
tracing

CLI:
clap

Parallelism:
Rayon

HTTP:
reqwest / hyper

Testing:
built-in test framework + relevant ecosystem tools
```

For each technology explain:

* problem solved
* abstraction level
* internal architecture
* performance characteristics
* ecosystem maturity
* when to choose it
* when not to choose it
* alternatives

Do not turn the book into framework documentation.

---

# PART XXIII — DATABASES AND STORAGE

Build storage systems in Rust.

Teach:

* memory-mapped files
* WAL
* B-trees
* LSM trees
* indexes
* serialization
* compaction
* concurrency control
* transactions

Then build a progressively more capable miniature database.

---

# PART XXIV — DISTRIBUTED SYSTEMS IN RUST

Connect Rust to the learner's system-design background.

Build progressively:

1. TCP server
2. HTTP service
3. concurrent key-value store
4. persistent key-value store
5. replicated store
6. leader/follower replication
7. consensus concepts
8. distributed cache
9. message broker
10. rate limiter

For each system discuss:

```text
Failure model
Consistency
Availability
Concurrency
Backpressure
Memory
Network
Persistence
Recovery
Observability
```

---

# PART XXV — BLOCKCHAIN / INFRASTRUCTURE

Treat blockchain as one systems domain rather than assuming it is automatically the future.

Explain where Rust is useful for:

* high-performance nodes
* networking
* cryptographic infrastructure
* execution engines
* storage
* consensus implementations
* smart-contract tooling
* WASM environments

Explain the underlying systems concepts rather than teaching token speculation.

---

# PART XXVI — BUILDING A LANGUAGE

This is the final transformation.

Ask:

> "If you had to build a programming language inspired by Rust, what would you need?"

Build a toy language incrementally.

Stage 1:

```text
Lexer
Parser
AST
Interpreter
```

Stage 2:

```text
Variables
Functions
Types
```

Stage 3:

```text
Structs
Enums
Pattern Matching
```

Stage 4:

```text
Ownership
Borrowing
Lifetimes
```

Stage 5:

```text
Type Checker
Borrow Checker
```

Stage 6:

```text
MIR-like IR
```

Stage 7:

```text
LLVM code generation
```

The learner should finish understanding the relationship between:

```text
Rust language design
Rust compiler
Rust memory model
Rust type system
Rust borrow checker
Rust runtime
Rust ecosystem
```

---

# EVERY CHAPTER MUST CONTAIN

Each chapter should follow this structure:

## 1. Problem

What real engineering problem does this concept solve?

## 2. Mental Model

Give a simple conceptual model.

## 3. Rust Code

Show idiomatic Rust.

## 4. Under the Hood

Explain compiler/runtime behavior.

## 5. Memory

Show stack/heap/reference diagrams where relevant.

## 6. CPU / OS

Explain lower-level implications when relevant.

## 7. Trade-offs

Explicitly compare alternatives.

## 8. Java Comparison

Because the learner comes from Java, compare with Java when useful.

For example:

```text
Java ArrayList
vs
Rust Vec
```

or:

```text
Java GC
vs
Rust ownership
```

or:

```text
Java synchronized
vs
Rust Mutex
```

## 9. Production Scenario

Give a realistic backend/infrastructure scenario.

## 10. Failure Scenario

Show how an incorrect implementation fails.

## 11. Interview / Architecture Questions

Ask questions such as:

> Why does Rust prohibit this?

> What happens if we replace this with Arc<Mutex<T>>?

> What is the allocation behavior?

> What happens under contention?

> What happens when this service handles 1M requests/sec?

## 12. Exercises

Provide:

* beginner exercise
* intermediate exercise
* advanced exercise
* systems exercise
* architecture exercise

## 13. Debugging Exercise

Give broken Rust code and ask the learner to diagnose it.

## 14. Design Exercise

Give an architecture problem where multiple valid solutions exist.

Require the learner to justify trade-offs.

---

# DIAGRAM REQUIREMENT

Use diagrams heavily.

For important concepts use ASCII diagrams such as:

```text
        Ownership
            |
            v
      +-------------+
      |   String    |
      +-------------+
       |     |     |
       v     v     v
    ptr     len   cap
       |
       v
     HEAP
```

For async:

```text
async fn
   |
   v
Future
   |
   v
State Machine
   |
   v
poll()
 /   \
Pending Ready
  |
  v
Waker
  |
  v
Executor
```

For compilation:

```text
Rust Source
     |
     v
   Parser
     |
     v
    AST
     |
     v
    HIR
     |
     v
Type Checking
     |
     v
Borrow Checking
     |
     v
    MIR
     |
     v
   LLVM
     |
     v
Machine Code
```

---

# TRADE-OFF ENGINE

Whenever two approaches exist, create a decision matrix.

Example:

| Decision        | Option A | Option B |
| --------------- | -------- | -------- |
| Dispatch        | Static   | Dynamic  |
| Runtime cost    | ...      | ...      |
| Binary size     | ...      | ...      |
| Compile time    | ...      | ...      |
| Extensibility   | ...      | ...      |
| Best suited for | ...      | ...      |

Do NOT declare a universal winner.

Explain what circumstances make each choice appropriate.

---

# "WHY NOT?" QUESTIONS

The book must repeatedly ask:

> Why not simply use X?

Examples:

* Why not Java?
* Why not Go?
* Why not C++?
* Why not garbage collection?
* Why not `clone()` everything?
* Why not `Arc<Mutex<T>>` everywhere?
* Why not use threads instead of async?
* Why not use async everywhere?
* Why not use `unsafe`?
* Why not use dynamic dispatch?
* Why not use a lock-free data structure?
* Why not use BFS?
* Why not use DFS?
* Why not use a database?
* Why not use an in-memory cache?

Answer using engineering trade-offs.

---

# PRODUCTION-LEVEL THINKING

The learner must gradually stop thinking:

> "How do I make this compile?"

and start thinking:

> "What guarantees does this design provide?"

Then:

> "What does it cost?"

Then:

> "How does it behave under failure?"

Then:

> "How does it behave at scale?"

Then:

> "What happens at the OS and CPU level?"

Then:

> "Could I implement the abstraction myself?"

That progression is the central goal of the book.

---

# PROJECT LADDER

The book must contain increasingly difficult projects.

### Level 1

CLI tools.

### Level 2

File processor.

### Level 3

HTTP server.

### Level 4

Concurrent key-value store.

### Level 5

Async TCP server.

### Level 6

Database client/server.

### Level 7

Persistent storage engine.

### Level 8

Distributed cache.

### Level 9

Message broker.

### Level 10

Mini database.

### Level 11

Mini distributed database.

### Level 12

Toy programming language.

### Level 13

Toy compiler.

The final projects should integrate concepts from the entire book.

---

# ARCHITECTURE REVIEWS

After major projects, act as a principal engineer conducting an architecture review.

Ask:

* What are the bottlenecks?
* Where are allocations occurring?
* Where can contention happen?
* What happens if a node crashes?
* What happens under backpressure?
* What is the consistency model?
* What is the failure model?
* What are the memory ownership boundaries?
* Where is unsafe code required?
* Can this be made zero-copy?
* What happens under 10x load?
* What happens under 100x load?
* How would you benchmark it?
* How would you observe it?
* What would you change if latency became the primary requirement?
* What would you change if throughput became the primary requirement?
* What would you change if developer velocity became the primary requirement?

---

# INTERVIEW MODE

At the end of every major section, generate senior-level interview questions.

Do NOT give the answers immediately.

Questions should include:

### Language

* Why does Rust have ownership?
* Why are references not nullable?
* What is the difference between `Copy` and `Clone`?
* What is a lifetime really describing?

### Compiler

* How does borrow checking work conceptually?
* What is MIR?
* What is monomorphization?

### Concurrency

* Why are `Send` and `Sync` separate?
* Why is `Rc<T>` not thread-safe?
* What does `Arc<T>` actually provide?

### Async

* What does `.await` do?
* Why does a Future not execute immediately?
* What is a Waker?

### Performance

* When does `clone()` become expensive?
* What causes allocations?
* When is dynamic dispatch preferable?

### Architecture

* When would you choose Rust over Java or Go?
* When would Rust NOT be the appropriate choice?
* How would you design a 1M requests/sec service in Rust?

---

# ADVANCED "WHAT ACTUALLY HAPPENS?" QUESTIONS

Frequently stop and ask questions like:

> What actually happens when this function is called?

> Where does this value live?

> Who owns this memory?

> When is the destructor executed?

> What does the compiler know?

> What does LLVM know?

> What does the CPU actually execute?

> Does this abstraction allocate?

> Does this operation copy?

> Can the compiler eliminate the copy?

> What happens when multiple threads execute this?

> What happens when the process crashes?

These questions should become the learner's default mental model.

---

# SOURCE QUALITY

Use authoritative sources whenever possible.

Prioritize:

1. Official Rust documentation
2. Rust Reference
3. Rustonomicon
4. Rust compiler documentation
5. Rust RFCs
6. Official Rust blog
7. Tokio documentation
8. LLVM documentation
9. Operating-system documentation
10. Academic papers where relevant

When discussing implementation details, distinguish:

```text
Guaranteed by Rust
```

from:

```text
Current implementation detail
```

Never fabricate compiler internals.

---

# VERSION AWARENESS

Rust evolves.

Whenever a concept is version-sensitive:

* identify the relevant Rust edition/version
* explain whether the behavior is stable
* avoid presenting unstable/nightly behavior as stable
* identify when compiler implementation details may change

Prefer stable Rust for production examples unless the chapter specifically studies nightly/compiler development.

---

# CODE REQUIREMENTS

All code must:

* compile conceptually
* use idiomatic modern Rust
* explain important lines
* include error handling where appropriate
* avoid unnecessary cleverness
* include comments only when they add educational value

For difficult examples provide:

```text
Code
↓
Expected behavior
↓
Memory model
↓
Compiler reasoning
↓
Runtime behavior
```

---

# JAVA COMPARISON REQUIREMENT

Because the learner has a Java backend background, whenever a useful comparison exists, include it.

Examples:

```text
Java Object
Rust struct

Java interface
Rust trait

Java ArrayList
Rust Vec

Java HashMap
Rust HashMap

Java Optional
Rust Option

Java Exception
Rust Result

Java GC
Rust ownership + Drop

Java synchronized
Rust Mutex

Java volatile
Rust atomic / memory ordering

Java CompletableFuture
Rust Future

Java ExecutorService
Rust runtime / task executor
```

But never force a Java analogy when it becomes misleading.

Explicitly say:

> "This analogy is useful only up to this point."

---

# FINAL CAPSTONE

The learner should eventually be challenged with this:

> Design a systems programming language inspired by Rust.

They must define:

```text
Memory model
Ownership model
Type system
Generics
Traits/interfaces
Error handling
Concurrency model
Async model
Module system
Compiler architecture
Intermediate representation
Runtime model
FFI
Package manager
Build system
```

Then implement a small subset.

The purpose is not merely to build the language.

The purpose is to prove that the learner understands **why Rust looks the way it does**.

---

# FINAL LEARNING OUTCOME

At the end of the book, the learner should be able to look at Rust code and reason across five layers:

```text
┌─────────────────────────────┐
│ Architecture                │
├─────────────────────────────┤
│ Distributed System          │
├─────────────────────────────┤
│ Runtime / OS                │
├─────────────────────────────┤
│ Compiler / Language         │
├─────────────────────────────┤
│ CPU / Memory                │
└─────────────────────────────┘
```

They should not merely know:

> "How to write Rust."

They should understand:

> **Why Rust was designed this way, how the compiler enforces its guarantees, what machine-level behavior results, what trade-offs those guarantees impose, and when an architect should choose one design over another.**

The final goal is:

**Rust Developer → Systems Engineer → Senior Engineer → Principal Engineer → Language/Compiler/System Architect.**

Generate the book incrementally, one major part at a time. Maintain continuity between chapters. Do not skip foundational concepts merely because the learner already knows programming; instead, compress familiar material while going deep on the parts that are unique to Rust and systems programming.

Never optimize for the number of topics covered.

Optimize for **depth of understanding, ability to reason from first principles, and architectural judgment.**

---

# ONE IMPORTANT ADDITION: THE THREE-PASS LEARNING MODEL

Every major Rust concept follows a three-pass learning model:

**Pass 1 — User level:**
"How do I use this?"

**Pass 2 — Systems level:**
"What happens in memory, compiler, runtime, OS and CPU?"

**Pass 3 — Architect level:**
"When should I choose this design, what are the alternatives, and what happens at scale?"

That is what prevents the book from becoming another "Rust Book + 200 syntax examples." It trains the mental model of **understanding the mechanism well enough to make engineering decisions from first principles.**
