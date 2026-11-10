use std::fs::File;
use std::io::BufReader;
use std::io::BufWriter;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use clap::Subcommand;
use log::info;

use merc_explore::CachingStrategy;
use merc_explore::ExplorationStrategy;
use merc_lts::AutFormat;
use merc_lts::AutStream;
use merc_lts::LtsFormat;
use merc_lts::LtsStream;
use merc_lts::MutexLtsBuilder;
use merc_lts::guess_lts_output_format;
use merc_symbolic::ExplorationArgs;
use merc_symbolic::LddLenCache;
use merc_symbolic::MaxIterationsArgs;
use merc_symbolic::OxiddArgs;
use merc_symbolic::ReachabilityOptions;
use merc_symbolic::ReorderArgs;
use merc_symbolic::SummandGrouping;
use merc_symbolic::SymbolicLTS;
use merc_symbolic::SymbolicLpsOptions;
use merc_symbolic::ldd_len;
use merc_symbolic::write_symbolic_lts;
use merc_tools::VerbosityFlag;
use merc_tools::Version;
use merc_tools::VersionFlag;
use merc_tools::report_error;
use merc_unsafety::print_allocator_metrics;
use merc_utilities::MercError;
use merc_utilities::Timing;

use merc_lps::DEFAULT_PREPROCESSING;
use merc_lps::LpsMultiActionLabel;
use merc_lps::LtsMultiActionAdapter;
use merc_lps::Rewriter;
use merc_lps::TransformedLps;
use merc_lps::explore_lps_explicit;
use merc_lps::explore_lps_explicit_parallel;
use merc_lps::explore_lps_symbolic;
use merc_lps::explore_lps_symbolic_to_sym;
use merc_lps::preprocess;
use merc_lps::read_lps;

/// A command line tool for linear process specifications (LPSs)
#[derive(clap::Parser, Debug)]
#[command(arg_required_else_help = true)]
struct Cli {
    #[command(flatten)]
    version: VersionFlag,

    #[command(flatten)]
    verbosity: VerbosityFlag,

    #[arg(long, global = true)]
    timings: bool,

    /// Skip the preprocessing that mCRL2 applies to an LPS before exploring it.
    #[arg(long, global = true, default_value_t = false)]
    no_preprocess: bool,

    /// The rewrite engine used to evaluate data expressions.
    #[arg(long, global = true, value_enum, default_value_t = Rewriter::default())]
    rewriter: Rewriter,

    #[command(flatten)]
    oxidd: OxiddArgs,

    #[command(subcommand)]
    commands: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Explores the state space of an LPS symbolically
    Explore(ExploreArgs),
    /// Explores the state space of an LPS explicitly
    ExploreExplicit(ExploreExplicitArgs),
    /// Prints an LPS as mCRL2 text
    Print(InputArgs),
}

/// The input LPS shared by every subcommand.
#[derive(clap::Args, Debug)]
struct InputArgs {
    /// The input LPS file.
    filename: String,
}

impl InputArgs {
    /// Reads the binary LPS and completes its data specification. If `preprocess_lps` is true, the
    /// LPS is preprocessed using the default preprocessing options.
    fn read(&self, timing: &Timing, preprocess_lps: bool, rewriter: Rewriter) -> Result<TransformedLps, MercError> {
        let lps = timing.measure("load LPS", || read_lps(BufReader::new(File::open(&self.filename)?)))?;
        let lps = timing
            .measure("complete data specification", || TransformedLps::new(&lps))?
            .with_rewriter(rewriter);

        if preprocess_lps {
            timing.measure("preprocess LPS", || preprocess(lps, DEFAULT_PREPROCESSING))
        } else {
            info!("Skipping LPS preprocessing (--no-preprocess)");
            Ok(lps)
        }
    }
}

#[derive(clap::Args, Debug)]
struct ExploreArgs {
    #[command(flatten)]
    input: InputArgs,

    #[command(flatten)]
    exploration: ExplorationArgs,

    /// How the summands are distributed over the transition groups: 'none' (one group per summand),
    /// 'used' (join summands using the same parameters), 'simple' (join summands with the same
    /// read/write pattern) or a partition of the summand indices, e.g. '0; 1 3 4; 2 5'.
    #[arg(long, default_value_t = SummandGrouping::default(), value_parser = parse_grouping)]
    groups: SummandGrouping,

    #[command(flatten)]
    reorder: ReorderArgs,

    /// Detect and report deadlock states (reachable states with no outgoing transition).
    #[arg(long)]
    deadlocks: bool,

    /// Cache the domain of every transition relation, so that successors are only learned for
    /// process parameter values that a group has not seen before.
    #[arg(long)]
    cached: bool,

    #[command(flatten)]
    max_iterations: MaxIterationsArgs,

    /// Write the reachable symbolic LTS to this .sym file, including the data specification,
    /// process parameters, parameter values and action labels. If not given, the LTS is not written.
    #[arg(long, short('o'))]
    output: Option<PathBuf>,
}

/// Parses the `--groups` argument, since [`MercError`] is not a [`std::error::Error`] that clap accepts.
fn parse_grouping(text: &str) -> Result<SummandGrouping, String> {
    text.parse::<SummandGrouping>().map_err(|error| error.to_string())
}

#[derive(clap::Args, Debug)]
struct ExploreExplicitArgs {
    #[command(flatten)]
    input: InputArgs,

    /// Explicitly specify the output LTS file format.
    #[arg(long, short('o'), value_enum)]
    out_format: Option<LtsFormat>,

    /// Specify the output LTS. If not given, the LTS is not written.
    #[arg(long)]
    output: Option<PathBuf>,

    #[arg(long, short('c'), value_enum, default_value_t = CachingStrategy::None)]
    caching: CachingStrategy,

    /// Order in which discovered states are explored.
    #[arg(long, short('s'), value_enum, default_value_t = ExplorationStrategy::Dfs)]
    strategy: ExplorationStrategy,

    /// Use a control flow graph analysis to prune summands whose control flow
    /// guard cannot hold in the current state. The explored transition system is
    /// unchanged.
    #[arg(long)]
    control_flow: bool,

    /// Number of worker threads used for exploration.
    #[arg(long, default_value_t = 1)]
    threads: usize,

    /// Pin each worker thread round-robin to the available CPU cores.
    #[arg(long, default_value_t = false)]
    pinned: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    env_logger::Builder::new()
        .filter_level(cli.verbosity.log_level_filter())
        .parse_default_env()
        .init();

    if cli.version.into() {
        eprintln!("{}", Version);
        return ExitCode::SUCCESS;
    }

    let timing = Timing::new();
    let result = handle_command(&cli, &timing);

    if cli.timings {
        timing.print();
    }

    print_allocator_metrics();
    report_error(result)
}

fn handle_command(cli: &Cli, timing: &Timing) -> Result<(), MercError> {
    let preprocess_lps = !cli.no_preprocess;

    if let Some(command) = &cli.commands {
        match command {
            Commands::Explore(args) => handle_explore(cli, args, timing, preprocess_lps)?,
            Commands::ExploreExplicit(args) => handle_explore_explicit(args, timing, preprocess_lps, cli.rewriter)?,
            Commands::Print(args) => print!("{}", read_lps(BufReader::new(File::open(&args.filename)?))?),
        }
    }

    Ok(())
}

/// Handles symbolic exploration of an LPS.
fn handle_explore(cli: &Cli, args: &ExploreArgs, timing: &Timing, preprocess_lps: bool) -> Result<(), MercError> {
    let rewriter = cli.rewriter;
    let lps = args.input.read(timing, preprocess_lps, rewriter)?;

    let storage = cli.oxidd.init_ldd_manager();

    let options = ReachabilityOptions {
        strategy: args.exploration.strategy,
        detect_deadlocks: args.deadlocks,
        cached: args.cached,
    };

    let encoding = SymbolicLpsOptions {
        grouping: args.groups.clone(),
        order: args.reorder.variable_order()?,
    };

    let mut stopped_after = None;
    let on_iteration = args.max_iterations.on_iteration(&mut stopped_after);

    if let Some(output) = &args.output {
        let (lts, deadlocks) = explore_lps_symbolic_to_sym(&storage, lps, &encoding, &options, timing, on_iteration)?;
        println!("Number of states: {}", ldd_len(lts.states(), &mut LddLenCache::new()));
        if let Some(deadlocks) = &deadlocks {
            println!("Number of deadlocks: {}", ldd_len(deadlocks, &mut LddLenCache::new()));
        }

        let mut file = BufWriter::new(File::create(output)?);
        write_symbolic_lts(&storage, &mut file, &lts)?;
    } else {
        let result = explore_lps_symbolic(&storage, lps, &encoding, &options, timing, on_iteration)?;
        println!("Number of states: {}", ldd_len(&result.states, &mut LddLenCache::new()));
        if let Some(deadlocks) = &result.deadlocks {
            println!("Number of deadlocks: {}", ldd_len(deadlocks, &mut LddLenCache::new()));
        }
    }

    MaxIterationsArgs::warn_if_stopped(stopped_after);

    Ok(())
}

/// Handles the explicit exploration of an LPS.
fn handle_explore_explicit(
    args: &ExploreExplicitArgs,
    timing: &Timing,
    preprocess_lps: bool,
    rewriter: Rewriter,
) -> Result<(), MercError> {
    let lps = args.input.read(timing, preprocess_lps, rewriter)?;

    let output_format = guess_lts_output_format(args.output.as_deref(), args.out_format, LtsFormat::Aut);

    // The binary `.lts` format additionally carries a data specification, so it's handled
    // separately (see `handle_explore_explicit_lts`).
    if output_format == LtsFormat::Lts {
        return handle_explore_explicit_lts(args, lps, timing);
    }

    let aut_format = match output_format {
        LtsFormat::Aut => AutFormat::Aut,
        LtsFormat::AutMcrl2 => AutFormat::AutMcrl2,
        LtsFormat::Bcg => {
            return Err(
                "BCG output is not supported by explore-explicit; write AUT or .lts and convert with merc-lts.".into(),
            );
        }
        LtsFormat::Lts => unreachable!("handled above"),
    };

    if args.threads > 1 {
        // Parallel exploration adds transitions concurrently.
        if let Some(output) = &args.output {
            let mut file = BufWriter::new(File::create(output)?);
            let mut builder = MutexLtsBuilder::new(AutStream::with_format(&mut file, aut_format)?);
            explore_lps_explicit_parallel(
                &mut builder,
                lps,
                args.caching,
                args.threads,
                args.control_flow,
                args.pinned,
                timing,
            )?;
        } else {
            // No output requested, discard the explored transitions.
            explore_lps_explicit_parallel(
                &mut (),
                lps,
                args.caching,
                args.threads,
                args.control_flow,
                args.pinned,
                timing,
            )?;
        }
    } else if let Some(output) = &args.output {
        let mut file = BufWriter::new(File::create(output)?);
        let mut builder: AutStream<_, LpsMultiActionLabel> = AutStream::with_format(&mut file, aut_format)?;
        explore_lps_explicit(
            &mut builder,
            lps,
            args.caching,
            args.strategy,
            args.control_flow,
            timing,
        )?;
    } else {
        // No output requested, discard the explored transitions.
        let mut builder: () = ();
        explore_lps_explicit(
            &mut builder,
            lps,
            args.caching,
            args.strategy,
            args.control_flow,
            timing,
        )?;
    }

    Ok(())
}

/// Explores `lps` explicitly and streams the result straight to disk in the binary mCRL2 `.lts`
/// format via [`LtsStream`].
fn handle_explore_explicit_lts(
    args: &ExploreExplicitArgs,
    lps: TransformedLps,
    timing: &Timing,
) -> Result<(), MercError> {
    let Some(output) = &args.output else {
        // No output requested: still explore (for the stats and timing), but discard the
        // transitions rather than converting and writing them for nothing.
        if args.threads > 1 {
            explore_lps_explicit_parallel(
                &mut (),
                lps,
                args.caching,
                args.threads,
                args.control_flow,
                args.pinned,
                timing,
            )?;
        } else {
            explore_lps_explicit(&mut (), lps, args.caching, args.strategy, args.control_flow, timing)?;
        }
        return Ok(());
    };

    // The data specification never changes during exploration, so convert it once up front,
    // before `lps` is consumed by the explorer below. `LtsStream::new` writes it immediately, as
    // the format's header.
    let data_spec = lps.lps().data_specification.clone();
    let stream = LtsStream::new(File::create(output)?, &data_spec)?;

    if args.threads > 1 {
        let mut builder = MutexLtsBuilder::new(LtsMultiActionAdapter::new(stream));
        explore_lps_explicit_parallel(
            &mut builder,
            lps,
            args.caching,
            args.threads,
            args.control_flow,
            args.pinned,
            timing,
        )?;
    } else {
        let mut builder = LtsMultiActionAdapter::new(stream);
        explore_lps_explicit(
            &mut builder,
            lps,
            args.caching,
            args.strategy,
            args.control_flow,
            timing,
        )?;
    }

    Ok(())
}
