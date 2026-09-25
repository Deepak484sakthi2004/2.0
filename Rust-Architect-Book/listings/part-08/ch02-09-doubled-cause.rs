// verify: debug ok
use std::error::Error;

#[derive(Debug, thiserror::Error)]
pub enum ArgsError {
    #[error("--top needs a value")]
    MissingValue,
}

#[derive(Debug, thiserror::Error)]
pub enum LogstatError {
    #[error("{0}")]
    Usage(#[from] ArgsError),
    #[error("cannot open {path}")]
    Open { path: String, source: std::io::Error },
}

fn render(e: &dyn Error) -> String {
    let mut line = format!("logstat: {e}");
    let mut cause = e.source();
    while let Some(c) = cause {
        line.push_str(&format!(": {c}"));
        cause = c.source();
    }
    line
}

fn main() {
    println!("{}", render(&LogstatError::from(ArgsError::MissingValue)));
    let open = LogstatError::Open { path: "access.log".into(), source: std::io::ErrorKind::NotFound.into() };
    println!("{}", render(&open));
}
