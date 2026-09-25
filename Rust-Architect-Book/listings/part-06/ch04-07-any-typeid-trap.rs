// verify: debug ok
use std::any::{Any, TypeId};

fn describe(value: &dyn Any) -> String {
    if let Some(n) = value.downcast_ref::<u32>() {
        format!("u32 {n}")
    } else if let Some(s) = value.downcast_ref::<String>() {
        format!("String {s:?}")
    } else {
        "unknown type".to_string()
    }
}

fn main() {
    let items: Vec<Box<dyn Any>> = vec![Box::new(7u32), Box::new(String::from("acme")), Box::new(1.5f64)];
    for item in &items {
        println!("{}", describe(&**item));
    }

    // The trap: Box<dyn Any> is itself a 'static type, so it is also `Any`.
    let boxed: Box<dyn Any> = Box::new(7u32);
    println!("describe(&boxed)  -> {}", describe(&boxed)); // coerces the BOX to &dyn Any
    println!("describe(&*boxed) -> {}", describe(&*boxed)); // the value inside the box
    println!("boxed.type_id()    is u32? {}", boxed.type_id() == TypeId::of::<u32>());
    println!("(*boxed).type_id() is u32? {}", (*boxed).type_id() == TypeId::of::<u32>());
    println!("boxed.type_id()    is Box<dyn Any>? {}", boxed.type_id() == TypeId::of::<Box<dyn Any>>());
}
