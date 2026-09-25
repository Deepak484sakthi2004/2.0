// verify: debug ok
// verify: debug miri-ok
// Initializing an array element by element: the MaybeUninit way, and the safe way that usually suffices.
use std::mem::MaybeUninit;

fn names_unsafe() -> [String; 4] {
    // An array of uninitialized slots: no String exists yet, so nothing can be dropped by accident.
    let mut slots: [MaybeUninit<String>; 4] = [const { MaybeUninit::uninit() }; 4];
    for (i, slot) in slots.iter_mut().enumerate() {
        slot.write(format!("worker-{i}")); // `write` stores without reading or dropping the old bytes
    }
    // SAFETY: all 4 slots were written above, and [MaybeUninit<String>; 4] has the same size and
    // layout as [String; 4] (MaybeUninit<T> is repr(transparent) over T).
    unsafe { std::mem::transmute::<[MaybeUninit<String>; 4], [String; 4]>(slots) }
}

fn names_safe() -> [String; 4] {
    std::array::from_fn(|i| format!("worker-{i}")) // the same result, no unsafe
}

fn main() {
    println!("{:?}", names_unsafe());
    println!("{:?}", names_safe());
    println!(
        "size_of MaybeUninit<String> = {}, String = {}, Option<String> = {}, MaybeUninit<Option<String>> = {}",
        size_of::<MaybeUninit<String>>(),
        size_of::<String>(),
        size_of::<Option<String>>(),
        size_of::<MaybeUninit<Option<String>>>()
    );
    println!(
        "Option<MaybeUninit<&u8>> = {} B vs Option<&u8> = {} B (MaybeUninit hides the niche)",
        size_of::<Option<MaybeUninit<&u8>>>(),
        size_of::<Option<&u8>>()
    );
}
