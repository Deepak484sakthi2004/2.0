// verify: debug error:E0382
#[derive(Debug)]
struct ClientConfig {
    timeout_ms: u64,
    retries: u32,
}

struct ClientBuilder {
    timeout_ms: u64,
    retries: u32,
}

impl ClientBuilder {
    fn new() -> Self {
        ClientBuilder { timeout_ms: 1_000, retries: 0 }
    }
    fn timeout_ms(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }
    fn retries(mut self, n: u32) -> Self {
        self.retries = n;
        self
    }
    fn build(self) -> ClientConfig {
        ClientConfig { timeout_ms: self.timeout_ms, retries: self.retries }
    }
}

fn main() {
    let builder = ClientBuilder::new();
    builder.timeout_ms(250);
    builder.retries(3);
    let config = builder.build();
    println!("{config:?}");
}
