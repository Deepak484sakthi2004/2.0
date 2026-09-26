// verify: debug ok
// Chapter 6.5's C-ABI rule vtable, loaded for real: this program builds a rule PLUGIN as a separate
// `cdylib` with rustc, then loads it with dlopen/dlsym. The rule borrows the library (FfiRule<'lib>),
// so it can't outlive it. A counting allocator in the HOST shows that the plugin's allocations never
// touch the host's allocator: the plugin has its own copy of std. Every inner step must succeed.
use std::ffi::{CStr, c_void};
use std::marker::PhantomData;
use std::process::Command;
use std::ptr::NonNull;

// --- instrumentation: count the HOST's Rust heap allocations (GlobalAlloc is explained in Part XV) ---
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    static ALLOCS: AtomicUsize = AtomicUsize::new(0);
    static FREES: AtomicUsize = AtomicUsize::new(0);

    pub struct Counting;

    // SAFETY: both methods forward their exact arguments to `System`, which upholds the
    // GlobalAlloc contract; the counters are plain atomics, so counting never allocates.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            FREES.fetch_add(1, Relaxed);
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static GLOBAL: Counting = Counting;

    pub fn counts() -> (usize, usize) {
        (ALLOCS.load(Relaxed), FREES.load(Relaxed))
    }
}

/// The plugin, as the analytics team would ship it: Chapter 6.5's plugin side, plus an ABI version.
const PLUGIN_RS: &str = r##"
use std::ffi::c_void;

#[repr(C)]
pub struct RuleVTable {
    pub score: extern "C" fn(data: *const c_void, amount_cents: i64) -> u32,
    pub drop: unsafe extern "C" fn(data: *mut c_void),
}

#[repr(C)]
pub struct FfiRule {
    data: *mut c_void,
    vtable: &'static RuleVTable, // 'static is true INSIDE the plugin, not for the host
}

struct LargeAmount {
    over_cents: i64,
}

extern "C" fn large_amount_score(data: *const c_void, amount_cents: i64) -> u32 {
    // SAFETY: the host passes back only the `data` created by `new_large_amount`, until `drop`.
    let rule = unsafe { &*(data as *const LargeAmount) };
    if amount_cents > rule.over_cents { 60 } else { 0 }
}

unsafe extern "C" fn large_amount_drop(data: *mut c_void) {
    // SAFETY: `data` came from Box::into_raw below; the host calls this once.
    drop(unsafe { Box::from_raw(data as *mut LargeAmount) });
}

static LARGE_AMOUNT_VTABLE: RuleVTable = RuleVTable { score: large_amount_score, drop: large_amount_drop };

#[unsafe(no_mangle)]
pub extern "C" fn rule_plugin_abi_version() -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn new_large_amount(over_cents: i64) -> FfiRule {
    let data = Box::into_raw(Box::new(LargeAmount { over_cents })) as *mut c_void; // the PLUGIN's allocator
    FfiRule { data, vtable: &LARGE_AMOUNT_VTABLE }
}
"##;

// ---------------- the host ----------------

/// Must match the plugin's RuleVTable exactly (the plugin ABI, version 1).
#[repr(C)]
struct RuleVTable {
    score: extern "C" fn(data: *const c_void, amount_cents: i64) -> u32,
    drop: unsafe extern "C" fn(data: *mut c_void),
}

/// What `new_large_amount` returns, as the host declares it: two pointers, no lifetimes.
#[repr(C)]
struct RawRule {
    data: *mut c_void,
    vtable: *const RuleVTable,
}

/// INVARIANT: `handle` came from a successful dlopen and is closed exactly once, in Drop.
pub struct Library {
    handle: NonNull<c_void>,
}

impl Library {
    pub fn open(path: &CStr) -> Result<Library, String> {
        // SAFETY: `path` is NUL-terminated. Loading runs the library's initializers: trusted plugins only.
        let h = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        NonNull::new(h).map(|handle| Library { handle }).ok_or_else(|| "dlopen failed".to_owned())
    }

    /// # Safety
    /// `F` must be the symbol's exact function-pointer type.
    pub unsafe fn get<F: Copy>(&self, name: &CStr) -> Result<F, String> {
        assert_eq!(size_of::<F>(), size_of::<*mut c_void>());
        // SAFETY: a live handle and a NUL-terminated name.
        let p = unsafe { libc::dlsym(self.handle.as_ptr(), name.as_ptr()) };
        if p.is_null() {
            return Err(format!("symbol {name:?} not found"));
        }
        // SAFETY: the caller guarantees F is the symbol's exact type.
        Ok(unsafe { std::mem::transmute_copy::<*mut c_void, F>(&p) })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        println!("  dlclose(plugin)");
        // SAFETY: the invariant; every FfiRule<'lib> borrowing this library is already gone.
        unsafe { libc::dlclose(self.handle.as_ptr()) };
    }
}

/// A rule whose code, vtable, and state live in a loaded library: it can't outlive that library.
pub struct FfiRule<'lib> {
    raw: RawRule,
    _lib: PhantomData<&'lib Library>,
}

impl FfiRule<'_> {
    pub fn score(&self, amount_cents: i64) -> u32 {
        // SAFETY: `vtable` points into the library, which outlives 'lib (the borrow), so it's live.
        let vt = unsafe { &*self.raw.vtable };
        (vt.score)(self.raw.data, amount_cents)
    }
}

impl Drop for FfiRule<'_> {
    fn drop(&mut self) {
        println!("  rule dropped: the plugin frees its own state");
        // SAFETY: the vtable is live (see `score`); `data` came from the plugin's constructor and is
        // released exactly once, by the plugin's own drop entry (so by the plugin's allocator).
        unsafe { ((*self.raw.vtable).drop)(self.raw.data) }
    }
}

fn load_large_amount(lib: &Library, over_cents: i64) -> Result<FfiRule<'_>, String> {
    // SAFETY: the plugin ABI (version 1) declares exactly these two signatures.
    let version = unsafe { lib.get::<extern "C" fn() -> u32>(c"rule_plugin_abi_version")? };
    if version() != 1 {
        return Err(format!("unsupported plugin ABI {}", version()));
    }
    let ctor = unsafe { lib.get::<extern "C" fn(i64) -> RawRule>(c"new_large_amount")? };
    Ok(FfiRule { raw: ctor(over_cents), _lib: PhantomData })
}

/// Runs a command and insists that it succeeded; returns its stdout.
fn run(what: &str, cmd: &mut Command) -> String {
    let out = cmd.output().unwrap_or_else(|e| panic!("{what}: could not start: {e}"));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{what} failed ({}):\n{stderr}", out.status);
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn main() -> Result<(), String> {
    let dir = "/tmp/meridian-16-3-plugin";
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(format!("{dir}/rule_large_amount.rs"), PLUGIN_RS).unwrap();
    let so = format!("{dir}/librule_large_amount.so");
    run(
        "rustc (plugin cdylib)",
        Command::new("rustc").args(["--edition", "2024", "--crate-type", "cdylib", "--crate-name", "rule_large_amount"])
            .args(["-o", &so, &format!("{dir}/rule_large_amount.rs")]),
    );
    let syms = run("nm", Command::new("nm").args(["-D", "--defined-only", &so]));
    println!("plugin exports: {:?}", syms.lines().map(|l| l.rsplit(' ').next().unwrap()).collect::<Vec<_>>());

    let path = std::ffi::CString::new(so).unwrap();
    let lib = Library::open(&path)?;
    {
        let before = counting::counts();
        let rule = load_large_amount(&lib, 500_000)?;
        let after = counting::counts();
        println!("host allocator during load + new_large_amount: +{} allocs", after.0 - before.0);
        println!("score(1_200) = {}, score(750_000) = {}", rule.score(1_200), rule.score(750_000));
        let before = counting::counts();
        drop(rule);
        println!("host allocator during the rule's drop: +{} frees", counting::counts().1 - before.1);
    } // every FfiRule<'_> borrowing `lib` is gone here...
    drop(lib); // ...so unloading is allowed (Chapter 16.4 shows the E0505 when it isn't)
    Ok(())
}
