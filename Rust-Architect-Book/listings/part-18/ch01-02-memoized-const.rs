// verify: debug error:E0080
// One failing constant, three uses. The constant is evaluated once (a memoized query); the error is
// reported once, and each use only gets a note.
const MAX_RETRIES: u8 = 200 + 100; // overflows u8: const evaluation fails

fn gateway() -> u8 {
    MAX_RETRIES
}
fn payments() -> u8 {
    MAX_RETRIES
}
fn ledger() -> u8 {
    MAX_RETRIES
}

fn main() {
    println!("{} {} {}", gateway(), payments(), ledger());
}
