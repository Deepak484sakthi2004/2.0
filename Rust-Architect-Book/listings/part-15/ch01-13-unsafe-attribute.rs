// verify: debug error:attribute
#[no_mangle]
pub extern "C" fn meridian_version() -> u32 {
    3
}

fn main() {
    println!("{}", meridian_version());
}
