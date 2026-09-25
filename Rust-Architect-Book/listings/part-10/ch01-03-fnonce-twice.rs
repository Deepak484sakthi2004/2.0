// verify: debug error:E0382
fn main() {
    let batch = vec![10_u64, 20, 30];
    let flush = move || {
        let owned = batch; // moves the captured Vec out: this closure is FnOnce only
        owned.iter().sum::<u64>()
    };
    println!("first flush: {}", flush());
    println!("second flush: {}", flush()); // a second call would use a moved-out Vec
}
