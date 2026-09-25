// verify: debug ok
// Iterator's contract allows next() to return Some again after None. `fuse()` makes None final.
struct TailLog {
    polls: u32,
}

impl Iterator for TailLog {
    type Item = String;
    fn next(&mut self) -> Option<String> {
        self.polls += 1;
        // Like `tail -f` on a log: nothing new on polls 2 and 3, a new line on poll 4.
        match self.polls {
            1 => Some("line A".into()),
            4 => Some("line B".into()),
            _ => None,
        }
    }
}

fn main() {
    let mut raw = TailLog { polls: 0 };
    let raw_results: Vec<Option<String>> = (0..4).map(|_| raw.next()).collect();
    println!("unfused: {raw_results:?}");

    let mut fused = TailLog { polls: 0 }.fuse();
    let fused_results: Vec<Option<String>> = (0..4).map(|_| fused.next()).collect();
    println!("fused:   {fused_results:?}");
}
