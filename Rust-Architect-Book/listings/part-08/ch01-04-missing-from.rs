// verify: debug error:E0277
#[derive(Debug)]
enum ConfigError {
    Missing(&'static str),
}

fn parse_port(raw: Option<&str>) -> Result<u16, ConfigError> {
    let raw = raw.ok_or(ConfigError::Missing("port"))?;
    let port = raw.parse::<u16>()?; // no From<ParseIntError> for ConfigError
    Ok(port)
}

fn main() {
    println!("{:?}", parse_port(Some("80")));
}
