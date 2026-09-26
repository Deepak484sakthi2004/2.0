// verify: debug ok
// Chapter 16.4's ownership contracts from the C side, for real: this program builds the Rust
// library as a `cdylib`, compiles a C client that follows every rule in the header (who allocates,
// who frees, with which function, for how long), and runs it twice: normally, and under
// AddressSanitizer + LeakSanitizer. Both runs must succeed and the sanitizers must report nothing.
use std::process::Command;

const LIB_RS: &str = r##"
use std::ffi::{c_char, c_void, CStr};
use std::panic::{self, AssertUnwindSafe};

pub const MERIDIAN_OK: i32 = 0;
pub const MERIDIAN_ERR_INVALID: i32 = -1;
pub const MERIDIAN_ERR_TOO_SMALL: i32 = -2;
pub const MERIDIAN_ERR_PANIC: i32 = -99;

pub struct MeridianScorer {
    block_at: u32,
}

#[unsafe(no_mangle)]
pub extern "C" fn meridian_scorer_create(block_at: u32) -> Option<Box<MeridianScorer>> {
    (block_at <= 100).then(|| Box::new(MeridianScorer { block_at }))
}

#[unsafe(no_mangle)]
pub extern "C" fn meridian_scorer_destroy(s: Option<Box<MeridianScorer>>) {
    drop(s);
}

#[unsafe(no_mangle)]
pub extern "C" fn meridian_scorer_block_at(s: &MeridianScorer) -> u32 {
    s.block_at
}

fn explain(txn: u64) -> Option<String> {
    (txn != 0).then(|| format!("txn {txn}: amount +0.41, velocity +0.22, country -0.05"))
}

fn guard(f: impl FnOnce() -> i32) -> i32 {
    panic::catch_unwind(AssertUnwindSafe(f)).unwrap_or(MERIDIAN_ERR_PANIC)
}

/// # Safety
/// `buf` writable for `cap` bytes (NULL allowed if cap == 0); `needed` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_explain_into(txn: u64, buf: *mut u8, cap: usize, needed: *mut usize) -> i32 {
    guard(|| {
        let Some(text) = explain(txn) else { return MERIDIAN_ERR_INVALID };
        if needed.is_null() || (cap > 0 && buf.is_null()) {
            return MERIDIAN_ERR_INVALID;
        }
        // SAFETY: non-null and writable (the contract).
        unsafe { needed.write(text.len()) };
        if text.len() > cap {
            return MERIDIAN_ERR_TOO_SMALL;
        }
        // SAFETY: `buf` writable for cap >= len bytes; can't overlap our String.
        unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), buf, text.len()) };
        MERIDIAN_OK
    })
}

#[repr(C)]
pub struct MeridianBuf {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

/// # Safety
/// `out` writable. On MERIDIAN_OK, `*out` must be released with `meridian_buf_free`, once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_explain(txn: u64, out: *mut MeridianBuf) -> i32 {
    guard(|| {
        let Some(text) = explain(txn) else { return MERIDIAN_ERR_INVALID };
        if out.is_null() {
            return MERIDIAN_ERR_INVALID;
        }
        let (ptr, len, cap) = text.into_bytes().into_raw_parts();
        // SAFETY: non-null and writable (the contract).
        unsafe { out.write(MeridianBuf { ptr, len, cap }) };
        MERIDIAN_OK
    })
}

/// # Safety
/// `buf` came from `meridian_explain`, unmodified, freed once. A NULL `ptr` is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_buf_free(buf: MeridianBuf) {
    if !buf.ptr.is_null() {
        // SAFETY: the triple came from Vec::into_raw_parts and comes back exactly once.
        drop(unsafe { Vec::from_raw_parts(buf.ptr, buf.len, buf.cap) });
    }
}

pub type FeatureCb = unsafe extern "C" fn(user: *mut c_void, name: *const c_char, value: f64) -> i32;

/// # Safety
/// `cb`, if non-null, is safe to call with `user` and a NUL-terminated `name` valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_for_each_feature(txn: u64, cb: Option<FeatureCb>, user: *mut c_void) -> i32 {
    let Some(cb) = cb else { return MERIDIAN_ERR_INVALID };
    for (name, weight) in [(c"amount", 0.41), (c"velocity", 0.22), (c"country", -0.05)] {
        // A fresh CString per call: the name really is valid only during the callback.
        let owned = CStr::to_owned(name);
        // SAFETY: the caller's contract on cb/user; `owned` outlives the call.
        let rc = unsafe { cb(user, owned.as_ptr(), weight * (txn % 10) as f64) };
        if rc != 0 {
            return rc;
        }
    }
    MERIDIAN_OK
}
"##;

const HEADER_H: &str = r##"#ifndef MERIDIAN_OWNERSHIP_H
#define MERIDIAN_OWNERSHIP_H
#include <stddef.h>
#include <stdint.h>

#define MERIDIAN_OK 0
#define MERIDIAN_ERR_INVALID -1
#define MERIDIAN_ERR_TOO_SMALL -2

typedef struct MeridianScorer MeridianScorer;
/* Owned by the caller until passed to meridian_scorer_destroy (once; NULL is a no-op). */
MeridianScorer *meridian_scorer_create(uint32_t block_at);
void meridian_scorer_destroy(MeridianScorer *s);
uint32_t meridian_scorer_block_at(const MeridianScorer *s);

/* (1) Caller-allocated: writes at most cap bytes, no NUL; on TOO_SMALL, *needed says how many. */
int32_t meridian_explain_into(uint64_t txn, uint8_t *buf, size_t cap, size_t *needed);

/* (2) Library-allocated: read ptr[0..len]; don't modify; release with meridian_buf_free, once. */
typedef struct MeridianBuf { uint8_t *ptr; size_t len; size_t cap; } MeridianBuf;
_Static_assert(sizeof(MeridianBuf) == 24, "MeridianBuf layout changed");
int32_t meridian_explain(uint64_t txn, MeridianBuf *out);
void meridian_buf_free(MeridianBuf buf);

/* (3) Borrowed during a callback: `name` is valid only until the callback returns; copy what you keep.
       `user` is passed through untouched. Return 0 to continue, non-zero to stop. */
typedef int32_t (*meridian_feature_cb)(void *user, const char *name, double value);
int32_t meridian_for_each_feature(uint64_t txn, meridian_feature_cb cb, void *user);
#endif
"##;

const CLIENT_C: &str = r##"#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "meridian_ownership.h"

typedef struct { char name[32]; double value; int seen; } Best;

static int32_t keep_best(void *user, const char *name, double value) {
    Best *best = user;                      /* our own struct, handed back untouched */
    best->seen++;
    if (value > best->value) {
        best->value = value;
        snprintf(best->name, sizeof best->name, "%s", name); /* copy: `name` dies after we return */
    }
    return 0;
}

int main(void) {
    MeridianScorer *s = meridian_scorer_create(80);
    printf("scorer block_at = %u; create(150) is NULL: %s\n",
           meridian_scorer_block_at(s), meridian_scorer_create(150) == NULL ? "yes" : "no");

    /* (1) caller-allocated: C's stack, then C's heap, freed by C's free() */
    uint8_t small[16];
    size_t needed = 0;
    int32_t rc = meridian_explain_into(7001, small, sizeof small, &needed);
    printf("(1) explain_into(cap=16) -> rc=%d, needed=%zu\n", rc, needed);
    uint8_t *exact = malloc(needed);
    if (exact == NULL) return 1;
    rc = meridian_explain_into(7001, exact, needed, &needed);
    printf("    explain_into(cap=%zu) -> rc=%d, \"%.*s\"\n", needed, rc, (int)needed, (const char *)exact);
    free(exact);

    /* (2) library-allocated: read it, then give it back to the library */
    MeridianBuf buf = { 0 };
    rc = meridian_explain(7002, &buf);
    printf("(2) explain -> rc=%d, len=%zu cap=%zu, \"%.*s\"\n", rc, buf.len, buf.cap, (int)buf.len, (const char *)buf.ptr);
    meridian_buf_free(buf);

    /* (3) borrowed during a callback, with our own user data */
    Best best = { .value = -1e9 };
    rc = meridian_for_each_feature(7, keep_best, &best);
    printf("(3) for_each_feature -> rc=%d, saw %d, largest: %s=%.2f\n", rc, best.seen, best.name, best.value);

    meridian_scorer_destroy(s);
    meridian_scorer_destroy(NULL);
    printf("every allocation returned to the allocator that made it\n");
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
    let dir = "/tmp/meridian-16-4";
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(format!("{dir}/meridian_ownership.rs"), LIB_RS).unwrap();
    std::fs::write(format!("{dir}/meridian_ownership.h"), HEADER_H).unwrap();
    std::fs::write(format!("{dir}/client.c"), CLIENT_C).unwrap();
    let lib = format!("{dir}/libmeridian_ownership.so");
    run(
        "rustc (cdylib)",
        Command::new("rustc").args(["--edition", "2024", "--crate-type", "cdylib", "--crate-name", "meridian_ownership"])
            .args(["-o", &lib, &format!("{dir}/meridian_ownership.rs")]),
    );
    let link = ["-L", dir, "-lmeridian_ownership", &format!("-Wl,-rpath,{dir}")];
    run(
        "gcc",
        Command::new("gcc").args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-o", &format!("{dir}/client")])
            .arg(format!("{dir}/client.c")).args(link),
    );
    run(
        "gcc (ASan + LSan)",
        Command::new("gcc").args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-g", "-fsanitize=address"])
            .args(["-o", &format!("{dir}/client-asan")]).arg(format!("{dir}/client.c")).args(link),
    );

    let (out, _) = run("the C client", &mut Command::new(format!("{dir}/client")));
    print!("{out}");

    let (asan_out, asan_err) = run(
        "the C client under ASan + LSan",
        Command::new(format!("{dir}/client-asan")).env("ASAN_OPTIONS", "detect_leaks=1:halt_on_error=1"),
    );
    assert_eq!(asan_out, out, "same behavior under the sanitizers");
    assert!(!asan_err.contains("Sanitizer"), "sanitizer report:\n{asan_err}");
    println!("\nunder AddressSanitizer + LeakSanitizer: exit 0, same output, no reports");
}
