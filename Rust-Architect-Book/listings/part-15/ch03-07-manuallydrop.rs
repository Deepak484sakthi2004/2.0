// verify: debug ok
// verify: debug miri-ok
// ManuallyDrop: (1) control the order in which fields are dropped; (2) take a Vec apart and rebuild it.
use std::mem::ManuallyDrop;

struct Session {
    // Fields drop in declaration order. We need the connection closed BEFORE the pool handle goes away,
    // whatever order a future refactor puts the fields in, so we drop `conn` explicitly.
    conn: ManuallyDrop<Conn>,
    #[allow(dead_code)] // held only for its Drop
    pool: PoolHandle,
}

struct Conn;
impl Drop for Conn {
    fn drop(&mut self) {
        println!("  conn closed");
    }
}
struct PoolHandle;
impl Drop for PoolHandle {
    fn drop(&mut self) {
        println!("  pool handle released");
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: `conn` is dropped exactly once, here; it is never used after this line
        // (the compiler drops `pool` next and never touches a ManuallyDrop field).
        unsafe { ManuallyDrop::drop(&mut self.conn) };
    }
}

fn main() {
    println!("drop(Session):");
    drop(Session { conn: ManuallyDrop::new(Conn), pool: PoolHandle });

    // Take a Vec apart without freeing its buffer (what `Vec::into_raw_parts` does), then rebuild it.
    let v = vec![1u32, 2, 3];
    let mut v = ManuallyDrop::new(v); // from here on, nothing frees the buffer automatically
    let (ptr, len, cap) = (v.as_mut_ptr(), v.len(), v.capacity());
    // SAFETY: ptr/len/cap came from a Vec<u32> that was never dropped (ManuallyDrop), and
    // we rebuild it exactly once, so the buffer is freed exactly once.
    let rebuilt = unsafe { Vec::from_raw_parts(ptr, len, cap) };
    println!("rebuilt: {rebuilt:?} (len {len}, cap {cap})");
    println!(
        "Option<ManuallyDrop<&u8>> = {} B, Option<&u8> = {} B (ManuallyDrop keeps the niche)",
        size_of::<Option<ManuallyDrop<&u8>>>(),
        size_of::<Option<&u8>>()
    );
}
