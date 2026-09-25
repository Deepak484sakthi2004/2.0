// verify: debug ok
// Answer-key check (Chapter 10.3, systems exercise): which of these collect in place on this compiler?
fn report<T, U>(label: &str, src: Vec<T>, f: impl FnOnce(std::vec::IntoIter<T>) -> Vec<U>) {
    let (ptr, cap) = (src.as_ptr() as usize, src.capacity());
    let out = f(src.into_iter());
    println!(
        "{label:<22} same buffer={:<5} len={} cap={} (source cap {cap})",
        out.as_ptr() as usize == ptr,
        out.len(),
        out.capacity()
    );
}

fn main() {
    report("u8 -> u8", (0..1000u32).map(|x| x as u8).collect(), |it| it.map(|x| x ^ 1).collect::<Vec<u8>>());
    report("[u8; 3] -> u8", vec![[1u8, 2, 3]; 1000], |it| it.map(|a| a[0]).collect::<Vec<u8>>());
    report("u64 -> (u32, u32)", (0..1000u64).collect(), |it| {
        it.map(|x| (x as u32, (x >> 32) as u32)).collect::<Vec<(u32, u32)>>()
    });
}
