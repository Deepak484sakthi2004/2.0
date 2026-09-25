// verify: debug error:E0382
fn main() {
    let batch = vec!["evt-1".to_string(), "evt-2".to_string()];
    for evt in batch {
        println!("publish {evt}");
    }
    println!("published {} events", batch.len()); // the loop consumed `batch`
}
