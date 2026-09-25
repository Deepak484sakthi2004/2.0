// verify: debug ok
// verify: debug miri-ok
use std::ffi::c_void;

/// A hand-built "trait object" with a C-compatible layout. A plugin boundary needs this,
/// because Rust's own vtable layout (and `dyn Trait` in general) is not a stable ABI.
#[repr(C)]
pub struct RuleVTable {
    pub score: extern "C" fn(data: *const c_void, amount_cents: i64) -> u32,
    pub drop: unsafe extern "C" fn(data: *mut c_void),
}

#[repr(C)]
pub struct FfiRule {
    data: *mut c_void,
    vtable: &'static RuleVTable,
}

impl FfiRule {
    pub fn score(&self, amount_cents: i64) -> u32 {
        (self.vtable.score)(self.data, amount_cents)
    }
}

impl Drop for FfiRule {
    fn drop(&mut self) {
        // SAFETY: `data` was produced by the constructor paired with this vtable (Box::into_raw),
        // and FfiRule is not Clone, so this is the only drop of that allocation.
        unsafe { (self.vtable.drop)(self.data) }
    }
}

// --- the "plugin side": an ordinary Rust type exported through the C-ABI table ---
struct LargeAmount {
    over_cents: i64,
}

extern "C" fn large_amount_score(data: *const c_void, amount_cents: i64) -> u32 {
    // SAFETY: the host only passes back the `data` pointer created by `new_large_amount`,
    // which points to a live LargeAmount until the drop entry is called.
    let rule = unsafe { &*(data as *const LargeAmount) };
    if amount_cents > rule.over_cents { 60 } else { 0 }
}

unsafe extern "C" fn large_amount_drop(data: *mut c_void) {
    // SAFETY: `data` came from Box::into_raw in `new_large_amount`; the caller guarantees a single call.
    drop(unsafe { Box::from_raw(data as *mut LargeAmount) });
}

static LARGE_AMOUNT_VTABLE: RuleVTable = RuleVTable { score: large_amount_score, drop: large_amount_drop };

/// What a plugin would export from a cdylib (loading it is Part XVI's job).
pub extern "C" fn new_large_amount(over_cents: i64) -> FfiRule {
    let data = Box::into_raw(Box::new(LargeAmount { over_cents })) as *mut c_void;
    FfiRule { data, vtable: &LARGE_AMOUNT_VTABLE }
}

fn main() {
    let rule = new_large_amount(500_000);
    println!("score(1_200) = {}, score(750_000) = {}", rule.score(1_200), rule.score(750_000));
    println!("size_of::<FfiRule>() = {} bytes", std::mem::size_of::<FfiRule>());
} // `rule` is dropped here: the plugin's own drop entry frees its Box
