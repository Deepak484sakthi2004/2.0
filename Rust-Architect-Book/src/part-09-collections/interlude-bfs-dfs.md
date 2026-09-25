# Interlude — The Trade-off Engine: BFS vs DFS, Down to the Stack Page

> **Where this sits:** Part IX · between Chapter 9.5 and the Part review. This is the book's worked example of the
> SPEC's "critical trade-off" method, and every later comparison (Mutex vs RwLock, Arc vs Rc, sync vs async, channel vs
> shared state) follows its shape.
> **Prerequisites:** Chapter 2.4 (stack frames, no guaranteed tail calls, the "overflowed its stack" abort), Chapter
> 3.6 (graphs with indices), Chapters 9.1–9.5.
> **After this interlude you can:** measure a function's stack frame and predict the recursion depth that overflows a
> given stack; explain, down to the guard page, what happens when it does; convert any recursive DFS into an explicit
> stack that preserves its semantics; predict a BFS frontier's memory from the graph's shape; and write a 12-criterion
> decision matrix for any "X vs Y" choice.

---

## Pass 1 · User level — *Two frontiers*

### 1. Problem

"Why BFS instead of DFS?" has a textbook answer: both are O(V + E); BFS finds shortest paths in unweighted graphs; DFS
gives you post-order for topological sorts and cycle detection. That answer is complete for an algorithms exam and
incomplete for a production system. In a service, the questions are different:

- Will this traversal **crash the process** on some input?
- How much **memory** does it need at its peak, and is that bounded by something I control?
- What does it **allocate**, and where do its bytes live?
- Does it give the **answer I need** (a shortest path? a post-order?) or just *an* answer?

Chapter 2.4 promised this interlude for deep recursion, and Chapter 3.6's exercise asked for cycle detection with an
explicit stack. Both are here, measured.

### 2. Mental model

**Both searches are the same loop with a different frontier:**

```text
 frontier ← {start}; mark start
 while frontier not empty:
     v ← take from frontier            DFS: take the NEWEST (LIFO)   BFS: take the OLDEST (FIFO)
     for each neighbor w of v:
         if w not marked: mark w; put w in frontier
```

The frontier is where the memory goes, and its peak size depends on the **shape of the graph**:

```text
                    wide & shallow (tree, depth 20)        narrow & deep (chain of 1M)
 DFS frontier        ~depth           = 21                  ~depth = 1,000,000
 BFS frontier        ~widest level    = 1,048,576           ~1

 where it lives:     recursive DFS → the THREAD STACK (fixed size, overflow = abort)
                     explicit DFS  → a Vec on the HEAP (grows, failure = allocation error)
                     BFS           → a VecDeque on the HEAP
```

So the choice isn't "BFS or DFS". It's three choices: **which frontier order** (does the answer need LIFO or FIFO?),
**where the frontier lives** (the call stack or the heap), and **what bounds its size** (the graph's depth or its
width).

### 3. Rust code

**How big is one recursive frame?** (listing `intl-01-frame-size.rs`, verified in debug and release). The DFS records
the address of a local variable at depth 0 and depth 1,000. The difference divided by 1,000 is the frame size:

```rust,ignore
#[inline(never)]
fn dfs(adj: &[Vec<u32>], node: u32, visited: &mut [bool], depth: usize, probe: &mut [usize; 2]) -> usize {
    let marker = 0u8;
    if depth == 0 {
        probe[0] = &marker as *const u8 as usize;
    } else if depth == 1000 {
        probe[1] = &marker as *const u8 as usize;
    }
    visited[node as usize] = true;
    let mut deepest = depth;
    for &next in &adj[node as usize] {
        if !visited[next as usize] {
            deepest = deepest.max(dfs(adj, next, visited, depth + 1, probe));
        }
    }
    deepest
}
```

```text
[debug] one dfs() frame = 224 bytes
[debug] predicted max depth: ~9362 on a 2 MiB thread, ~37449 on an 8 MiB stack
[debug] chain of 8425 nodes (90% of prediction) on a 2 MiB thread: ok, reached depth 8424
[release] one dfs() frame = 96 bytes
[release] predicted max depth: ~21845 on a 2 MiB thread, ~87381 on an 8 MiB stack
[release] chain of 19660 nodes (90% of prediction) on a 2 MiB thread: ok, reached depth 19659
```

**Then 110% of the prediction** (listing `intl-02-dfs-overflow.rs`, verified to crash in both profiles):

```text
[debug] frame 224 bytes, predicted max ~9362; trying a chain of 10298 (110%)

thread '<unknown>' (45) has overflowed its stack
fatal runtime error: stack overflow, aborting
```

```text
[release] frame 96 bytes, predicted max ~21845; trying a chain of 24029 (110%)

thread '<unknown>' (44) has overflowed its stack
fatal runtime error: stack overflow, aborting
```

That is the whole story of recursive DFS in production, in four numbers:

- **The same code overflows at different depths depending on the build profile**: about 9,400 levels in debug and about
  21,800 in release, on the same 2 MiB thread. Your tests (debug) and production (release) disagree.
- **The limit is modest.** Twenty thousand is not a big number for a graph: a linked chain of accounts, a deeply nested
  JSON document, a long dependency chain.
- **The failure is an abort**, not a panic. There's no unwinding and no `catch_unwind`. The whole process dies, taking
  every other request and task with it.
- **The message says `'<unknown>'`** because the thread was spawned without a name. Name your threads
  (`thread::Builder::new().name("fraud-graph".into())`) so the one line you get in the logs says which pool died.

**The explicit stack that behaves exactly like the recursion** (listing `intl-03-dfs-explicit.rs`, verified). Each
heap "frame" is the pair `(node, index of the next edge to try)`: exactly the state the recursive version keeps in its
machine frame (the current node, and where it was in the loop over neighbors):

```rust,ignore
fn dfs_frames(adj: &[Vec<u32>], start: u32) -> (Vec<u32>, Vec<u32>, usize) {
    let mut parent = vec![NONE; adj.len()];
    let mut visited = vec![false; adj.len()];
    let mut post = Vec::with_capacity(adj.len());
    let mut stack: Vec<(u32, u32)> = vec![(start, 0)];
    visited[start as usize] = true;
    let mut peak = 1;
    while let Some(top) = stack.last_mut() {
        let (node, edge) = *top;
        match adj[node as usize].get(edge as usize) {
            Some(&next) => {
                top.1 += 1; // resume point: like the saved instruction pointer of a real frame
                if !visited[next as usize] {
                    visited[next as usize] = true;
                    parent[next as usize] = node;
                    stack.push((next, 0));
                    peak = peak.max(stack.len());
                }
            }
            None => {
                post.push(node); // all children done: post-order position (topological sort needs this)
                stack.pop();
            }
        }
    }
    (parent, post, peak)
}
```

```text
explicit stack, 1,000,000-node chain, 2 MiB thread: visited 1000000, peak stack 1000000 frames = 7812 KiB of heap
recursive, 200,000-node chain, 64 MiB thread: visited 200000
frame-stack DFS: parents [None, Some(0), Some(1), Some(2)], post-order [3, 2, 1, 0]
mark-on-push:    parents [None, Some(0), Some(1), Some(0)]
```

- **A million-deep chain on the same 2 MiB thread just works**, because the frontier is in a `Vec` on the heap. Each
  heap frame is 8 bytes (two `u32`s) against 96 bytes for a release machine frame, so the explicit version also uses
  12× less memory at the same depth: 7.8 MiB instead of ~92 MiB.
- **Or keep the recursion and size the stack.** A 64 MiB thread runs the recursive version on a 200,000-node chain.
  That's a legitimate fix when the maximum depth is known and bounded, but it turns the depth limit into a deployment
  parameter that must be kept in sync with the data.
- **The popular shortcut is a different algorithm.** The last two lines use the graph `0 → {1, 3}, 1 → {2}, 2 → {3}`.
  The common "stack of nodes, mark on push" loop visits every node once, but it discovers node 3 from node 0 (parent
  `Some(0)`), while real DFS discovers it through 1 and 2 (parent `Some(2)`). Its tree is different, it has no
  post-order, and it can't tell a back edge (a cycle) from a cross edge. For reachability it's fine. For topological
  sort or cycle detection, it's wrong. Use the `(node, edge)` frame.

**BFS: the frontier is as wide as the graph** (listing `intl-04-bfs-frontier.rs`, verified in debug and release):

```text
binary tree, 2097151 nodes (depth 20):
  BFS peak queue 1048576 nodes (capacity 1048576 = 4096 KiB); DFS peak stack 21 nodes
  allocator calls for both traversals (incl. two 2097151-entry visited arrays): 27
chain, 1000000 nodes: BFS peak queue 1, DFS peak explicit stack 1 (recursion would need depth 1000000)
20x20 grid, (0,0) -> (19,19): BFS path 38 steps, DFS path 190 steps
```

- **On the wide tree, BFS holds the whole bottom level at once**: 1,048,576 entries in a `VecDeque<u32>`, 4 MiB,
  reached through about 19 doublings (27 allocator calls for everything, including both `visited` arrays and the scratch
  neighbor buffer). DFS holds 21.
- **On the chain, BFS's queue never exceeds 1.** An explicit mark-on-push DFS stack doesn't either (each node has one
  neighbor), but a *recursive* DFS would need a million frames: the case that crashed above.
- **On the grid, BFS returns the shortest path**: 38 steps, exactly the Manhattan distance 19 + 19. DFS, following
  "right, down, left, up" greedily, found a valid path of 190 steps. This is a property of the algorithm, not of Rust:
  BFS explores in order of distance, so in an unweighted graph the first time it reaches a node is along a shortest
  path. DFS makes no such promise.

---

## Pass 2 · Systems level — *Down to the stack page*

### 4. Under the hood

**What's in a 96-byte frame.** Here's the release prologue of `dfs` (listing `intl-01-frame-size.rs`, fetched with
`tools/emit.ps1 -Target asm -Mode release -CrateType bin`, trimmed; the label numbers are rustc's):

```text
playground::dfs:
	push	rbp                         ; 6 callee-saved registers:  6 × 8 = 48 bytes
	push	r15
	push	r14
	push	r13
	push	r12
	push	rbx
	sub	rsp, 40                     ; locals + spill slots + outgoing stack argument: 40 bytes
	mov	r15, r9                     ; depth
	...
	mov	byte ptr [rsp + 15], 0      ; `marker` lives here, so its address is a real stack address
	cmp	r9, 1000
	je	.LBB20_12
	...
	mov	rsi, qword ptr [rsp + 96]   ; the 6th argument (`probe`) arrives on the stack, just above the frame
```

The call instruction pushes an 8-byte return address. 8 + 48 + 40 = **96 bytes**, matching the measurement. [RUSTC]
Every part of that number is a compiler decision: which registers the function needs across its recursive call (and so
must save), how many locals spill, whether arguments fit in registers (the x86-64 System V ABI passes six integer
arguments in registers; our `dfs` has five parameters, but `adj` and `visited` are slices of two words each, so the
seventh word, `probe`, goes on the stack). In **debug**, nothing lives in registers across statements, so every local and temporary gets its own
stack slot: 224 bytes. Change the function and the number changes. That's why this book measures frame size instead of
quoting one.

**The stack is a mapping with a hole below it.** [OS][RUNTIME] When std spawns a thread on Linux, glibc's
`pthread_create` `mmap`s the stack (2 MiB here, `thread::Builder::stack_size`) and places a **guard page**, mapped with
no access permissions, at its low end:

```text
 high addresses
 ┌──────────────────────────┐ ← stack top: thread start, main closure's frame
 │ dfs frame (depth 0)      │   96 bytes
 │ dfs frame (depth 1)      │   96 bytes
 │ ...                      │   the stack grows DOWN
 │ dfs frame (depth 21,8xx) │
 ├──────────────────────────┤ ← 2 MiB below the top
 │ GUARD PAGE (PROT_NONE)   │   any access → SIGSEGV
 └──────────────────────────┘
 low addresses
```

When the next frame's first write lands in the guard page, the CPU raises a page fault and the kernel delivers
`SIGSEGV`. std's runtime has installed a `SIGSEGV`/`SIGBUS` handler that runs on an **alternate signal stack**
(`sigaltstack`, since the normal stack is exactly what's exhausted). The handler checks whether the faulting address is
inside the current thread's guard range. If it is, it prints `thread '...' has overflowed its stack` and aborts. If
not, it restores the default action and returns, and the fault then kills the process as an ordinary segfault. Either
way, it's not recoverable, because unwinding would need stack space to run destructors and there isn't any.

**Stack probes: why a big frame can't jump the guard.** A guard page catches the *first* access below the stack. But a
function with a 16 KiB local array moves `rsp` down by 16 KiB in one instruction, and it could write to a page
*below* the guard without ever touching the guard itself, corrupting whatever memory happens to be mapped there.
[RUSTC] rustc prevents that with **stack probes**: any frame larger than a page touches each page in order. Here's a
function with a 16 KiB array (listing `intl-05-stack-probe.rs`, release asm, trimmed):

```text
playground::big_frame:
	sub	rsp, 4096
	mov	qword ptr [rsp], 0          ; touch page 1 — faults on the guard page if we've run out
	sub	rsp, 4096
	mov	qword ptr [rsp], 0          ; touch page 2
	sub	rsp, 4096
	mov	qword ptr [rsp], 0          ; touch page 3
	sub	rsp, 3976                   ; the rest (16 KiB array + locals)
	...

playground::small_frame:
	sub	rsp, 392                    ; 512-byte array: under a page, no probes
	mov	qword ptr [rsp - 120], rdi  ; (a leaf function may also use the 128-byte red zone below rsp)
	...
```

This is why stack overflow in safe Rust is a clean abort and never silent memory corruption: the guard page plus
probes make the overflow *detectable*. C compilers need `-fstack-clash-protection` for the same guarantee; in Rust it's
the default on major targets. [VERSION] The probe mechanism changed over time (a `__rust_probestack` function call on
older toolchains, inline probes like the above on current ones).

**Stack sizes are platform and runtime facts, not constants.** The SPEC's point, as a table ([RUNTIME]/[OS]; defaults
as documented, and each one is configurable):

| Where the code runs | Default stack | Set by | Notes |
|---|---|---|---|
| Rust `std::thread::spawn` | 2 MiB | std | override per thread with `Builder::stack_size`, or globally with the `RUST_MIN_STACK` environment variable |
| Rust main thread, Linux | typically 8 MiB | `ulimit -s` (RLIMIT_STACK) at exec | grows on demand up to the limit |
| Rust main thread, Windows | 1 MiB | the linker (`/STACK` reserve in the PE header) | 8× smaller than Linux: recursion that passes on Linux can fail on Windows |
| Rust main thread, macOS | 8 MiB | the OS | secondary pthreads default to 512 KiB, but Rust threads use 2 MiB |
| Tokio worker threads | 2 MiB | `runtime::Builder::thread_stack_size` | an async task's *state* lives on the heap in its future, but everything it calls synchronously runs on the worker's stack |
| Java threads (HotSpot, Linux x64) | 1 MiB | `-Xss` / `-XX:ThreadStackSize` | overflow throws a catchable `StackOverflowError` |
| Go goroutines | starts at a few KiB | the runtime | grows by copying, up to a large maximum (1 GB on 64-bit by default) |

Two operational consequences:

- **The same binary has different limits in different places.** A recursive function that is fine on the main thread
  (8 MiB) can crash on a worker thread (2 MiB) or on Windows (1 MiB), and `RUST_MIN_STACK` set in one environment and
  not another changes the limit without a code change.
- **Async doesn't remove the stack.** A recursive `async fn` needs `Box::pin` (its future would otherwise have
  infinite size, Part XII), which moves each level's state to the heap. A *synchronous* recursive function called from
  async code still recurses on the worker's 2 MiB stack.

### 5. Memory

**Peak memory, by frontier and by graph shape** (V nodes, depth D, maximum level width W):

| | Recursive DFS | Explicit DFS, `(node, edge)` frames | Explicit DFS, mark on push | BFS (`VecDeque<u32>`) |
|---|---|---|---|---|
| Frontier memory | D × frame (96 B release, 224 B debug, here) | D × 8 B | up to V × 4 B (every pushed neighbor waits) | W × 4 B |
| Where | thread stack (fixed) | heap (growable) | heap | heap |
| Wide tree (D = 21, W = 2²⁰) | ~2 KiB | 168 B | small | 4 MiB |
| Chain (D = 10⁶, W = 1) | ~92 MiB: **abort** on 2 MiB | 7.8 MiB | 4 B | 4 B |
| Plus | the `visited` set: V bits to V bytes, the same for all | | | |

The mark-on-push variant's worst case surprises people: on a dense graph, a node's whole neighbor list is pushed at
once, so the stack can hold O(E) entries at worst unless you mark on push (which bounds it by V). The `(node, edge)`
frame holds one entry per level of the current path, never more.

**Allocation behavior.** Recursion allocates nothing, which is its real advantage. The explicit stack and the BFS queue
grow by doubling like any `Vec`: about log₂(peak) allocations, 27 calls in total in the tree measurement. Presizing
with `with_capacity` is possible when you know the bound. And the stack and queue can be **reused** across traversals
(`clear()` keeps capacity, with the capacity policy from Chapter 9.1), which makes a traversal service allocation-free
in steady state.

### 6. CPU / OS

**Locality.** [CPU] The honest answer is that it depends more on the graph's memory layout than on the algorithm:

- Both algorithms touch the `visited` array and the adjacency lists at addresses decided by node IDs. If IDs are
  assigned in a traversal-friendly order (a tree stored in preorder suits DFS; a heap-shaped array, like
  `BinaryHeap`'s, suits BFS), the matching algorithm streams through memory. Random IDs make both random.
- DFS has better *temporal* locality on trees: it finishes a subtree while the subtree's nodes are still cached.
- BFS processes whole levels, so its frontier is naturally a batch of independent lookups, and that's exactly the
  memory-level parallelism Chapter 9.5 measured. It's also why parallel graph processing is usually BFS-shaped.

Those are predictions from mechanism. The Systems exercise measures them.

**Call overhead.** A recursive call costs a `call`, a prologue (six pushes here), an epilogue, and a `ret`, which the
CPU's return-stack buffer predicts well until the recursion is deeper than the buffer (a few dozen entries on current
cores). An explicit stack costs a `Vec` push/pop with a capacity check. Neither dominates a traversal whose real cost is
cache misses on the graph. Expect them to be within a small factor of each other, and measure if it matters.

**Throughput and contention.** DFS is inherently sequential: the next step depends on the last. BFS levels are
independent sets that can be processed in parallel (level-synchronous BFS with `rayon`, Part XI), at the cost of a
shared `visited` set (atomics, or per-thread sets merged per level). That's the one place BFS has a structural
advantage for throughput.

---

## Pass 3 · Architect level — *The trade-off engine*

### 7. Trade-offs

**The decision matrix** (the SPEC's twelve criteria; "Recursive DFS" means DFS on the thread stack):

| Criterion | Recursive DFS | Explicit-stack DFS (`(node, edge)` frames) | BFS (`VecDeque`) |
|---|---|---|---|
| **Memory** | depth × frame (96–224 B/level measured); fixed ceiling | depth × 8 B; grows as needed | max level width × 4 B; can be O(V) on wide graphs |
| **CPU** | call/ret + prologue per level; no allocation | `Vec` push/pop per level | queue push/pop per node |
| **Latency** | no allocation spikes | occasional reallocation (presize to remove) | reallocations as the frontier widens |
| **Throughput** | sequential | sequential | levels can run in parallel |
| **Contention** | none | none | shared `visited` if parallelized |
| **Cache** | good temporal locality on trees; the stack itself is always hot | the same, plus a dense heap stack | level-at-a-time: batchable, overlapping misses |
| **Allocation** | none | ~log₂(depth) | ~log₂(width) |
| **Complexity** | simplest to write and read | moderate: the frame must capture the resume point | simple |
| **Safety** | safe, but aborts on deep input: a **DoS vector** when input controls depth | safe; failure is an allocation error at a far higher depth | safe; memory proportional to width |
| **Maintainability** | obvious code; the depth limit is invisible | more code; the limit is explicit (you can cap the `Vec`) | obvious code |
| **Failure modes** | process abort, varies by profile and platform | OOM only at extreme depth; a wrong "shortcut" gives a wrong DFS tree | memory blowup on wide graphs |
| **Operational implications** | stack size becomes a deployment parameter (`RUST_MIN_STACK`, `Builder::stack_size`) | none beyond memory limits | frontier size must fit the memory budget |

**How to decide:**

1. **Does the answer need FIFO order?** Shortest paths in unweighted graphs, "everything within k hops", level-by-level
   processing: BFS. (For weighted shortest paths, Dijkstra with a `BinaryHeap`, Chapter 9.4.)
2. **Does it need DFS structure?** Post-order (topological sort, dependency resolution), back-edge detection (cycles),
   strongly connected components, subtree aggregates: DFS with `(node, edge)` frames.
3. **Is depth bounded by something you control?** If yes (a parse tree with a grammar-enforced limit, your own data
   structure's height), recursion is fine and clearest. **If input controls depth, recursion needs a hard depth limit,
   or it's a denial-of-service vector.** `serde_json` does exactly this: [LIB] its deserializer refuses input nested
   more than 128 levels deep by default, and the limit can only be disabled explicitly.
4. **Is width bounded?** BFS on a high-fan-out graph (a social graph, a dependency graph with popular nodes) can hold a
   large fraction of the graph in its queue. Iterative deepening DFS finds shortest paths with DFS memory at the cost
   of repeated work, when BFS's frontier doesn't fit.

### 8. Java comparison

| | Java | Rust |
|---|---|---|
| Default thread stack | 1 MiB (HotSpot, Linux x64) | 2 MiB for spawned threads; the OS decides for main |
| Overflow | `StackOverflowError`, catchable | abort, not catchable |
| Detection | guard zones (HotSpot reserves yellow/red zones to run the handler) | guard page + `sigaltstack` handler in std |
| Frame size | depends on interpreter vs JIT tier; JIT frames are small | depends on profile; measured 224 B debug vs 96 B release |
| The explicit-stack rewrite | `ArrayDeque<int[]>` of boxed frames, or an `int[]` stack | `Vec<(u32, u32)>`: inline, 8 B per frame |

> **Analogy limit.** "Java catches `StackOverflowError`, Rust aborts" sounds like a point for Java. In practice, catching
> it is fragile: the error can be thrown in the middle of any method, including in library code holding a lock or
> halfway through updating a data structure, and the JVM's own docs discourage recovery from `Error`s. Services that
> "handle" it usually just log and continue with possibly corrupted state. Rust's abort is honest about the same
> reality. The actual fix is the same in both languages: bound the depth, or don't use the call stack for
> input-controlled recursion.

> **Why not just raise the stack size?** It's a fine tool when depth is known (the verified 64 MiB thread above), and a
> poor one when input controls depth, because there's always a deeper input. It also costs virtual address space per
> thread (physical memory is committed only as pages are touched), and it's invisible configuration that someone will
> "clean up" later.

### 9. Production scenario

**Linked-account traversal in fraud scoring.** Meridian's fraud system flags accounts connected to a known-bad
account through shared devices, cards, or addresses, within a few hops. The Java version used recursive DFS with a
depth limit of 6 and ran fine for years. The Rust port (part of the FFM fraud library) raised design questions the
Java version had never answered explicitly:

- **What's the question?** "Accounts within 3 hops" is a *distance* question, and a depth-limited DFS answers it
  incorrectly: DFS can reach an account first through a long path, mark it visited, and then skip the short path to it,
  so a node 2 hops away can be recorded at depth 5 or missed by the limit. The team switched to **BFS**, which records
  every account at its true hop distance.
- **What bounds the frontier?** Fraud rings are dense: one shared device can link to thousands of accounts. BFS's
  frontier at hop 2 was measured on production-like data at up to ~80,000 accounts. The design caps the frontier
  (stop expanding and flag "too connected to score" beyond 100,000), which is itself a strong fraud signal.
- **Where does it run?** Scoring runs on the JVM's threads via FFM, whose stack sizes the Rust library doesn't control.
  That was one more reason not to recurse: the explicit `VecDeque` lives on the heap, and the library's behavior no
  longer depends on the caller's `-Xss`.
- **Allocation:** the queue and the `visited` set are reused per worker thread, with a capacity cap (Chapter 9.1's
  policy), so steady-state scoring allocates nothing for traversal.

### 10. Failure scenario

**The rule engine that took down the pool.** Meridian's merchant-onboarding service lets risk analysts upload
eligibility rules as nested JSON (`{"all": [{"any": [...]}, ...]}`), which a Rust service evaluates with a recursive
function on Tokio worker threads.

A partner integration generated rules programmatically and uploaded one nested about 40,000 levels deep (a
flattening bug in *their* tooling). The first evaluation overflowed a 2 MiB worker stack. The abort took down the whole
process: every in-flight request on every worker. Kubernetes restarted the pod, the rule was still in the database, the
next evaluation crashed it again, and the service was in a crash loop across all replicas within minutes.

- **Why tests didn't catch it:** the deepest test rule had 12 levels. And why parsing didn't catch it: the upload path
  used a hand-written parser without `serde_json`'s depth limit.
- **Immediate mitigation:** quarantine the rule (a manual database update), then deploy with the evaluator on a
  dedicated thread with a 256 MiB stack, while the real fix was written.
- **Real fix:** a depth limit of 64 enforced at upload (rejecting the rule with a clear error), and an evaluator
  rewritten with an explicit stack, so that even a rule that bypasses validation (from an old backup, say) fails one
  request with an error instead of aborting the process.
- **Lessons:** (1) input-controlled recursion depth is a denial-of-service vector; (2) in Rust a stack overflow is
  process-wide, so its blast radius is every request the process is serving; (3) a crash loop from persisted poison
  input needs a quarantine mechanism that doesn't require a deploy.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part IX).*

1. "Why BFS instead of DFS?" Answer it in the terms of this interlude, not only complexity: frontier order, frontier
   location, and what bounds its size.
2. How would you measure the stack frame size of a recursive function? Why does it differ between debug and release?
3. Walk through what happens, from the page fault to the abort message, when a Rust thread overflows its stack.
4. What are stack probes, and what would go wrong without them?
5. Why is the "stack of nodes, mark on push" loop not a DFS? For which problems does the difference matter?
6. Give the peak frontier memory for BFS and for explicit DFS on (a) a complete binary tree of depth 20 and (b) a chain
   of a million nodes.
7. Why does a depth-limited DFS give wrong answers to "all nodes within k hops"?
8. Name four places where the default stack size differs, and one operational consequence of that.
9. Java lets you catch `StackOverflowError`. Why isn't that the advantage it seems?
10. When is recursion the right choice in a Rust service?

### 12. Exercises

- **Beginner.** Run `intl-01-frame-size.rs` with an extra `[u64; 16]` local in `dfs` (use it through `black_box`).
  Predict the new release frame size and depth limit before running it.
- **Intermediate.** Implement cycle detection for a directed graph with the `(node, edge)` explicit stack, using three
  colors (unvisited, on stack, done). Verify it on a graph with a back edge and on a DAG. This completes Chapter 3.6's
  exercise.
- **Advanced.** Implement topological sort of a 1-million-node dependency chain three ways (recursive on a big-stack
  thread, explicit frames, and Kahn's algorithm with a `VecDeque`), and compare time and peak memory.
- **Systems.** Build two layouts of the same 2²¹-node binary tree: node IDs in BFS order and node IDs randomly permuted.
  Time BFS and explicit DFS on both. Which effect is bigger, the algorithm or the layout?
- **Architecture.** Find a recursive function in a service you know whose depth is controlled by input (a parser, a
  tree walk, a rule evaluator). Write down its current depth limit (probably none), the stack it runs on, and the
  blast radius of an overflow.

### 13. Debugging exercise

A command-line import tool runs fine on Linux servers, and the same input crashes it on a developer's Windows laptop
with:

```text
thread 'main' has overflowed its stack
```

The tool builds a 30,000-node singly linked structure (`Option<Box<Node>>`) in `main` and lets it drop at the end.
There's no recursion anywhere in the tool's own code.

1. Where is the recursion? (Hint: Chapter 3.5's drop glue, applied to a `Node` that owns the next `Option<Box<Node>>`.)
2. Why does it pass on Linux and fail on Windows?
3. Fix it in a way that works on every platform, and explain why the fix doesn't need a bigger stack.

### 14. Design exercise

**The dependency resolver.** Meridian's build tooling (written in Go today) resolves internal package dependencies:
about 40,000 packages, typical depth 15, and one pathological legacy chain of about 9,000. The team wants to rewrite it
in Rust as a library that is also called from a Tokio-based build server.

Design the traversal: the algorithm for resolution order and cycle detection with useful error messages (the cycle's
path), where the frontier lives, the depth and memory bounds, and how the library behaves when called from a 2 MiB
Tokio worker versus a CLI's main thread. Fill in the twelve-row decision matrix for your choice against the two
alternatives, and name the one measurement you'd take first.
