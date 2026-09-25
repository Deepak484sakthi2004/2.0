// verify: debug build
// Source for the MIR artifacts in Chapter 18.4 (tools/emit.ps1 -Target mir, debug and release):
// a match that moves out of enum variants, with the drops and unwind paths that implies.
pub enum Cmd {
    Get(String),
    Del(String),
    Ping,
}

#[inline(never)]
pub fn cost(cmd: Cmd) -> usize {
    match cmd {
        Cmd::Get(k) => k.len(),
        Cmd::Del(k) => k.len() + 1,
        Cmd::Ping => 0,
    }
}
