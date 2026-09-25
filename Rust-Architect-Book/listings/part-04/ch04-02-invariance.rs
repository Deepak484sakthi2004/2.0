// verify: debug error:E0597
/// Overwrites the reference stored in `slot`.
fn overwrite<'a>(slot: &mut &'a str, value: &'a str) {
    *slot = value;
}

fn main() {
    let mut label: &'static str = "default";
    {
        let local = String::from("short-lived");
        overwrite(&mut label, &local); // &mut T is INVARIANT in T: 'a must equal 'static
    }
    println!("{label}"); // if this were allowed, `label` would point into freed memory
}
