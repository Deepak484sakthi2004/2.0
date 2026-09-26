// verify: debug ok
// The PR under review: "fraud: expose explanations to the risk console (FFM)".
// It compiles (with one warning, below), and the author's demo in `main` works, because the demo's
// caller is Rust. The review is about what happens when the caller is Java.
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char};
use std::sync::Mutex;

pub struct Explainer {
    weights: HashMap<String, f64>,
    last: CString,
}

static EXPLAINER: Mutex<Option<Explainer>> = Mutex::new(None);

pub struct ExplainOptions {
    pub top: usize,
    pub include_negative: bool,
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq)]
pub enum Format {
    Text = 0,
    Json = 1,
}

/// Loads the model. Returns false on failure.
#[unsafe(no_mangle)]
pub extern "C" fn init(model_name: String) -> bool {
    let weights = HashMap::from([
        ("amount".to_string(), 0.41),
        ("velocity".to_string(), 0.22),
        ("country".to_string(), -0.05),
    ]);
    *EXPLAINER.lock().unwrap() = Some(Explainer { weights, last: CString::default() });
    !model_name.is_empty()
}

/// Returns a newly allocated explanation. The caller frees it with free().
#[unsafe(no_mangle)]
pub extern "C" fn explain(features: *const f64, n: i32, opts: *const ExplainOptions, format: Format) -> *mut c_char {
    let features = unsafe { std::slice::from_raw_parts(features, n as usize) };
    let opts = unsafe { &*opts };
    let mut guard = EXPLAINER.lock().unwrap();
    let e = guard.as_mut().unwrap();
    let names = ["amount", "velocity", "country"];
    let mut parts: Vec<(&str, f64)> = names.iter().zip(features).map(|(n, x)| (*n, x * e.weights[*n])).collect();
    parts.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    parts.retain(|p| opts.include_negative || p.1 >= 0.0);
    parts.truncate(opts.top);
    let text = match format {
        Format::Text => parts.iter().map(|(n, v)| format!("{n} {v:+.2}")).collect::<Vec<_>>().join(", "),
        Format::Json => format!(
            "[{}]",
            parts.iter().map(|(n, v)| format!("{{\"f\":\"{n}\",\"v\":{v:.2}}}")).collect::<Vec<_>>().join(",")
        ),
    };
    e.last = CString::new(text.clone()).unwrap();
    CString::new(text).unwrap().into_raw()
}

/// The last explanation, for the console's "copy" button.
#[unsafe(no_mangle)]
pub extern "C" fn last_explanation() -> *const c_char {
    EXPLAINER.lock().unwrap().as_ref().unwrap().last.as_ptr()
}

fn main() {
    // The author's demo: correct calls from Rust.
    assert!(init("fraud-2026-09".to_string()));
    let features = [0.9, 0.5, 0.1];
    let opts = ExplainOptions { top: 2, include_negative: false };
    let text = explain(features.as_ptr(), 3, &opts, Format::Text);
    let json = explain(features.as_ptr(), 3, &opts, Format::Json);
    unsafe {
        println!("text: {}", CStr::from_ptr(text).to_str().unwrap());
        println!("json: {}", CStr::from_ptr(json).to_str().unwrap());
        println!("last: {}", CStr::from_ptr(last_explanation()).to_str().unwrap());
        drop(CString::from_raw(text)); // the demo frees correctly; the doc comment tells C to use free()
        drop(CString::from_raw(json));
    }
}
