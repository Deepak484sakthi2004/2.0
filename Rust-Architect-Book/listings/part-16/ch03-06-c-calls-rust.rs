// verify: debug ok
// Calling Rust from C, for real, inside the Playground container: this program writes the fraud
// library's Rust source, its C header, and a C client to /tmp; builds the library as a `cdylib` with
// rustc; compiles and links the client with gcc (-Wall -Wextra -Werror); runs it; and lists the
// library's exported symbols with `nm -D`. Every inner step must succeed, or this program panics.
use std::process::Command;

/// The library: the C API of listing ch03-01 (same rules), compiled as a real shared library.
const LIB_RS: &str = r##"
use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};

pub const MERIDIAN_OK: i32 = 0;
pub const MERIDIAN_ERR_INVALID: i32 = -1;
pub const MERIDIAN_ERR_PANIC: i32 = -99;

#[repr(C)]
pub struct MeridianConfig {
    pub block_at: u32,
    pub review_at: u32,
}

pub struct MeridianScorer {
    weights: [f64; 3],
    rows: HashMap<u64, [f64; 3]>,
}
const _: () = {
    const fn assert_sync<T: Sync>() {}
    assert_sync::<MeridianScorer>(); // the header promises: thread-safe
};

struct Invalid;

fn ffi_guard(f: impl FnOnce() -> Result<(), Invalid>) -> i32 {
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => MERIDIAN_OK,
        Ok(Err(Invalid)) => MERIDIAN_ERR_INVALID,
        Err(_) => MERIDIAN_ERR_PANIC,
    }
}

/// # Safety
/// If `n > 0` and `p` is non-null, `p` points to `n` readable `T`s, valid for `'a`.
unsafe fn slice_in<'a, T>(p: *const T, n: usize) -> Result<&'a [T], Invalid> {
    if n == 0 {
        return Ok(&[]);
    }
    if p.is_null() || !p.is_aligned() || n > isize::MAX as usize / size_of::<T>() {
        return Err(Invalid);
    }
    // SAFETY: checked above; readable for `n` (the caller's contract).
    Ok(unsafe { std::slice::from_raw_parts(p, n) })
}

/// # Safety
/// As `slice_in`, but writable and not accessed through any other pointer for `'a`.
unsafe fn slice_out<'a, T>(p: *mut T, n: usize) -> Result<&'a mut [T], Invalid> {
    if n == 0 {
        return Ok(&mut []);
    }
    if p.is_null() || !p.is_aligned() || n > isize::MAX as usize / size_of::<T>() {
        return Err(Invalid);
    }
    // SAFETY: as in slice_in, plus exclusivity (the caller's contract).
    Ok(unsafe { std::slice::from_raw_parts_mut(p, n) })
}

#[unsafe(no_mangle)]
pub extern "C" fn meridian_abi_version() -> u32 {
    3
}

/// # Safety
/// `cfg` is NULL or a valid MeridianConfig; `out` is NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_scorer_new(cfg: *const MeridianConfig, out: *mut *mut MeridianScorer) -> i32 {
    ffi_guard(|| {
        // SAFETY: NULL or valid (the contract).
        let cfg = unsafe { cfg.as_ref() }.ok_or(Invalid)?;
        if out.is_null() || cfg.review_at > cfg.block_at || cfg.block_at > 100 {
            return Err(Invalid);
        }
        let rows = HashMap::from([(1001, [0.9, 0.8, 0.7]), (1002, [0.1, 0.2, 0.1]), (1003, [0.6, 0.5, 0.2])]);
        let s = Box::new(MeridianScorer { weights: [0.5, 0.3, 0.2], rows });
        // SAFETY: non-null (checked) and writable (the contract).
        unsafe { out.write(Box::into_raw(s)) };
        Ok(())
    })
}

/// # Safety
/// `s` is NULL (no-op) or came from meridian_scorer_new and is not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_scorer_free(s: *mut MeridianScorer) {
    if !s.is_null() {
        // SAFETY: from Box::into_raw in meridian_scorer_new, released once.
        drop(unsafe { Box::from_raw(s) });
    }
}

/// # Safety
/// `s` from meridian_scorer_new; `ids`/`out` point to `n` elements (NULL allowed when n == 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_score_batch(s: *const MeridianScorer, ids: *const u64, n: usize, out: *mut i32) -> i32 {
    ffi_guard(|| {
        // SAFETY: NULL or a live scorer (the contract).
        let s = unsafe { s.as_ref() }.ok_or(Invalid)?;
        // SAFETY: the contract on ids/out; NULL, alignment and size checked inside.
        let (ids, out) = unsafe { (slice_in(ids, n)?, slice_out(out, n)?) };
        for (id, slot) in ids.iter().zip(out.iter_mut()) {
            let f = s.rows[id]; // panics for an unknown id: the contained-bug path
            let v: f64 = f.iter().zip(&s.weights).map(|(x, w)| x * w).sum();
            *slot = (v * 100.0).round() as i32;
        }
        Ok(())
    })
}
"##;

/// The header C sees, with the C side's own layout assertions.
const HEADER_H: &str = r##"#ifndef MERIDIAN_FRAUD_H
#define MERIDIAN_FRAUD_H
#include <stddef.h>
#include <stdint.h>

#define MERIDIAN_ABI_VERSION 3
#define MERIDIAN_OK 0
#define MERIDIAN_ERR_INVALID -1
#define MERIDIAN_ERR_PANIC -99

typedef struct MeridianScorer MeridianScorer; /* opaque */

typedef struct MeridianConfig {
    uint32_t block_at;  /* 0..=100 */
    uint32_t review_at; /* <= block_at */
} MeridianConfig;
_Static_assert(sizeof(MeridianConfig) == 8, "MeridianConfig layout changed");
_Static_assert(offsetof(MeridianConfig, review_at) == 4, "MeridianConfig layout changed");

uint32_t meridian_abi_version(void);
int32_t meridian_scorer_new(const MeridianConfig *cfg, MeridianScorer **out);
void meridian_scorer_free(MeridianScorer *s); /* NULL is a no-op; free each scorer once */
/* Thread-safe. ids[n] and out[n] must not overlap. On error, out is unspecified. */
int32_t meridian_score_batch(const MeridianScorer *s, const uint64_t *ids, size_t n, int32_t *out);
#endif
"##;

/// A C caller that follows the header.
const CLIENT_C: &str = r##"#include <stdio.h>
#include "meridian_fraud.h"

int main(void) {
    if (meridian_abi_version() != MERIDIAN_ABI_VERSION) {
        fprintf(stderr, "meridian ABI mismatch\n");
        return 1;
    }
    MeridianConfig cfg = { .block_at = 80, .review_at = 50 };
    MeridianScorer *s = NULL;
    printf("scorer_new(80/50)         -> rc=%d\n", meridian_scorer_new(&cfg, &s));

    uint64_t ids[3] = { 1001, 1002, 1003 };
    int32_t out[3] = { 0 };
    int32_t rc = meridian_score_batch(s, ids, 3, out);
    printf("score_batch(3 ids)        -> rc=%d scores=[%d, %d, %d]\n", rc, out[0], out[1], out[2]);
    printf("score_batch(NULL, n=3)    -> rc=%d\n", meridian_score_batch(s, NULL, 3, out));
    printf("score_batch(NULL, n=0)    -> rc=%d\n", meridian_score_batch(s, NULL, 0, NULL));

    uint64_t with_unknown[2] = { 1001, 4242 };
    printf("score_batch([1001, 4242]) -> rc=%d\n", meridian_score_batch(s, with_unknown, 2, out));

    MeridianConfig bad = { .block_at = 40, .review_at = 60 };
    MeridianScorer *other = NULL;
    rc = meridian_scorer_new(&bad, &other);
    printf("scorer_new(40/60)         -> rc=%d, out left NULL: %s\n", rc, other == NULL ? "yes" : "no");

    meridian_scorer_free(s);
    meridian_scorer_free(NULL);
    printf("C main returns normally after a contained Rust panic\n");
    return 0;
}
"##;

/// Runs a command and insists that it succeeded; returns (stdout, stderr).
fn run(what: &str, cmd: &mut Command) -> (String, String) {
    let out = cmd.output().unwrap_or_else(|e| panic!("{what}: could not start: {e}"));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{what} failed ({}):\n{stderr}", out.status);
    (stdout, stderr)
}

fn main() {
    let dir = "/tmp/meridian-16-3";
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(format!("{dir}/meridian_fraud.rs"), LIB_RS).unwrap();
    std::fs::write(format!("{dir}/meridian_fraud.h"), HEADER_H).unwrap();
    std::fs::write(format!("{dir}/client.c"), CLIENT_C).unwrap();

    let lib = format!("{dir}/libmeridian_fraud.so");
    run(
        "rustc (cdylib)",
        Command::new("rustc").args(["--edition", "2024", "--crate-type", "cdylib", "--crate-name", "meridian_fraud"])
            .args(["-C", "opt-level=2", "-o", &lib, &format!("{dir}/meridian_fraud.rs")]),
    );
    run(
        "gcc",
        Command::new("gcc").args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-o", &format!("{dir}/client")])
            .args([&format!("{dir}/client.c"), "-L", dir, "-lmeridian_fraud", &format!("-Wl,-rpath,{dir}")]),
    );
    println!("built {lib} (rustc) and client (gcc -Werror): OK\n");

    let (out, err) = run("the C client", &mut Command::new(format!("{dir}/client")));
    print!("{out}");
    for line in err.lines().filter(|l| !l.trim().is_empty()).take(2) {
        println!("  client stderr | {line}");
    }

    let (syms, _) = run("nm", Command::new("nm").args(["-D", "--defined-only", &lib]));
    println!("\nnm -D --defined-only libmeridian_fraud.so:");
    for line in syms.lines() {
        println!("  {line}");
    }
}
