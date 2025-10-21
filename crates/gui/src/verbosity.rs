use clap::ValueEnum;
use log::LevelFilter;

#[derive(ValueEnum, Debug, Clone)]
pub enum Verbosity {
    Quiet,
    Verbose,
    Debug,
    Trace,
}

impl std::fmt::Display for Verbosity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Verbosity::Quiet => write!(f, "quiet"),
            Verbosity::Verbose => write!(f, "verbose"),
            Verbosity::Debug => write!(f, "debug"),
            Verbosity::Trace => write!(f, "trace"),
        }
    }
}

impl Verbosity {
    /// Returns the log filter level corresponding to this verbosity.
    pub fn log_level_filter(&self) -> LevelFilter {
        match self {
            Verbosity::Quiet => LevelFilter::Off,
            Verbosity::Verbose => LevelFilter::Info,
            Verbosity::Debug => LevelFilter::Debug,
            Verbosity::Trace => LevelFilter::Trace,
        }
    }
}
