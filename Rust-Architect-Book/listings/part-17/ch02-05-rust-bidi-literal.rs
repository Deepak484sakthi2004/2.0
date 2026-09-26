// verify: debug error:text_direction_codepoint_in_literal
// Listing 17.2-5: rustc rejects Unicode bidirectional control characters inside literals
// (deny-by-default since Rust 1.56.1, after "Trojan Source", CVE-2021-42574). The string below
// contains a real U+202E RIGHT-TO-LEFT OVERRIDE; an editor may display this line reordered.
fn main() {
    let access_level = "user‮ ";
    println!("{access_level}");
}
