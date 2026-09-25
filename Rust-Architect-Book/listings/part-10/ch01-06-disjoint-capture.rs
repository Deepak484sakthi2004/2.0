// verify: debug ok
// verify: debug@2018 error:E0382
// Edition 2021+ closures capture the PLACES they use (cfg.name), not whole variables (cfg).
struct Upstream {
    name: String,
    timeout_ms: u64,
}

fn main() {
    let cfg = Upstream { name: "processor-a".to_string(), timeout_ms: 250 };
    let log_name = move || println!("upstream = {}", cfg.name); // moves only cfg.name (2021+)
    log_name();
    println!("timeout still readable: {} ms", cfg.timeout_ms); // 2018: `cfg` was moved whole
}
