// verify: debug ok
// Measured answers for the size-prediction exercises of Chapters 10.1 and 10.3 (see Appendix A, Part X).
use std::mem::size_of_val;
fn main() {
    let s: &str = "hello";
    let v: Vec<u8> = vec![1, 2, 3];
    let mut buf = [0u8; 1024];
    let arr = [0u8; 1024];
    let c0 = || 1;
    let c1 = || s.len();
    let c2 = move || s.len();
    let c3 = move || v.len();
    println!("nothing={} &str-by-ref={} &str-moved={} Vec-moved={}", size_of_val(&c0), size_of_val(&c1), size_of_val(&c2), size_of_val(&c3));
    let c4 = || { buf[0] = 1; };
    println!("&mut [u8;1024]={}", size_of_val(&c4));
    let c5 = move || arr.len();
    println!("moved [u8;1024]={}", size_of_val(&c5));
    let v2: Vec<u32> = (0..20).collect();
    let w2: Vec<u32> = (0..20).collect();
    let it = v2.iter().zip(w2.iter()).skip(3).step_by(2);
    println!("zip.skip.step_by={} B  {}", size_of_val(&it), std::any::type_name_of_val(&it));
    println!("zip alone={} B", size_of_val(&v2.iter().zip(w2.iter())));
    println!("IntoIter<u64>={} B", std::mem::size_of::<std::vec::IntoIter<u64>>());
}
