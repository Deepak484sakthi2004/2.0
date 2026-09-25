// verify: debug ok
use std::mem::size_of;

#[allow(dead_code)]
struct Point {
    x: f64,
    y: f64,
}

fn main() {
    println!("Point:             {:>2} bytes", size_of::<Point>());
    println!("[Point; 4]:        {:>2} bytes", size_of::<[Point; 4]>());
    println!("Vec<Point>:        {:>2} bytes (the handle, not the elements)", size_of::<Vec<Point>>());
    println!("Box<Point>:        {:>2} bytes", size_of::<Box<Point>>());
    println!("&u64:              {:>2} bytes", size_of::<&u64>());
    println!("Option<&u64>:      {:>2} bytes", size_of::<Option<&u64>>());
    println!("Option<Box<u64>>:  {:>2} bytes", size_of::<Option<Box<u64>>>());
    println!("u64:               {:>2} bytes", size_of::<u64>());
    println!("Option<u64>:       {:>2} bytes", size_of::<Option<u64>>());
    println!("String:            {:>2} bytes", size_of::<String>());

    let points: Vec<Point> = (0..1_000_000)
        .map(|i| Point { x: i as f64, y: 0.0 })
        .collect();
    let payload = points.len() * size_of::<Point>();
    println!("1M points payload: {} bytes, one contiguous block", payload);
}
