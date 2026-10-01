//! Build-automation commands for the merc workspace (sanitizers, coverage, packaging, tool
//! smoke tests), invoked as `cargo xtask <command>`.

#![forbid(unsafe_code)]

use std::error::Error;
use std::process::ExitCode;

use clap::Parser;
use clap::Subcommand;
use std::path::PathBuf;

mod coverage;
mod discover_tests;
mod package;
mod publish;
mod sanitizer;
mod tool_testing;

#[derive(Parser)]
#[command(name = "xtask")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generates a code coverage report using grcov.
    Coverage {
        #[clap(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Runs the given cargo command with AddressSanitizer enabled.
    AddressSanitizer {
        #[clap(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Runs the given cargo command with ThreadSanitizer enabled.
    ThreadSanitizer {
        #[clap(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Discovers tests from the examples folder, and prints them as a `#[test_case]` annotation.
    DiscoverTests,
    /// Builds and packages the binaries for release.
    Package {
        #[arg(long)]
        no_gui: bool,

        #[arg(long)]
        no_mcrl2: bool,
    },
    /// Runs `cargo publish --dry-run` for every library crate, in dependency order.
    Publish,
    /// Runs a smoke test of the packaged tool binaries found in the given directory.
    TestTools { directory: PathBuf },
}

fn main() -> Result<ExitCode, Box<dyn Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Coverage { args } => coverage::coverage(args)?,
        Commands::AddressSanitizer { args } => sanitizer::address_sanitizer(args)?,
        Commands::ThreadSanitizer { args } => sanitizer::thread_sanitizer(args)?,
        Commands::DiscoverTests => discover_tests::discover_tests()?,
        Commands::Package { no_gui, no_mcrl2 } => package::package(!no_mcrl2, !no_gui)?,
        Commands::Publish => publish::publish_crates()?,
        Commands::TestTools { directory } => tool_testing::test_tools(directory.as_path())?,
    }

    Ok(ExitCode::SUCCESS)
}
