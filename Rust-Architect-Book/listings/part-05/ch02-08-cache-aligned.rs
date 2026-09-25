// verify: debug ok
use std::mem::{align_of, size_of};
use std::sync::atomic::AtomicU64;

/// One value per cache line, so values updated by different cores never share a line.
#[repr(align(64))]
struct CachePadded<T>(T);

/// Eight counters packed together, starting on a line boundary.
#[repr(align(64))]
struct Packed([AtomicU64; 8]);

fn main() {
    println!("AtomicU64:              size {:>3}, align {:>2}", size_of::<AtomicU64>(), align_of::<AtomicU64>());
    println!("CachePadded<AtomicU64>: size {:>3}, align {:>2}", size_of::<CachePadded<AtomicU64>>(), align_of::<CachePadded<AtomicU64>>());
    println!("Packed (8 counters):    size {:>3}", size_of::<Packed>());
    println!("[CachePadded<_>; 8]:    size {:>3}", size_of::<[CachePadded<AtomicU64>; 8]>());

    let packed = Packed(std::array::from_fn(|_| AtomicU64::new(0)));
    let padded: [CachePadded<AtomicU64>; 8] = std::array::from_fn(|_| CachePadded(AtomicU64::new(0)));

    let base = &packed as *const Packed as usize;
    let lines: Vec<usize> = packed.0.iter().map(|c| (c as *const AtomicU64 as usize - base) / 64).collect();
    println!("packed: counter i lives on line {lines:?} (relative to the array)");

    let base = &padded as *const _ as usize;
    let lines: Vec<usize> = padded.iter().map(|c| (&c.0 as *const AtomicU64 as usize - base) / 64).collect();
    println!("padded: counter i lives on line {lines:?}");
}
