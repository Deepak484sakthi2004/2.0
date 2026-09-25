// verify: debug build
// Source for the proc-macro expansion artifact in Chapter 18.2 (tools/emit.ps1 -Target expand):
// what #[derive(Serialize)] generates, including its anonymous-const naming trick.
use serde::Serialize;

#[derive(Serialize)]
pub struct Payment {
    pub id: u64,
    pub amount_cents: i64,
}
