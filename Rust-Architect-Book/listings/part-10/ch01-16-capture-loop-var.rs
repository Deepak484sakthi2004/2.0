// verify: debug error:E0597
fn main() {
    let mut handlers: Vec<Box<dyn Fn() -> u32>> = Vec::new();
    for shard in 0..3 {
        handlers.push(Box::new(|| shard * 10)); // borrows the loop variable, which dies each iteration
    }
    for h in &handlers {
        println!("{}", h());
    }
}
