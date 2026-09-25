// verify: debug ok
use std::mem::size_of;

fn main() {
    // Inference: integer literals default to i32 and floats to f64, unless context says otherwise.
    let a = 10;
    let b = 2.5;
    let c: u8 = 200;
    let d = 1_000_000u64;
    println!(
        "sizes: i32={} f64={} u8={} u64={} usize={} char={} bool={}",
        size_of::<i32>(), size_of::<f64>(), size_of::<u8>(), size_of::<u64>(),
        size_of::<usize>(), size_of::<char>(), size_of::<bool>()
    );
    println!("values: a={a} b={b} c={c} d={d}");

    // Mutability belongs to the binding, not the value.
    let mut total = 0u64;
    total += d;
    let total = total; // shadowing: from here on, `total` is an immutable binding
    println!("total={total}");

    // Shadowing can change the type: a transformation pipeline.
    let input = "  42 ";
    let input = input.trim();
    let input: u32 = input.parse().expect("a number");
    println!("parsed={input}");

    // `as` is a raw cast: it truncates, wraps, or saturates, and never fails.
    println!("300i32 as u8   = {}", 300i32 as u8);
    println!("-1i32 as u32   = {}", -1i32 as u32);
    println!("3.99f64 as i32 = {}", 3.99f64 as i32);
    println!("1e20 as i32    = {}", 1e20f64 as i32);
    println!("NaN as i32     = {}", f64::NAN as i32);

    // `From` / `TryFrom` are the checked alternatives.
    let wide: u64 = u64::from(c); // lossless: always succeeds
    let narrow = u8::try_from(300i32); // lossy: returns a Result
    println!("u64::from(200u8) = {wide}, u8::try_from(300) = {narrow:?}");

    // char is a Unicode scalar value (4 bytes), not a UTF-16 code unit.
    let ch = 'é';
    println!("'{ch}' = U+{:04X}, {} bytes in UTF-8", ch as u32, ch.len_utf8());
}
