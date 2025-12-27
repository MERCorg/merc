//!
//! `xtask` is a crate that can be used to enable `make`-like commands in cargo. These commands are then implemented in Rust.
//!

#![forbid(unsafe_code)]

use std::error::Error;
use std::process::ExitCode;

use benchmark::Rewriter;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod benchmark;
mod coverage;
mod discover_tests;
mod package;
mod publish;
mod sanitizer;
mod test_tools;

#[derive(Parser)]
#[command(name = "xtask")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Benchmark {
        rewriter: String,
        output: PathBuf,
    },
    CreateTable {
        input: PathBuf,
    },
    Coverage {
        #[clap(trailing_var_arg = true)]
        args: Vec<String>,
    },
    AddressSanitizer {
        #[clap(trailing_var_arg = true)]
        args: Vec<String>,
    },
    ThreadSanitizer {
        #[clap(trailing_var_arg = true)]
        args: Vec<String>,
    },
    DiscoverTests,
    Package,
    Publish,
    TestTools {
        directory: PathBuf,
    },
}

fn main() -> Result<ExitCode, Box<dyn Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Benchmark { rewriter, output } => {
            let rewriter = rewriter.parse::<Rewriter>()?;
            benchmark::benchmark(output.to_string_lossy().into_owned(), rewriter)?;
        }
        Commands::CreateTable { input } => {
            benchmark::create_table(input.to_string_lossy().into_owned())?;
        }
        Commands::Coverage { args } => coverage::coverage(args)?,
        Commands::AddressSanitizer { args } => sanitizer::address_sanitizer(args)?,
        Commands::ThreadSanitizer { args } => sanitizer::thread_sanitizer(args)?,
        Commands::DiscoverTests => discover_tests::discover_tests()?,
        Commands::Package => package::package()?,
        Commands::Publish => publish::publish_crates(),
        Commands::TestTools { directory } => test_tools::test_tools(directory.as_path())?,
    }

    Ok(ExitCode::SUCCESS)
}
