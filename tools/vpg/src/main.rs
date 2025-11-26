use std::fs::File;
use std::process::ExitCode;

use clap::Parser;
use clap::Subcommand;

use log::debug;
use merc_tools::VerbosityFlag;
use merc_tools::Version;
use merc_tools::VersionFlag;
use merc_unsafety::print_allocator_metrics;
use merc_utilities::MercError;
use merc_utilities::Timing;
use merc_vpg::read_pg;
use merc_vpg::solve_zielonka;

#[derive(clap::Parser, Debug)]
#[command(name = "Maurice Laveaux", about = "A command line variability parity game tool")]
struct Cli {
    #[command(flatten)]
    version: VersionFlag,

    #[command(flatten)]
    verbosity: VerbosityFlag,

    #[arg(long, global = true)]
    timings: bool,

    #[command(subcommand)]
    commands: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Solve(SolveArgs),
    Reachable(ReachableArgs),
}

/// Arguments for solving a parity game
#[derive(clap::Args, Debug)]
struct SolveArgs {
    filename: String,
}

/// Arguments for solving a parity game
#[derive(clap::Args, Debug)]
struct ReachableArgs {
    filename: String,
    output: String,
}

fn main() -> Result<ExitCode, MercError> {
    let cli = Cli::parse();

    let mut timing = Timing::new();

    env_logger::Builder::new()
        .filter_level(cli.verbosity.log_level_filter())
        .parse_default_env()
        .init();

    if cli.version.into() {
        eprintln!("{}", Version);
        return Ok(ExitCode::SUCCESS);
    }

    if let Some(command) = cli.commands {
        match command {
            Commands::Solve(args) => {
                let mut time_read = timing.start("read_pg");
                let mut file = File::open(&args.filename)?;
                let game = read_pg(&mut file)?;
                time_read.finish();

                let mut time_solve = timing.start("solve_zielonka");
                println!("{}", solve_zielonka(&game).solution());
                time_solve.finish();
            }
            Commands::Reachable(args) => {
                let mut time_read = timing.start("read_pg");
                let mut file = File::open(&args.filename)?;
                let game = read_pg(&mut file)?;
                time_read.finish();

                let mut time_reachable = timing.start("compute_reachable");
                let (reachable_game, mapping) = merc_vpg::compute_reachable(&game);
                time_reachable.finish();

                for (old_index, new_index) in mapping.iter().enumerate() {
                    debug!("{} -> {}", old_index, new_index);
                }

                let mut output_file = File::create(&args.output)?;
                merc_vpg::write_pg(&mut output_file, &reachable_game)?;
            }
        }
    }

    if cli.timings {
        timing.print();
    }

    print_allocator_metrics();
    Ok(ExitCode::SUCCESS)
}
