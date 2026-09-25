// verify: debug ok
// Java's Collectors.toMap throws IllegalStateException on a duplicate key.
// Rust's collect::<HashMap<_, _>>() silently keeps the LAST value. A port that assumes Java's behavior loses the check.
use std::collections::HashMap;
use std::collections::hash_map::Entry;

/// Fee schedule rows: (merchant id, fee in basis points).
fn rows() -> Vec<(&'static str, u32)> {
    vec![("m-100", 290), ("m-200", 250), ("m-100", 190), ("m-300", 310)] // m-100 appears twice
}

/// The literal port: compiles, runs, and silently picks 190 bps for m-100.
fn load_ported(rows: Vec<(&'static str, u32)>) -> HashMap<&'static str, u32> {
    rows.into_iter().collect()
}

/// The faithful port: duplicates are an error, like toMap without a merge function.
fn load_strict(rows: Vec<(&'static str, u32)>) -> Result<HashMap<&'static str, u32>, String> {
    rows.into_iter().try_fold(HashMap::new(), |mut m, (k, v)| match m.entry(k) {
        Entry::Vacant(e) => {
            e.insert(v);
            Ok(m)
        }
        Entry::Occupied(e) => Err(format!("duplicate key {k}: {} and {v}", e.get())),
    })
}

fn main() {
    let ported = load_ported(rows());
    println!("ported: m-100 -> {} bps ({} merchants)", ported["m-100"], ported.len());
    println!("strict: {:?}", load_strict(rows()).map(|m| m.len()));
}
