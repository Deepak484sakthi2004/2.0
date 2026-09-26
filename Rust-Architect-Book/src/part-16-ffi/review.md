# Part XVI Review — The Explain-API PR & Interview Mode

> Consolidate Part XVI, then use it: review a pull request that adds a C API to the fraud library. It compiles with one
> warning, its author's demo prints the right answers, and it contains more than a dozen boundary defects, only one of
> which the compiler mentions. Then answer senior-level questions without notes. Answers are in **Appendix A, Part
> XVI**.

---

## Part XVI on one page

```text
 ABI             four agreements: layout · calling convention · symbols · unwinding
                 repr(C) / repr(transparent) / repr(C, u32) · extern "C" · #[unsafe(no_mangle)] · "C" vs "C-unwind"
                 Rust's own ABI promises nothing (conv: Rust); nobody checks the agreement: the linker matches NAMES
                 System V: ≤16 B structs split into INTEGER/SSE eightbytes; larger → a copy on the stack (byval)
                 Option<&T>/Option<Box<T>>/Option<NonNull<T>>/Option<fn> = a nullable pointer (guaranteed)
                 Rust may SEND enums; it must RECEIVE integers (DecisionCode(u32) + TryFrom)
                 extern "C" can't unwind: rustc adds a landing pad → panic_cannot_unwind → abort (1.81+)
        │
 CALLING C       declaration (unsafe extern: "the signature is exact") · call (unsafe {}: "preconditions hold now")
                 · safe wrapper (types make the preconditions impossible to violate)
                 &CStr in · copy out (getenv) · io::Result from -1/errno (read errno FIRST) · Drop for anything freed
                 &mut self for "valid until the next call" · !Send for thread-affine · `safe fn` for no preconditions
                 a C callback that must be a total order (qsort_r) can't take arbitrary safe closures
                 lints: improper_ctypes, dangling_pointers_from_temporaries (deny both)
        │
 CALLING RUST    ten rules: prefix · unsafe extern "C" fn for pointers · catch_unwind everywhere · int codes
                 · opaque handles · concrete types · (ptr, len) checked · validate everything · Sync asserted · version
                 cdylib exports only #[no_mangle] items; executables export nothing (dlsym: undefined symbol)
                 FFM: FunctionDescriptor + downcallHandle + Arena; jextract from the cbindgen header; JNI = C glue
                 Modified UTF-8 is not UTF-8: validate, never repair
        │
 OWNERSHIP       who allocated · who frees (with which allocator) · how long valid · which threads
                 memory returns to the allocator that made it; whoever allocates exports the free
                 Box::into_raw ↔ from_raw · Vec::into_raw_parts ↔ from_raw_parts (same cap!) · Option<Box<T>> destroy
                 caller buffer + size query | library buffer + *_free | borrowed during a callback
                 handles: pointer (UB on misuse) | exposed-provenance integer (UB) | registry + generation (error code)
                 thread-affine resource → !Send + owner thread + bounded channel + one-shot replies
```

## Ten ideas to carry forward

1. **An ABI is a contract nobody checks.** Layout, registers, names, and unwinding must match on both sides, and the
   only enforcement is what you build: one generated header, `const` layout assertions, and a version handshake at load.
2. **`repr(C)` marks a boundary.** Use it only where something outside Rust reads the bytes, and pin every offset with
   an assertion the day you add it.
3. **Send enums, receive integers.** A value that arrives from outside carries no validity guarantee, so it enters
   Rust as an integer and becomes an enum only through `TryFrom`.
4. **A safe wrapper's signature is its proof.** `&CStr`, slices, owned return values, `&mut self`, `Drop`, and `!Send`
   each discharge one clause of a C contract, so callers can't break it without writing `unsafe`.
5. **Unsafe code can't trust safe behavior it didn't write.** A comparator handed to `qsort_r`, an `Ord` impl, a
   callback: if C's contract makes misbehavior undefined, the wrapper passes data, not arbitrary code.
6. **Panics stop at the waist.** `catch_unwind` in every export, a code for "our bug," and `resume_unwind` only once
   you're back in Rust frames. `C-unwind` is for callers built to receive an unwind, never a JVM.
7. **Exported pointer-takers are `unsafe extern "C" fn`.** C ignores the keyword. Rust callers need it.
8. **Whoever allocates exports the free.** "Free with `free()`" leaks an implementation detail into a contract and
   breaks the day the allocator changes.
9. **Make misuse defined where you can.** A registry handle turns a double close into `-1`. A pointer handle turns it
   into undefined behavior. Choose by who the caller is.
10. **Thread rules are types.** `!Send` for "only the creating thread," a `Sync` assertion for "safe from many
    threads," and an owner thread when both must be true at once.

---

## Capstone: review the explain-API PR

The risk console (a Java web app) wants to show analysts *why* a transaction scored the way it did. A teammate opens a
PR against the fraud library: "fraud: expose explanations to the risk console (FFM)." It adds three exports. The Java
side will call `init` once at startup, `explain` for each transaction an analyst opens, and `last_explanation` for the
console's "copy" button (listing `review-01-explain-pr.rs`, verified: it compiles, and the author's demo runs):

```rust,ignore
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char};
use std::sync::Mutex;

pub struct Explainer {
    weights: HashMap<String, f64>,
    last: CString,
}

static EXPLAINER: Mutex<Option<Explainer>> = Mutex::new(None);

pub struct ExplainOptions {
    pub top: usize,
    pub include_negative: bool,
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq)]
pub enum Format {
    Text = 0,
    Json = 1,
}

/// Loads the model. Returns false on failure.
#[unsafe(no_mangle)]
pub extern "C" fn init(model_name: String) -> bool {
    let weights = HashMap::from([
        ("amount".to_string(), 0.41),
        ("velocity".to_string(), 0.22),
        ("country".to_string(), -0.05),
    ]);
    *EXPLAINER.lock().unwrap() = Some(Explainer { weights, last: CString::default() });
    !model_name.is_empty()
}

/// Returns a newly allocated explanation. The caller frees it with free().
#[unsafe(no_mangle)]
pub extern "C" fn explain(features: *const f64, n: i32, opts: *const ExplainOptions, format: Format) -> *mut c_char {
    let features = unsafe { std::slice::from_raw_parts(features, n as usize) };
    let opts = unsafe { &*opts };
    let mut guard = EXPLAINER.lock().unwrap();
    let e = guard.as_mut().unwrap();
    let names = ["amount", "velocity", "country"];
    let mut parts: Vec<(&str, f64)> = names.iter().zip(features).map(|(n, x)| (*n, x * e.weights[*n])).collect();
    parts.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    parts.retain(|p| opts.include_negative || p.1 >= 0.0);
    parts.truncate(opts.top);
    let text = match format {
        Format::Text => parts.iter().map(|(n, v)| format!("{n} {v:+.2}")).collect::<Vec<_>>().join(", "),
        Format::Json => format!(
            "[{}]",
            parts.iter().map(|(n, v)| format!("{{\"f\":\"{n}\",\"v\":{v:.2}}}")).collect::<Vec<_>>().join(",")
        ),
    };
    e.last = CString::new(text.clone()).unwrap();
    CString::new(text).unwrap().into_raw()
}

/// The last explanation, for the console's "copy" button.
#[unsafe(no_mangle)]
pub extern "C" fn last_explanation() -> *const c_char {
    EXPLAINER.lock().unwrap().as_ref().unwrap().last.as_ptr()
}
```

The author's demo calls each function correctly from Rust and prints:

```text
text: amount +0.37, velocity +0.11
json: [{"f":"amount","v":0.37},{"f":"velocity","v":0.11}]
last: [{"f":"amount","v":0.37},{"f":"velocity","v":0.11}]
```

The build printed exactly one warning:

```text
warning: `extern` fn uses type `String`, which is not FFI-safe
  --> src/main.rs:30:36
   |
30 | pub extern "C" fn init(model_name: String) -> bool {
   |                                    ^^^^^^ not FFI-safe
   |
   = help: consider adding a `#[repr(C)]` or `#[repr(transparent)]` attribute to this struct
   = note: this struct has unspecified layout
```

Under Meridian's CI setting for FFI crates, the lint is denied, and the same function fails to compile (listing
`review-02-pr-lint.rs`, verified: the same message, as an `error:`). The PR's own CI job
didn't deny it, so the warning scrolled past.

**Your review.**

1. Find **at least twelve** defects. For each, name the Part XVI chapter (or the earlier chapter) whose rule it breaks,
   and say what the Java side or the on-call engineer would see. Where a defect breaks a contract, say in one sentence
   what goes wrong, and point to the chapter that explains why it's undefined; don't try to reproduce it.
2. The compiler flagged `init`. Why did it say nothing about `ExplainOptions`, which has no `repr(C)` at all? About
   `format: Format`? (Chapter 16.1's debugging exercise and listing `ch01-11-lint-blind-spot.rs` are relevant.)
3. The demo passes. List the defects that a demo written in Rust *can't* reveal, and explain why: what's different
   about a caller that is Java?
4. Answer Chapter 16.4's four questions for every pointer the API returns or accepts. Which answers does the header
   comment get wrong, and which does it leave out?
5. Suppose the author adds `catch_unwind` to every function and changes nothing else. Which failure mode gets
   *worse*? (Hint: what happens to a `Mutex` when a panic occurs while it's locked, Chapter 11.3.)
6. Rewrite the API. State the handle design, the error convention, how options and formats are encoded, how the
   explanation is returned and freed, and what replaces `last_explanation`.

A rewrite (listing `review-03-explain-fixed.rs`, verified in debug, under Miri, and with 3 tests passing) keeps the
`meridian_` prefix and the conventions of Chapters 16.1–16.4:

```rust,ignore
/// C: `typedef struct { uint32_t top; uint32_t format; uint8_t include_negative; } MeridianExplainOptions;`
#[repr(C)]
pub struct MeridianExplainOptions {
    pub top: u32,
    pub format: u32,           // MERIDIAN_FORMAT_*: validated, never transmuted into an enum
    pub include_negative: u8,  // 0 or 1: validated, not a Rust bool
}
const _: () = {
    assert!(size_of::<MeridianExplainOptions>() == 12);
    assert!(offset_of!(MeridianExplainOptions, include_negative) == 8);
};

/// Opaque to C. Immutable after creation, so it's Sync: no global lock on the scoring path.
pub struct MeridianExplainer {
    names: Vec<&'static str>,
    weights: Vec<f64>,
}
const _: () = {
    const fn sync<T: Sync>() {}
    sync::<MeridianExplainer>();
};

/// # Safety
/// `e` is a live explainer (shared use from many threads is fine); `features` points to `n` f64s
/// (NULL allowed when n == 0); `opts` and `out` are NULL or valid. On MERIDIAN_OK, `*out` owns a
/// buffer to be released with `meridian_buf_free`; on any error `*out` is left untouched.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_explain(
    e: *const MeridianExplainer,
    features: *const f64,
    n: usize,
    opts: *const MeridianExplainOptions,
    out: *mut MeridianBuf,
) -> i32 {
```

Its `main` plays the Java caller, including the inputs the PR would have trusted:

```text
abi 4 | explainer_new -> 0
text          -> Ok("amount +0.37, velocity +0.11")
json          -> Ok("[{\"f\":\"amount\",\"v\":0.37},{\"f\":\"velocity\",\"v\":0.11}]")
format 7      -> Err(-1)
NaN feature   -> Err(-1)
2 features    -> Err(-1)
NULL pointers -> -1
```

and its tests check that unknown encodings are rejected, that four threads can share one explainer, and that an empty
model name is refused:

```text
test tests::empty_model_name_is_invalid ... ok
test tests::rejects_unknown_encodings_instead_of_trusting_them ... ok
test tests::many_threads_share_one_explainer ... ok
```

Write your review first, then compare it with the rewrite and the model answers. The rewrite is one defensible design,
not the only one: a caller-buffer version (`meridian_explain_into`, Chapter 16.4) is equally reasonable for a console
that shows short texts.

---

## Interview mode

*Senior-level. Answer aloud or in writing, without notes, before checking Appendix A.*

### Language

1. What does Rust guarantee about the layout and ABI of `#[repr(C)]` structs, `#[repr(transparent)]` wrappers, and
   `Option<&T>`? What does it explicitly *not* guarantee about a struct with the default representation?
2. Why are `unsafe extern` blocks and `#[unsafe(no_mangle)]` marked unsafe in edition 2024, when neither executes any
   code? What is a `safe` item in an extern block, and when is declaring one a soundness bug?
3. An exported function takes `*mut T` and writes through it. Should it be `extern "C" fn` or `unsafe extern "C" fn`?
   Argue from the definition of soundness, and say who is affected by the choice.

### Compiler and runtime

4. Explain `conv: Rust` versus `conv: C` in a `#[rustc_abi(debug)]` dump for a 24-byte struct argument, and connect
   each to the assembly the caller generates.
5. What does rustc emit for an `extern "C"` function that may panic? What happens at run time when it does, and what
   changed in Rust 1.81?
6. What does a call through an `unsafe extern` declaration compile to on x86-64 Linux, and why can't LLVM optimize
   across it the way it optimizes a call to another Rust function?

### Performance

7. A Java service calls a Rust function through FFM 2 million times per second, each call doing about 50 ns of work.
   What do you predict about the overhead, what would you change in the API, and how would you measure it?
8. Compare the costs of returning data through a caller buffer, a library buffer with a free function, and a callback,
   for small results and for large ones.

### Architecture

9. Design the C API of a Rust library that will be called from Java, Go, and C++. Walk through the ten rules and say
   which ones each caller's runtime makes harder.
10. A vendor C library is thread-affine and not reentrant. Your service runs 64 worker threads. Design the Rust
    integration end to end, including failure handling.
11. JNI, FFM, or a sidecar process for a native library with a 5 ms p99 budget and occasional crashes? What would make
    you change your answer?
12. How do you keep a C header, a Rust crate, and Java bindings in agreement across releases and rolling deploys?

### Operations

13. A JVM that loads a Rust library dies with no Java stack trace. List what you'd check, in order, and which Part XVI
    failure each check would confirm.
14. Write the review checklist you'd apply to any PR that adds or changes an exported C function, in twelve lines or
    fewer.
