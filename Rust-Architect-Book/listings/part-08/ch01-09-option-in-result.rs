// verify: debug error:E0277
use std::collections::HashMap;
use std::num::ParseIntError;

fn pool_size(env: &HashMap<String, String>) -> Result<u32, ParseIntError> {
    let raw = env.get("POOL_SIZE")?;
    raw.parse()
}

fn main() {
    let env = HashMap::from([("POOL_SIZE".to_string(), "32".to_string())]);
    println!("{:?}", pool_size(&env));
}
