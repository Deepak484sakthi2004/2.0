// verify: debug ok
// verify: debug miri-ok
// Memory that belongs to the C allocator, owned by a Rust value: malloc in the constructor,
// free in Drop, and `into_raw` for handing it to a C API that will free() it itself.
use std::ffi::c_void;
use std::ptr::NonNull;

unsafe extern "C" {
    // Declared with *mut c_void, exactly as C does (other pointer types trip a lint for these symbols).
    fn malloc(size: usize) -> *mut c_void;
    fn free(p: *mut c_void);
}

/// INVARIANT: `ptr` came from `malloc(len.max(1))`, is freed exactly once (by Drop or by whoever
/// receives it from `into_raw`), and `[ptr, ptr + len)` is initialized.
pub struct CBuf {
    ptr: NonNull<u8>,
    len: usize,
}

impl CBuf {
    /// A zero-filled buffer from the C heap, or None if malloc fails.
    pub fn zeroed(len: usize) -> Option<CBuf> {
        // malloc(0) may return NULL or a unique pointer; asking for at least 1 byte avoids the ambiguity.
        // SAFETY: malloc has no preconditions.
        let raw = unsafe { malloc(len.max(1)) }.cast::<u8>();
        let ptr = NonNull::new(raw)?;
        // SAFETY: `ptr` is valid for writes of `len` bytes (we asked for at least that many).
        unsafe { ptr.as_ptr().write_bytes(0, len) };
        Some(CBuf { ptr, len })
    }

    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: the invariant: `len` initialized bytes at `ptr`, alive as long as `self`.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: as above, and `&mut self` guarantees no other slice of this buffer is live.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    /// Hands ownership to C. The receiver must release it with `free()`, exactly once.
    pub fn into_raw(self) -> *mut u8 {
        let p = self.ptr.as_ptr();
        std::mem::forget(self); // don't run Drop: ownership moved to the caller
        p
    }
}

impl Drop for CBuf {
    fn drop(&mut self) {
        // SAFETY: the invariant: `ptr` came from malloc and this is its only release.
        unsafe { free(self.ptr.as_ptr().cast()) }
    }
}

fn main() {
    let mut b = CBuf::zeroed(16).expect("malloc failed");
    b.as_mut_slice()[..5].copy_from_slice(b"hello");
    println!("{:?}", &b.as_slice()[..8]);
    drop(b); // free() runs here

    // The transfer path: C takes the pointer. Here "C" is played by a direct call to free().
    let handed_over = CBuf::zeroed(64).expect("malloc failed").into_raw();
    // SAFETY: `handed_over` came from malloc via CBuf::into_raw and is freed exactly once, here.
    unsafe { free(handed_over.cast()) };
    println!("both buffers released by the allocator that created them");
}
