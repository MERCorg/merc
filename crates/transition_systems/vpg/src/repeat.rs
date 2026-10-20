use std::fmt;

/// Utility to print a repeated static string a given number of times.
pub struct Repeat {
    s: &'static str,
    times: usize,
}

impl Repeat {
    /// Creates a new Repeat instance.
    pub fn new(s: &'static str, times: usize) -> Self {
        Self { s, times }
    }
}

impl fmt::Display for Repeat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for _ in 0..self.times {
            f.write_str(self.s)?;
        }
        Ok(())
    }
}
