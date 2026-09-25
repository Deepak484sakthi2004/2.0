// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release
// What each memory ordering costs on x86-64: read the instructions, not the names.
use std::sync::atomic::{compiler_fence, fence, AtomicU64, Ordering::*};

#[inline(never)] pub fn load_relaxed(a: &AtomicU64) -> u64 { a.load(Relaxed) }
#[inline(never)] pub fn load_acquire(a: &AtomicU64) -> u64 { a.load(Acquire) }
#[inline(never)] pub fn load_seqcst(a: &AtomicU64) -> u64 { a.load(SeqCst) }

#[inline(never)] pub fn store_relaxed(a: &AtomicU64, v: u64) { a.store(v, Relaxed) }
#[inline(never)] pub fn store_release(a: &AtomicU64, v: u64) { a.store(v, Release) }
#[inline(never)] pub fn store_seqcst(a: &AtomicU64, v: u64) { a.store(v, SeqCst) }

#[inline(never)] pub fn add_relaxed(a: &AtomicU64) -> u64 { a.fetch_add(1, Relaxed) }
#[inline(never)] pub fn add_seqcst(a: &AtomicU64) -> u64 { a.fetch_add(1, SeqCst) }
#[inline(never)] pub fn add_unused(a: &AtomicU64) { a.fetch_add(1, Relaxed); }
#[inline(never)] pub fn or_relaxed(a: &AtomicU64) -> u64 { a.fetch_or(1, Relaxed) }
#[inline(never)] pub fn or_unused(a: &AtomicU64) { a.fetch_or(1, Relaxed); }
#[inline(never)] pub fn swap_relaxed(a: &AtomicU64, v: u64) -> u64 { a.swap(v, Relaxed) }
#[inline(never)] pub fn max_relaxed(a: &AtomicU64, v: u64) -> u64 { a.fetch_max(v, Relaxed) }

#[inline(never)]
pub fn cas(a: &AtomicU64, old: u64, new: u64) -> Result<u64, u64> {
    a.compare_exchange(old, new, AcqRel, Acquire)
}

#[inline(never)] pub fn fence_acquire() { fence(Acquire) }
#[inline(never)] pub fn fence_release() { fence(Release) }
#[inline(never)] pub fn fence_acqrel() { fence(AcqRel) }
#[inline(never)] pub fn fence_seqcst() { fence(SeqCst) }
#[inline(never)] pub fn compiler_only() { compiler_fence(SeqCst) }
