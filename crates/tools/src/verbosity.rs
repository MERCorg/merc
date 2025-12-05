use clap::Args;
use log::LevelFilter;

#[derive(Args, Debug)]
pub struct VerbosityFlag {
    #[arg(
        short,
        long,
        global = true,
        default_value_t = false,
        help = "Set the verbosity to quiet"
    )]
    quiet: bool,

    #[arg(
        short,
        long,
        global = true,
        default_value_t = false,
        help = "Set the verbosity to verbose (default)"
    )]
    verbose: bool,

    #[arg(
        short,
        long,
        global = true,
        default_value_t = false,
        help = "Set the verbosity to debug"
    )]
    debug: bool,

    #[arg(long, global = true, default_value_t = false, help = "Set the verbosity to trace")]
    trace: bool,
}

impl VerbosityFlag {
    /// Returns the log level filter corresponding to the given verbosity flags.
    pub fn log_level_filter(&self) -> LevelFilter {
        self.verbosity().log_level_filter()
    }

    /// Returns the verbosity level corresponding to the given verbosity flags.
    pub fn verbosity(&self) -> Verbosity {
        if self.quiet {
            Verbosity::Quiet
        } else if self.trace {
            Verbosity::Trace
        } else if self.debug {
            Verbosity::Debug
        } else if self.verbose {
            Verbosity::Verbose
        } else {
            // Default verbosity level
            Verbosity::Verbose
        }
    }
}

#[derive(Debug, Clone)]
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
