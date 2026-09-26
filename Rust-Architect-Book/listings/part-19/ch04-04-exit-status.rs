// verify: debug ok
// How a process ends, as the parent sees it. Re-execute this binary in several "endings" and decode the raw wait
// status, the shell's $?, and what a pipeline hides without pipefail.
use std::os::unix::process::ExitStatusExt;
use std::process::Command;

#[allow(unconditional_recursion)]
fn recurse(depth: u64) -> u64 {
    let pad = std::hint::black_box([depth; 64]);
    recurse(depth + 1) + pad[0]
}

fn ending(name: &str) {
    match name {
        "ok" => {}
        "exit3" => std::process::exit(3),
        "error" => {
            let r: Result<(), String> = Err("config not found".into());
            r.unwrap();
        }
        "abort" => std::process::abort(),
        "overflow" => {
            recurse(0);
        }
        "sleep" => std::thread::sleep(std::time::Duration::from_secs(30)),
        _ => unreachable!(),
    }
}

fn main() {
    if let Some(name) = std::env::args().nth(1) {
        ending(&name);
        return;
    }
    let exe = std::env::current_exe().unwrap();
    println!("{:<9} {:>8} {:>10} {:>8} {:>6}  what the parent learns", "ending", "raw", "code()", "signal()", "sh $?");
    for name in ["ok", "exit3", "error", "abort", "overflow", "sleep"] {
        let mut child = Command::new(&exe).arg(name).stderr(std::process::Stdio::piped()).spawn().unwrap();
        if name == "sleep" {
            std::thread::sleep(std::time::Duration::from_millis(100));
            child.kill().unwrap(); // SIGKILL, as the OOM killer or `kubectl delete --force` would
        }
        let out = child.wait_with_output().unwrap();
        let st = out.status;
        let shell = Command::new("sh").arg("-c").arg(format!("'{}' {name} 2>/dev/null & p=$!; {} wait $p; echo $?",
            exe.display(), if name == "sleep" { "sleep 0.1; kill -9 $p;" } else { "" })).output().unwrap();
        let last = String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("").chars().take(60).collect::<String>();
        println!("{name:<9} {:>#8x} {:>10} {:>8} {:>6}  {last}", st.into_raw(), format!("{:?}", st.code()),
            format!("{:?}", st.signal()), String::from_utf8_lossy(&shell.stdout).trim());
    }
    println!("--- a failing command in a pipeline ---");
    let script = format!("'{}' error 2>/dev/null | cat; echo \"without pipefail: $?\"; \
                          set -o pipefail; '{}' error 2>/dev/null | cat; echo \"with pipefail:    $?\"", exe.display(), exe.display());
    let out = Command::new("bash").arg("-c").arg(&script).output().unwrap();
    print!("{}", String::from_utf8_lossy(&out.stdout));
}
