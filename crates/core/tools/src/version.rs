use std::fmt;

use clap::Args;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const BUILD_HASH: &str = env!("BUILD_HASH");

#[derive(Args, Clone, Copy, Debug)]
pub struct VersionFlag {
    #[arg(
        long,
        global = true,
        default_value_t = false,
        help = "Print the version of this tool"
    )]
    version: bool,
}

impl From<VersionFlag> for bool {
    fn from(val: VersionFlag) -> Self {
        val.version
    }
}

pub struct Version;

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Show a short hash, but fall back to the whole string when it is
        // shorter than eight bytes. This is currently the UNKNOWN placeholder.
        let short_hash = BUILD_HASH.get(..8).unwrap_or(BUILD_HASH);
        write!(f, "{VERSION}-{short_hash}")
    }
}
