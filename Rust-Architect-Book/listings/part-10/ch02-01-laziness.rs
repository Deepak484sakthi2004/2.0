// verify: debug ok
// Iterators are lazy and pull-based: nothing runs until a consumer asks for the next item,
// and then each item travels the whole pipeline before the next one starts.
fn main() {
    let amounts = [120_u64, 5, 980, 42, 3_000];

    let pipeline = amounts
        .iter()
        .inspect(|a| println!("  source yields {a}"))
        .filter(|&&a| {
            let keep = a >= 100;
            println!("    filter({a}) -> {keep}");
            keep
        })
        .map(|&a| {
            println!("      map({a}) -> {}", a * 2);
            a * 2
        });
    println!("pipeline built; nothing has run yet");

    let first_two: Vec<u64> = pipeline.take(2).collect();
    println!("first_two = {first_two:?}  (3,000 was never read)");

    // Short-circuiting consumers stop pulling as soon as the answer is known:
    let big = amounts.iter().inspect(|a| println!("  any() looked at {a}")).any(|&a| a > 500);
    println!("any > 500: {big}");
}
