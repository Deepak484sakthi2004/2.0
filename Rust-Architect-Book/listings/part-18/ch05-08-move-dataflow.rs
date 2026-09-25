// verify: debug error:E0382
// Move checking is a dataflow question on the MIR control-flow graph: "on SOME path to this use,
// was the value moved?" The back edge of the loop is such a path.
fn publish(batch: Vec<u64>) -> usize {
    batch.len()
}

fn main() {
    let batch = vec![1u64, 2, 3];
    for attempt in 0..3 {
        let sent = publish(batch); // moved in the first iteration...
        println!("attempt {attempt}: sent {sent}");
    }
}
