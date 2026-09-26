// verify: debug error:confusable
// Listing 17.2-4: rustc's own lexer-level defenses. Rust allows non-ASCII identifiers (RFC 2457,
// stable since 1.53), and ships lints for look-alikes. Denying them turns the warning into an error.
#![deny(confusable_idents, mixed_script_confusables)]

fn main() {
    let e = 1; // Latin small e
    let е = 2; // Cyrillic small ie (U+0435)
    println!("{}", e + е);
}
