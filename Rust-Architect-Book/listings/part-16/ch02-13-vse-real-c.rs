// verify: debug ok
// Chapter 16.2's vendor-engine binding against REAL C: this program writes the vendor's header and a C
// implementation of `libvse` (thread-affine: it records the opening thread), builds it with
// `gcc -shared`, then builds the Rust binding (the `sys` + safe `Engine` layers of listing ch02-11,
// now with `#[link(name = "vse")]`) with rustc, and runs it. Every inner step must succeed.
use std::process::Command;

const VSE_H: &str = r##"#ifndef VSE_H
#define VSE_H
#include <stddef.h>

#define VSE_OK 0
#define VSE_EINVAL 1
#define VSE_ENOMODEL 2
#define VSE_EWRONGTHREAD 3

typedef struct vse_engine vse_engine;
int vse_version(void);
int vse_open(const char *path, vse_engine **out);            /* use the engine on this thread only */
int vse_score(vse_engine *e, const double *features, size_t n, double *out);
const char *vse_last_error(const vse_engine *e);             /* valid until the next call on e */
void vse_close(vse_engine *e);
#endif
"##;

const VSE_C: &str = r##"#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "vse.h"

struct vse_engine {
    pthread_t owner;
    double weights[3];
    char last_error[128];
};

int vse_version(void) { return 402; }

int vse_open(const char *path, vse_engine **out) {
    size_t n = strlen(path);
    if (n < 4 || strcmp(path + n - 4, ".vse") != 0) return VSE_ENOMODEL;
    vse_engine *e = calloc(1, sizeof *e);
    if (e == NULL) return VSE_EINVAL;
    e->owner = pthread_self();
    e->weights[0] = 0.5; e->weights[1] = 0.3; e->weights[2] = 0.2;
    *out = e;
    return VSE_OK;
}

int vse_score(vse_engine *e, const double *features, size_t n, double *out) {
    if (!pthread_equal(e->owner, pthread_self())) {
        snprintf(e->last_error, sizeof e->last_error, "engine used from a thread other than the one that opened it");
        return VSE_EWRONGTHREAD;
    }
    if (n != 3) {
        snprintf(e->last_error, sizeof e->last_error, "expected 3 features, got %zu", n);
        return VSE_EINVAL;
    }
    double s = 0.0;
    for (size_t i = 0; i < n; i++) s += features[i] * e->weights[i];
    *out = s;
    return VSE_OK;
}

const char *vse_last_error(const vse_engine *e) { return e->last_error; }

void vse_close(vse_engine *e) { free(e); }
"##;

/// The binding: listing ch02-11's two layers, minus the simulated vendor module, plus `#[link]`.
const BINDING_RS: &str = r##"
use std::ffi::{CStr, CString, c_int};
use std::marker::PhantomData;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{self, NonNull};

mod sys {
    use std::ffi::{c_char, c_int};
    use std::marker::{PhantomData, PhantomPinned};

    #[repr(C)]
    pub struct VseEngine {
        _private: [u8; 0],
        _marker: PhantomData<(*mut u8, PhantomPinned)>,
    }

    pub const VSE_OK: c_int = 0;

    #[link(name = "vse")]
    unsafe extern "C" {
        pub safe fn vse_version() -> c_int;
        pub fn vse_open(path: *const c_char, out: *mut *mut VseEngine) -> c_int;
        pub fn vse_score(e: *mut VseEngine, features: *const f64, n: usize, out: *mut f64) -> c_int;
        pub fn vse_last_error(e: *const VseEngine) -> *const c_char;
        pub fn vse_close(e: *mut VseEngine);
    }
}

#[derive(Debug)]
pub enum VseError {
    PathContainsNul,
    Open { code: c_int },
    Score { code: c_int, message: String },
}

/// INVARIANT: a live engine from vse_open. !Send + !Sync: the vendor requires one thread per engine.
pub struct Engine {
    raw: NonNull<sys::VseEngine>,
    _thread_affine: PhantomData<*const ()>,
}

impl Engine {
    pub fn open(model: &Path) -> Result<Engine, VseError> {
        let path = CString::new(model.as_os_str().as_bytes()).map_err(|_| VseError::PathContainsNul)?;
        let mut raw = ptr::null_mut();
        // SAFETY: `path` is NUL-terminated and lives across the call; `&mut raw` is writable.
        let rc = unsafe { sys::vse_open(path.as_ptr(), &mut raw) };
        if rc != sys::VSE_OK {
            return Err(VseError::Open { code: rc });
        }
        Ok(Engine { raw: NonNull::new(raw).expect("VSE_OK with NULL"), _thread_affine: PhantomData })
    }

    pub fn score(&mut self, features: &[f64]) -> Result<f64, VseError> {
        let mut out = 0.0;
        // SAFETY: a live engine on its opening thread (!Send); `features` readable; `out` writable.
        let rc = unsafe { sys::vse_score(self.raw.as_ptr(), features.as_ptr(), features.len(), &mut out) };
        if rc == sys::VSE_OK {
            return Ok(out);
        }
        // SAFETY: valid until the next call on this engine; copied before one can happen (&mut self).
        let msg = unsafe { CStr::from_ptr(sys::vse_last_error(self.raw.as_ptr())) };
        Err(VseError::Score { code: rc, message: msg.to_string_lossy().into_owned() })
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: the invariant; closed once.
        unsafe { sys::vse_close(self.raw.as_ptr()) }
    }
}

fn main() {
    println!("vse_version() = {} (a `safe` foreign item, implemented in C)", sys::vse_version());
    println!("open(missing.bin): {:?}", Engine::open(Path::new("/models/missing.bin")).err());
    let mut engine = Engine::open(Path::new("/models/fraud-2026-09.vse")).expect("open");
    println!("score([0.9, 0.5, 0.1]) = {:?}", engine.score(&[0.9, 0.5, 0.1]));
    println!("score([0.9]) = {:?}", engine.score(&[0.9]));
}
"##;

/// Runs a command and insists that it succeeded; returns its stdout.
fn run(what: &str, cmd: &mut Command) -> String {
    let out = cmd.output().unwrap_or_else(|e| panic!("{what}: could not start: {e}"));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{what} failed ({}):\n{stderr}", out.status);
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn main() {
    let dir = "/tmp/meridian-16-2";
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(format!("{dir}/vse.h"), VSE_H).unwrap();
    std::fs::write(format!("{dir}/vse.c"), VSE_C).unwrap();
    std::fs::write(format!("{dir}/binding.rs"), BINDING_RS).unwrap();
    run(
        "gcc (libvse.so)",
        Command::new("gcc").args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-shared", "-fPIC", "-pthread"])
            .args(["-o", &format!("{dir}/libvse.so"), &format!("{dir}/vse.c")]),
    );
    run(
        "rustc (binding)",
        Command::new("rustc").args(["--edition", "2024", "-L", dir])
            .args(["-C", &format!("link-arg=-Wl,-rpath,{dir}"), "-o", &format!("{dir}/binding"), &format!("{dir}/binding.rs")]),
    );
    println!("built libvse.so (gcc) and the Rust binding (rustc, #[link(name = \"vse\")]): OK");
    let syms = run("nm", Command::new("nm").args(["-D", "--defined-only", &format!("{dir}/libvse.so")]));
    println!("libvse.so exports: {:?}\n", syms.lines().map(|l| l.rsplit(' ').next().unwrap()).collect::<Vec<_>>());
    print!("{}", run("the Rust binding", &mut Command::new(format!("{dir}/binding"))));
}
