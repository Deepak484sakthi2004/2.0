// verify: debug ok
/// The first `max_chars` characters of `full`, as a borrowed slice (no allocation).
fn display_name(full: &str, max_chars: usize) -> &str {
    match full.char_indices().nth(max_chars) {
        Some((byte_index, _)) => &full[..byte_index], // always a char boundary
        None => full,
    }
}

fn main() {
    for name in ["Ada Lovelace", "Kristina Øberg", "Zoë", "李小龍 Bruce Lee"] {
        println!("{:<20} -> {:?}", name, display_name(name, 10));
    }
}
