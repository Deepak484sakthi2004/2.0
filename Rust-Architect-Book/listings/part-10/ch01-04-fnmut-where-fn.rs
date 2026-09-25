// verify: debug error:E0594
fn run_twice<F: Fn()>(f: F) {
    f();
    f();
}

fn main() {
    let mut retries = 0;
    run_twice(|| retries += 1); // the Fn bound drives inference: this closure is checked AS an Fn
    println!("{retries}");
}
