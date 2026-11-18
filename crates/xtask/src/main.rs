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

        /// Installers to create from the package, can be given multiple times.
        #[arg(long, value_enum)]
        installer: Vec<package::Installer>,
    },
    /// Runs `cargo publish --dry-run` for every library crate, in dependency order.
    Publish,
    /// Runs a smoke test of the packaged tool binaries.
    TestTools {
        /// Directory containing the tools, e.g., `package`. When omitted the tools are
        /// searched for on the `PATH`, which tests an installed package.
        directory: Option<PathBuf>,

        /// Directory containing the examples, defaults to `<directory>/../examples`, or
        /// `examples` when no directory is given.
        #[arg(long)]
        examples: Option<PathBuf>,

        /// Do not expect the gui tools to be present.
        #[arg(long)]
        no_gui: bool,

        /// Do not expect the mCRL2 tools to be present.
        #[arg(long)]
        no_mcrl2: bool,
    },
}

fn main() -> Result<ExitCode, Box<dyn Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Coverage { args } => coverage::coverage(args)?,
        Commands::AddressSanitizer { args } => sanitizer::address_sanitizer(args)?,
        Commands::ThreadSanitizer { args } => sanitizer::thread_sanitizer(args)?,
        Commands::DiscoverTests => discover_tests::discover_tests()?,
        Commands::Package {
            no_gui,
            no_mcrl2,
            installer,
        } => package::package(!no_mcrl2, !no_gui, &installer)?,
        Commands::Publish => publish::publish_crates()?,
        Commands::TestTools {
            directory,
            examples,
            no_gui,
            no_mcrl2,
        } => tool_testing::test_tools(directory.as_deref(), examples.as_deref(), !no_mcrl2, !no_gui)?,
    }

    Ok(ExitCode::SUCCESS)
}
