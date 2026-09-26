// verify: debug error:E0121
// Listing 17.5-5: signatures are inference boundaries by design. rustc could infer this return
// type (it even tells you what it is), but it refuses: a signature is a contract, checked on its own.
fn answer() -> _ {
    42
}

fn main() {
    println!("{}", answer());
}
