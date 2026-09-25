// verify: debug ok
use std::mem::needs_drop;

fn main() {
    println!("u64:             {}", needs_drop::<u64>());
    println!("(u32, bool):     {}", needs_drop::<(u32, bool)>());
    println!("[u64; 1024]:     {}", needs_drop::<[u64; 1024]>());
    println!("&String:         {}", needs_drop::<&String>());
    println!("String:          {}", needs_drop::<String>());
    println!("Vec<u8>:         {}", needs_drop::<Vec<u8>>());
    println!("Option<Box<u8>>: {}", needs_drop::<Option<Box<u8>>>());
}
