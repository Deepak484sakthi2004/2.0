// verify: debug error:E0277
//! Every future from an `async` block or `async fn` is !Unpin, even one that holds no
//! references and never awaits [RUSTC]: the compiler doesn't analyze whether the state
//! machine actually borrows from itself.
fn is_unpin<T: Unpin>(_: &T) {}

fn main() {
    let fut = async { 1 + 1 }; // no borrows, no awaits
    is_unpin(&fut);
}
