//!
//! `xtask` is a crate that can be used to enable `make`-like commands in cargo. These commands are then implemented in Rust.
//!

#![forbid(unsafe_code)]

use std::env;
use std::error::Error;
use std::process::ExitCode;

use benchmark::Rewriter;

mod benchmark;
mod coverage;
mod discover_tests;
mod package;
mod sanitizer;

fn main() -> Result<ExitCode, Box<dyn Error>> {
    let mut args = env::args();

    // Ignore the first argument (which should be xtask)
    args.next();

    // The name of the task
    let task = args.next();

    match task.as_deref() {
        Some("benchmark") => {
            if let Some(rewriter) = args.next() {
                // Use the upstream mcrl2
                if let Some(output_path) = args.next() {
                    benchmark::benchmark(output_path, Rewriter::from_str(&rewriter)?)?
                } else {
                    println!("Missing argument for output file");
                    return Ok(ExitCode::FAILURE);
                }
            } else {
                println!("Missing argument for rewriter");
                return Ok(ExitCode::FAILURE);
            }
        }
        Some("create-table") => {
            if let Some(input_path) = args.next() {
                benchmark::create_table(input_path)?;
            } else {
                println!("Missing argument for input file");
                return Ok(ExitCode::FAILURE);
            }
        }
        Some("coverage") => coverage::coverage(args.collect())?,
        Some("address-sanitizer") => sanitizer::address_sanitizer(args.collect())?,
        Some("thread-sanitizer") => sanitizer::thread_sanitizer(args.collect())?,
        Some("discover-tests") => discover_tests::discover_tests()?,
        Some("package") => package::package()?,
        Some(x) => {
            println!("Unknown task {x}");
            println!();
            print_help();
            return Ok(ExitCode::FAILURE);
        }
        _ => print_help(),
    }

    Ok(ExitCode::SUCCESS)
}

/// Print the help message.
fn print_help() {
    println!(
        "Available tasks: benchmark, discover-tests, coverage <cargo_args>, address-sanitizer <cargo_args>, thread-sanitizer <cargo_args>, package"
    );
}
