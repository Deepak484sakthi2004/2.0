// verify: debug build
// The quota-service PR from the Part II review. It compiles; that is the point: every problem in it is a
// problem the compiler cannot see (or was told not to look for).
pub mod quota {
    pub struct Quota {
        pub used: u32,
        pub limit: u32,
    }

    impl Quota {
        pub fn consume(&mut self, n: u64) {
            debug_assert!(self.used as u64 + n <= self.limit as u64, "over quota");
            self.used += n as u32;
        }
    }

    pub enum Command {
        Consume { tenant: String, n: u64 },
        Reset { tenant: String },
        Snapshot([u8; 65536]),
    }

    pub fn validate(cmd: &Command) -> Result<(), String> {
        match cmd {
            Command::Consume { n, .. } if *n == 0 => Err("empty consume".to_string()),
            _ => Ok(()), // other commands don't need validation
        }
    }
}
