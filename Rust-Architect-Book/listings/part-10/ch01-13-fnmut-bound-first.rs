// verify: debug error:E0525
fn run_twice<F: Fn()>(f: F) {
    f();
    f();
}

fn main() {
    let mut retries = 0;
    let bump = || retries += 1; // no expected type here: inferred as FnMut from the body
    run_twice(bump); // ...and FnMut doesn't satisfy an Fn bound
    println!("{retries}");
}
