use std::ffi::OsStr;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use clap::Subcommand;
use log::info;
use log::warn;

use merc_data::DataExpression;
use merc_rec_tests::load_rec_from_file;
use merc_rewrite::Rewriter;
use merc_rewrite::rewrite_rec;
use merc_rewrite::rewrite_terms;
use merc_sabre::RewriteSpecification;
use merc_syntax::DataExpr;
use merc_syntax::SourceMap;
use merc_syntax::UntypedDataSpecification;
use merc_tools::VerbosityFlag;
use merc_tools::Version;
use merc_tools::VersionFlag;
use merc_tools::report_error;
use merc_typecheck::DataSpecification;
use merc_typecheck::NumberEncoding;
use merc_unsafety::print_allocator_metrics;
use merc_utilities::MercError;
use merc_utilities::Timing;
use merc_utilities::silence_ena_logging;

mod trs_format;

pub use trs_format::*;

/// A command line rewriting tool
#[derive(clap::Parser, Debug)]
#[command(arg_required_else_help = true)]
struct Cli {
    #[command(flatten)]
    version: VersionFlag,

    #[command(flatten)]
    verbosity: VerbosityFlag,

    #[command(subcommand)]
    commands: Option<Commands>,

    #[arg(long, global = true)]
    timings: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Rewrite a term using the rewrite rules specified in a REC file or mCRL2 specification
    Rewrite(RewriteArgs),

    /// Convert a REC specification to the TRS format, which is the format used
    /// by the term rewrite system termination checking tool called AProVE.
    Convert(ConvertArgs),

    /// Parse an mCRL2 data specification and print the stages of the pipeline:
    /// the parsed AST, the resolved and desugared intermediate representation,
    /// and the fully typed and lowered data specification.
    Check(CheckArgs),
}

#[derive(Debug, clap::ValueEnum, Clone)]
enum Format {
    /// The REC format, which is the native format of this tool.
    Rec,
    /// The mCRL2 format, which is the format used by the mCRL2 toolset.
    Mcrl2,
}

#[derive(clap::Args, Debug)]
struct RewriteArgs {
    rewriter: Rewriter,

    /// The REC specification that contains the rewrite rules.
    #[arg(value_name = "SPEC")]
    specification: PathBuf,

    /// File containing the terms to be rewritten. For an mCRL2 specification
    /// this is a file of mCRL2 data expressions, one per line; blank lines and
    /// lines starting with `%` are ignored. Ignored for a REC specification,
    /// which carries its own terms.
    terms: Option<PathBuf>,

    /// An mCRL2 data expression to rewrite, type checked and lowered against
    /// the specification. May be repeated; combines with `TERMS`. Only
    /// supported for an mCRL2 specification.
    #[arg(long, short = 'e', value_name = "EXPR")]
    expression: Vec<String>,

    #[arg(long, value_enum)]
    format: Option<Format>,

    /// Print the rewritten term(s)
    #[arg(long)]
    output: bool,
}

#[derive(clap::Args, Debug)]
struct ConvertArgs {
    /// The REC specification that contains the rewrite rules.
    #[arg(value_name = "SPEC")]
    specification: PathBuf,

    /// The output file to write the TRS to.
    output: String,
}

#[derive(clap::Args, Debug)]
struct CheckArgs {
    /// The mCRL2 data specification to check.
    #[arg(value_name = "SPEC")]
    specification: PathBuf,

    /// Print the parsed AST, before name resolution and typechecking.
    #[arg(long)]
    ast: bool,

    /// Print the resolved and desugared intermediate representation, after
    /// typechecking but before Phase-4 lowering.
    #[arg(long)]
    ir: bool,

    /// Print the fully typed and lowered mCRL2 data specification.
    #[arg(long)]
    lowered: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let mut logger_builder = env_logger::Builder::new();
    logger_builder
        .filter_level(cli.verbosity.log_level_filter())
        .parse_default_env();
    silence_ena_logging(&mut logger_builder).init();

    if cli.version.into() {
        eprintln!("{}", Version);
        return ExitCode::SUCCESS;
    }

    let timing = Timing::new();
    let result = handle_command(cli.commands, &timing);

    if cli.timings {
        timing.print();
    }

    print_allocator_metrics();
    report_error(result)
}

/// Reads the mCRL2 data expressions of a terms file, one per line.
///
/// Blank lines and `%`-comment lines (mCRL2's comment syntax) are skipped, so
/// a terms file may be annotated. Returns an empty list when no file is given.
fn read_expressions(path: Option<&Path>) -> Result<Vec<String>, MercError> {
    let Some(path) = path else {
        return Ok(Vec::new());
    };

    let contents = std::fs::read_to_string(path)?;
    Ok(contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('%'))
        .map(str::to_string)
        .collect())
}

/// Type checks `spec`, rendering a well-typedness error against `sources` (populated by whichever
/// `parse_with_imports` call produced `spec`) into a plain [MercError].
fn typecheck_or_render(
    spec: UntypedDataSpecification,
    encoding: NumberEncoding,
    sources: &mut SourceMap,
) -> Result<DataSpecification, MercError> {
    DataSpecification::from_untyped_with(spec, encoding, sources).map_err(|err| err.render(sources).into())
}

/// Parses, type checks and lowers one mCRL2 data expression against `spec`,
/// rendering a parse or type error against the expression text itself.
fn typecheck_expression(spec: &mut DataSpecification, text: &str) -> Result<DataExpression, MercError> {
    let expr = DataExpr::parse(text)?;

    spec.typecheck_expression(&expr).map_err(|err| {
        let mut sources = SourceMap::new();
        sources.add_text("<expression>", text.to_string());
        MercError::from(err.render(&sources))
    })
}

fn handle_command(commands: Option<Commands>, timing: &Timing) -> Result<(), MercError> {
    if let Some(command) = commands {
        match command {
            Commands::Rewrite(args) => run_rewrite(args, timing)?,
            Commands::Convert(args) => run_convert(args)?,
            Commands::Check(args) => run_check(args)?,
        }
    }

    Ok(())
}

/// The `rewrite` command: rewrites the terms of a REC or mCRL2 specification.
fn run_rewrite(args: RewriteArgs, timing: &Timing) -> Result<(), MercError> {
    let format = if let Some(format) = args.format {
        format
    } else if args.specification.extension() == Some(OsStr::new("rec")) {
        Format::Rec
    } else if args.specification.extension() == Some(OsStr::new("mcrl2")) {
        Format::Mcrl2
    } else {
        return Err("Unsupported file extension for rewriting, expected .rec or .mcrl2".into());
    };

    match format {
        Format::Rec => {
            if args.terms.is_some() {
                warn!(
                    "The --terms option is currently ignored when rewriting REC specifications, the terms are taken from the REC spec."
                );
            }
            if !args.expression.is_empty() {
                warn!(
                    "The --expression option is only supported for mCRL2 specifications, the terms are taken from the REC spec."
                );
            }

            let (syntax_spec, syntax_terms) = load_rec_from_file(&args.specification)?;

            let spec = syntax_spec.to_rewrite_spec();

            rewrite_rec(args.rewriter, &spec, &syntax_terms, args.output, timing)?;
        }
        Format::Mcrl2 => {
            let mut sources = SourceMap::new();
            let (untyped_spec, _import_graph) =
                UntypedDataSpecification::parse_with_imports(&args.specification, &mut sources)?;

            let mut data_spec = typecheck_or_render(untyped_spec, NumberEncoding::default(), &mut sources)?;

            // Every term is type checked and lowered against the
            // same specification the rules come from, so the two
            // share one number encoding and one sort lattice.
            let mut terms = Vec::new();
            for text in read_expressions(args.terms.as_deref())?.iter().chain(&args.expression) {
                terms.push(typecheck_expression(&mut data_spec, text)?);
            }

            let mcrl2_spec = data_spec.lower_data_specification();
            let spec = RewriteSpecification::from_data_specification(&mcrl2_spec);
            info!("Loaded {} rewrite rule(s)", spec.rewrite_rules().len());

            if terms.is_empty() {
                warn!("No terms to rewrite; pass --expression or a terms file.");
            }
            rewrite_terms(args.rewriter, &spec, &terms, args.output, timing)?;
        }
    }

    Ok(())
}

/// The `convert` command: converts a REC specification to the TRS format.
fn run_convert(args: ConvertArgs) -> Result<(), MercError> {
    if args.specification.extension() == Some(OsStr::new("rec")) {
        // Read the data specification
        let (spec_text, _) = load_rec_from_file(&args.specification)?;
        let spec = spec_text.to_rewrite_spec();

        let mut output = File::create(args.output)?;
        write!(output, "{}", TrsFormatter::new(&spec))?;
    } else {
        return Err("Unsupported file extension for conversion, expected .rec".into());
    }

    Ok(())
}

/// The `check` command: parses, resolves and type checks an mCRL2 data
/// specification, printing the selected stages of the pipeline.
fn run_check(args: CheckArgs) -> Result<(), MercError> {
    // With none of the stage flags given, show every stage.
    let show_all = !args.ast && !args.ir && !args.lowered;

    let mut sources = SourceMap::new();
    let (untyped_spec, _import_graph) =
        UntypedDataSpecification::parse_with_imports(&args.specification, &mut sources)?;

    if show_all || args.ast {
        println!("=== AST ===\n");
        println!("{untyped_spec}");
    }

    let data_spec = typecheck_or_render(untyped_spec, NumberEncoding::default(), &mut sources)?;

    if show_all || args.ir {
        println!("=== IR (resolved user declarations) ===\n");
        println!("{}", data_spec.data_specification());

        // Basic sorts and desugared structs only: a container/
        // function-update/comparison instantiation is generated at
        // lowering time now, not during type-checking, so it only
        // shows up under `--lowered` below, not here.
        println!("=== IR (system-defined declarations, unmonomorphized) ===\n");
        println!("{}", data_spec.system_defined_specification());
    }

    if show_all || args.lowered {
        let mcrl2_spec = data_spec.lower_data_specification();

        println!("=== Lowered ===\n");
        println!("{mcrl2_spec}");
    }

    eprintln!("The data specification is well-typed.");

    Ok(())
}
