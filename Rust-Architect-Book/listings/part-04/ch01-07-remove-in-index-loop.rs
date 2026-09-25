// verify: debug panic index out of bounds
fn main() {
    let mut sessions = vec!["ok", "expired", "expired", "ok"];
    // The borrow checker is satisfied (no loan outlives a mutation), and the LOGIC is wrong:
    // indices shift after remove(), and `0..len` was fixed before the loop started.
    let n = sessions.len();
    for i in 0..n {
        if sessions[i] == "expired" {
            sessions.remove(i);
        }
    }
    println!("{sessions:?}");
}
