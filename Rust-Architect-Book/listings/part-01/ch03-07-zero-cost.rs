// verify: release ok
pub fn sum_even_squares_loop(data: &[u64]) -> u64 {
    let mut total = 0;
    for i in 0..data.len() {
        let x = data[i];
        if x % 2 == 0 {
            total += x * x;
        }
    }
    total
}

pub fn sum_even_squares_iter(data: &[u64]) -> u64 {
    data.iter().filter(|&&x| x % 2 == 0).map(|&x| x * x).sum()
}

fn main() {
    let data: Vec<u64> = (1..=10).collect();
    println!("loop: {}", sum_even_squares_loop(&data));
    println!("iter: {}", sum_even_squares_iter(&data));
}
