use std::fmt;

use clap::Args;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const BUILD_HASH: &str = env!("BUILD_HASH");

#[derive(Args, Debug)]
pub struct VersionFlag {
    #[arg(
        long,
        global = true,
        default_value_t = false,
        help = "Print the version of this tool"
    )]
    version: bool,
}

impl Into<bool> for VersionFlag {
    fn into(self) -> bool {
        self.version
    }
}

pub struct Version;

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", VERSION, &BUILD_HASH[..8])
    }
}
