// verify: debug ok
use std::os::unix::process::ExitStatusExt;
use std::process::Command;

extern "C" fn callback() {
    panic!("unwinding out of an extern \"C\" function");
}

/// Re-runs this very binary in several failure modes and reports how the OS saw each one end.
fn main() -> Result<(), String> {
    if let Some(mode) = std::env::args().nth(1) {
        return match mode.as_str() {
            "ok" => Ok(()),
            "err" => Err("config missing".to_string()), // main returned Err
            "panic" => panic!("bug"),
            "exit(3)" => std::process::exit(3),
            "abort" => std::process::abort(),
            "extern-c" => {
                callback();
                Ok(())
            }
            other => Err(format!("unknown mode {other}")),
        };
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    for mode in ["ok", "err", "panic", "exit(3)", "abort", "extern-c"] {
        let out = Command::new(&exe).arg(mode).output().map_err(|e| e.to_string())?;
        let stderr = String::from_utf8_lossy(&out.stderr);
        let last = stderr.lines().filter(|l| !l.trim().is_empty()).last().unwrap_or("");
        println!("{mode:<9} code={:<8} signal={:<8} last stderr line: {last}", format!("{:?}", out.status.code()), format!("{:?}", out.status.signal()));
    }
    Ok(())
}
