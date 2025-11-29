use merc_utilities::MercError;


#[derive(clap::Parser, Debug)]
#[command(about = "A command line tool for variability parity games", arg_required_else_help = true)]
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
    Symmetry(SymmetryArgs),
}

/// Arguments for solving a parity game
#[derive(clap::Args, Debug)]
struct SymmetryArgs {
    filename: String,

    format: Option<ParityGameFormat>,
}

fn main() -> Result<(), MercError> {
    let cli = Cli::parse();

    env_logger::Builder::new()
        .filter_level(cli.verbosity.log_level_filter())
        .parse_default_env()
        .init();

    if cli.version.into() {
        eprintln!("{}", Version);
        return Ok(ExitCode::SUCCESS);
    }

    let mut timing = Timing::new();

    
    if cli.timings {
        timing.print();
    }

    Ok(())
}