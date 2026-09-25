// verify: debug ok
// verify: debug test
//! Project L1 `logstat`, error layer redesigned: String and ad-hoc io::Error wrapping replaced by enums.
use std::error::Error;
use std::fmt;
use std::io;
use std::process::ExitCode;

/// Errors from parsing the command line. Each variant is a distinct user mistake.
#[derive(Debug, PartialEq)]
pub enum ArgsError {
    MissingValue { flag: &'static str },
    NotANumber { flag: &'static str, value: String },
    UnknownOption(String),
}

impl fmt::Display for ArgsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingValue { flag } => write!(f, "{flag} needs a value"),
            Self::NotANumber { flag, value } => write!(f, "{flag} expects a number, got {value:?}"),
            Self::UnknownOption(flag) => write!(f, "unknown option {flag:?}"),
        }
    }
}

impl Error for ArgsError {}

/// Everything that can make a `logstat` run fail. Context (which file?) lives in the variant,
/// and the underlying io::Error stays reachable through `source()` instead of being flattened into text.
#[derive(Debug)]
pub enum LogstatError {
    Usage(ArgsError),
    Open { path: String, source: io::Error },
    Read { path: String, source: io::Error },
    Write(io::Error),
}

impl fmt::Display for LogstatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(e) => write!(f, "{e}"),
            Self::Open { path, .. } => write!(f, "cannot open {path}"),
            Self::Read { path, .. } => write!(f, "error while reading {path}"),
            Self::Write(_) => write!(f, "cannot write the report"),
        }
    }
}

impl Error for LogstatError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Usage(e) => e.source(), // transparent: Display already printed `e`
            Self::Open { source, .. } | Self::Read { source, .. } | Self::Write(source) => Some(source),
        }
    }
}

impl From<ArgsError> for LogstatError {
    fn from(e: ArgsError) -> Self {
        Self::Usage(e)
    }
}

impl LogstatError {
    /// The process contract: 2 = you called me wrong, 1 = I failed at run time.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::Open { .. } | Self::Read { .. } | Self::Write(_) => 1,
        }
    }

    /// `logstat big.log | head -3` closes stdout early. That is the reader's choice, not our failure.
    pub fn is_benign(&self) -> bool {
        matches!(self, Self::Write(e) if e.kind() == io::ErrorKind::BrokenPipe)
    }
}

/// Render "logstat: <error>: <cause>: <cause>" on one line, the way CLI users expect.
pub fn render(e: &dyn Error) -> String {
    let mut line = format!("logstat: {e}");
    let mut cause = e.source();
    while let Some(c) = cause {
        line.push_str(&format!(": {c}"));
        cause = c.source();
    }
    line
}

pub fn parse_top(argv: &[&str]) -> Result<usize, ArgsError> {
    let mut top = 5;
    let mut i = 0;
    while i < argv.len() {
        match argv[i] {
            "--top" => {
                let value = argv.get(i + 1).ok_or(ArgsError::MissingValue { flag: "--top" })?;
                top = value
                    .parse()
                    .map_err(|_| ArgsError::NotANumber { flag: "--top", value: value.to_string() })?;
                i += 1;
            }
            flag if flag.starts_with('-') && flag != "-" => return Err(ArgsError::UnknownOption(flag.to_string())),
            _ => {}
        }
        i += 1;
    }
    Ok(top)
}

fn open(path: &str) -> Result<std::fs::File, LogstatError> {
    std::fs::File::open(path).map_err(|source| LogstatError::Open { path: path.to_string(), source })
}

/// One run: `?` converts ArgsError into LogstatError through the From impl.
fn run(argv: &[&str], path: &str, simulate_closed_pipe: bool) -> Result<usize, LogstatError> {
    let top = parse_top(argv)?;
    let _file = open(path)?;
    if simulate_closed_pipe {
        return Err(LogstatError::Write(io::Error::from(io::ErrorKind::BrokenPipe)));
    }
    Ok(top)
}

fn finish(result: Result<usize, LogstatError>) -> ExitCode {
    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) if e.is_benign() => ExitCode::SUCCESS,
        Err(e) => {
            println!("{}   (exit {})", render(&e), e.exit_code()); // a real CLI writes this to stderr
            ExitCode::from(e.exit_code())
        }
    }
}

fn main() {
    let cases: [(&[&str], &str, bool); 5] = [
        (&["--top"], "/dev/null", false),
        (&["--top", "ten"], "/dev/null", false),
        (&["--verbose"], "/dev/null", false),
        (&["--top", "3"], "/var/log/missing-access.log", false),
        (&["--top", "3"], "/dev/null", true),
    ];
    for (argv, path, closed) in cases {
        let code = finish(run(argv, path, closed));
        if code == ExitCode::SUCCESS {
            println!("{argv:?} {path}: success (closed pipe is not an error)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_errors_are_values_you_can_assert_on() {
        assert_eq!(parse_top(&["--top"]), Err(ArgsError::MissingValue { flag: "--top" }));
        assert_eq!(parse_top(&["--top", "x"]), Err(ArgsError::NotANumber { flag: "--top", value: "x".into() }));
        assert_eq!(parse_top(&["-v"]), Err(ArgsError::UnknownOption("-v".into())));
        assert_eq!(parse_top(&["--top", "7", "-"]), Ok(7));
    }

    #[test]
    fn exit_codes_follow_the_contract() {
        assert_eq!(LogstatError::from(ArgsError::UnknownOption("-x".into())).exit_code(), 2);
        let open = LogstatError::Open { path: "x".into(), source: io::Error::from(io::ErrorKind::NotFound) };
        assert_eq!(open.exit_code(), 1);
        assert!(LogstatError::Write(io::Error::from(io::ErrorKind::BrokenPipe)).is_benign());
    }
}
