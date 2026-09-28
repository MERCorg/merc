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
use merc_syntax::UntypedPbes;
use merc_syntax::UntypedPres;
use merc_syntax::UntypedProcessSpecification;
use merc_syntax::UntypedStateFrmSpec;
use merc_tools::VerbosityFlag;
use merc_tools::Version;
use merc_tools::VersionFlag;
use merc_tools::report_error;
use merc_typecheck::DataSpecification;
use merc_typecheck::FormulaType;
use merc_typecheck::ModalSpecification;
use merc_typecheck::NumberEncoding;
use merc_typecheck::PbesSpecification;
use merc_typecheck::PresSpecification;
use merc_typecheck::ProcessSpecification;
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

    /// Parse an mCRL2-family specification (`.mcrl2`, `.dataspec`, `.pbes`, `.pres`, `.mcf`)
    /// and print the stages of the pipeline.
    Check(CheckArgs),
}

/// The specification formats this tool can load.
#[derive(Debug, clap::ValueEnum, Clone, Copy, PartialEq, Eq)]
enum SpecFormat {
    /// The REC format, which is the native format of this tool.
    Rec,
    /// A full mCRL2 process specification (`.mcrl2`).
    Mcrl2,
    /// A pure mCRL2 data specification (`.dataspec`).
    DataSpec,
    /// An mCRL2 parameterised boolean equation system (`.pbes`).
    Pbes,
    /// An mCRL2 parameterised real equation system (`.pres`).
    Pres,
    /// An mCRL2 modal (mu-calculus) state formula (`.mcf`).
    Modal,
}

impl SpecFormat {
    /// Detects the format of `path` from its extension, one of `.rec`, `.mcrl2`, `.dataspec`,
    /// `.pbes`, `.pres`, or `.mcf`.
    fn from_path(path: &Path) -> Result<SpecFormat, MercError> {
        match path.extension().and_then(OsStr::to_str) {
            Some("rec") => Ok(SpecFormat::Rec),
            Some("mcrl2") => Ok(SpecFormat::Mcrl2),
            Some("dataspec") => Ok(SpecFormat::DataSpec),
            Some("pbes") => Ok(SpecFormat::Pbes),
            Some("pres") => Ok(SpecFormat::Pres),
            Some("mcf") => Ok(SpecFormat::Modal),
            _ => Err(format!(
                "Unsupported file extension for rewriting: {}; expected one of .rec, .mcrl2, .dataspec, .pbes, .pres, .mcf",
                path.display()
            )
            .into()),
        }
    }
}

#[derive(clap::Args, Debug)]
struct RewriteArgs {
    rewriter: Rewriter,

    /// The specification that contains the rewrite rules: a REC file, or an
    /// mCRL2-family specification (`.mcrl2`, `.dataspec`, `.pbes`, `.pres`,
    /// `.mcf`).
    #[arg(value_name = "SPEC")]
    specification: PathBuf,

    /// File containing the terms to be rewritten. For an mCRL2-family specification this is a
    /// file of mCRL2 data expressions, one per line; blank lines and lines starting with `%` are
    /// ignored. Ignored for a REC specification, which carries its own terms.
    terms: Option<PathBuf>,

    /// An mCRL2 data expression to rewrite, type checked and lowered against the
    /// specification's data specification. May be repeated; combines with `TERMS`. Only
    /// supported for an mCRL2-family specification.
    #[arg(long, short = 'e', value_name = "EXPR")]
    expression: Vec<String>,

    #[arg(long, value_enum)]
    format: Option<SpecFormat>,

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
    /// The mCRL2-family specification to check (`.mcrl2`, `.dataspec`, `.pbes`, `.pres`,
    /// `.mcf`); the REC format is not supported here.
    #[arg(value_name = "SPEC")]
    specification: PathBuf,

    /// Print the parsed AST, before name resolution and typechecking.
    #[arg(long)]
    ast: bool,

    /// Print the resolved and desugared intermediate representation of the specification's
    /// contained data specification, after typechecking but before Phase-4 lowering.
    #[arg(long)]
    ir: bool,

    /// Print the fully typed and lowered mCRL2 data specification contained in the
    /// specification.
    #[arg(long)]
    lowered: bool,

    #[arg(long, value_enum)]
    format: Option<SpecFormat>,
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

/// Loads and type checks `path` as the given mCRL2-family `format` and returns
/// its data specification.
fn load_data_specification(
    format: SpecFormat,
    path: &Path,
    encoding: NumberEncoding,
    sources: &mut SourceMap,
) -> Result<DataSpecification, MercError> {
    match format {
        SpecFormat::Rec => unreachable!("the REC format is handled separately, never through this function"),
        SpecFormat::DataSpec => {
            let (spec, _import_graph) = UntypedDataSpecification::parse_with_imports(path, sources)?;
            typecheck_or_render(spec, encoding, sources)
        }
        SpecFormat::Mcrl2 => {
            let text = std::fs::read_to_string(path)?;
            let (spec, _import_graph) = UntypedProcessSpecification::parse_with_imports(path, &text, sources)?;
            let process_spec =
                ProcessSpecification::from_untyped_with(spec, encoding, sources).map_err(|err| err.render(sources))?;
            Ok(process_spec.into_data_specification())
        }
        SpecFormat::Pbes => {
            let text = std::fs::read_to_string(path)?;
            sources.add_text(path.display().to_string(), text.clone());
            let spec = UntypedPbes::parse(&text)?;
            let pbes_spec = PbesSpecification::from_untyped_with(spec, encoding).map_err(|err| err.render(sources))?;
            Ok(pbes_spec.into_data_specification())
        }
        SpecFormat::Pres => {
            let text = std::fs::read_to_string(path)?;
            sources.add_text(path.display().to_string(), text.clone());
            let spec = UntypedPres::parse(&text)?;
            let pres_spec = PresSpecification::from_untyped_with(spec, encoding).map_err(|err| err.render(sources))?;
            Ok(pres_spec.into_data_specification())
        }
        SpecFormat::Modal => {
            let text = std::fs::read_to_string(path)?;
            let (spec, _import_graph) = UntypedStateFrmSpec::parse_with_imports(path, &text, sources)?;
            // A modal formula's `val(...)` occurrences are plain-Boolean unless it's a
            // PRES-style quantitative formula; the rewriter only needs the data specification
            // underneath, so a fixed `FormulaType::Bool` is as good as either for that purpose.
            let modal_spec = ModalSpecification::from_untyped_with(spec, FormulaType::Bool, encoding, sources)
                .map_err(|err| err.render(sources))?;
            Ok(modal_spec.into_data_specification())
        }
    }
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

/// The `rewrite` command: rewrites the terms of a REC specification, or of any mCRL2-family
/// specification's contained data specification.
fn run_rewrite(args: RewriteArgs, timing: &Timing) -> Result<(), MercError> {
    let format = match args.format {
        Some(format) => format,
        None => SpecFormat::from_path(&args.specification)?,
    };

    match format {
        SpecFormat::Rec => {
            if args.terms.is_some() {
                warn!(
                    "The --terms option is currently ignored when rewriting REC specifications, the terms are taken from the REC spec."
                );
            }

            if !args.expression.is_empty() {
                warn!(
                    "The --expression option is only supported for mCRL2-family specifications, the terms are taken from the REC spec."
                );
            }

            let (syntax_spec, syntax_terms) = load_rec_from_file(&args.specification)?;

            let spec = syntax_spec.to_rewrite_spec();

            rewrite_rec(args.rewriter, &spec, &syntax_terms, args.output, timing)?;
        }
        SpecFormat::Mcrl2 | SpecFormat::DataSpec | SpecFormat::Pbes | SpecFormat::Pres | SpecFormat::Modal => {
            let mut sources = SourceMap::new();
            let mut data_spec =
                load_data_specification(format, &args.specification, NumberEncoding::default(), &mut sources)?;

            // Every term is type checked and lowered against the
            // same specification.
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

/// The `check` command: parses, resolves and type checks an mCRL2-family
/// specification printing the selected stages of the pipeline.
///
/// `--ast` prints the whole parsed specification (its contained data
/// specification together with whatever else the format itself declares:
/// processes, PBES/PRES equations, the modal formula). `--ir` and `--lowered`,
/// though, only ever show the specification's *contained data specification*.
fn run_check(args: CheckArgs) -> Result<(), MercError> {
    // With none of the stage flags given, show every stage.
    let show_all = !args.ast && !args.ir && !args.lowered;

    let format = match args.format {
        Some(format) => format,
        None => SpecFormat::from_path(&args.specification)?,
    };

    let mut sources = SourceMap::new();
    let encoding = NumberEncoding::default();

    let data_spec = match format {
        SpecFormat::Rec => {
            return Err(
                "The `check` command does not support the REC format; use `rewrite` or `convert` instead.".into(),
            );
        }
        SpecFormat::DataSpec => {
            let (untyped_spec, _import_graph) =
                UntypedDataSpecification::parse_with_imports(&args.specification, &mut sources)?;

            if show_all || args.ast {
                println!("=== AST ===\n");
                println!("{untyped_spec}");
            }

            typecheck_or_render(untyped_spec, encoding, &mut sources)?
        }
        SpecFormat::Mcrl2 => {
            let text = std::fs::read_to_string(&args.specification)?;
            let (untyped_spec, _import_graph) =
                UntypedProcessSpecification::parse_with_imports(&args.specification, &text, &mut sources)?;

            if show_all || args.ast {
                println!("=== AST ===\n");
                println!("{untyped_spec}");
            }

            ProcessSpecification::from_untyped_with(untyped_spec, encoding, &mut sources)
                .map_err(|err| err.render(&sources))?
                .into_data_specification()
        }
        SpecFormat::Pbes => {
            let text = std::fs::read_to_string(&args.specification)?;
            sources.add_text(args.specification.display().to_string(), text.clone());
            let untyped_spec = UntypedPbes::parse(&text)?;

            if show_all || args.ast {
                println!("=== AST ===\n");
                println!("{untyped_spec}");
            }

            PbesSpecification::from_untyped_with(untyped_spec, encoding)
                .map_err(|err| err.render(&sources))?
                .into_data_specification()
        }
        SpecFormat::Pres => {
            let text = std::fs::read_to_string(&args.specification)?;
            sources.add_text(args.specification.display().to_string(), text.clone());
            let untyped_spec = UntypedPres::parse(&text)?;

            if show_all || args.ast {
                println!("=== AST ===\n");
                println!("{untyped_spec}");
            }

            PresSpecification::from_untyped_with(untyped_spec, encoding)
                .map_err(|err| err.render(&sources))?
                .into_data_specification()
        }
        SpecFormat::Modal => {
            let text = std::fs::read_to_string(&args.specification)?;
            let (untyped_spec, _import_graph) =
                UntypedStateFrmSpec::parse_with_imports(&args.specification, &text, &mut sources)?;

            if show_all || args.ast {
                println!("=== AST ===\n");
                println!("{untyped_spec}");
            }

            ModalSpecification::from_untyped_with(untyped_spec, FormulaType::Bool, encoding, &mut sources)
                .map_err(|err| err.render(&sources))?
                .into_data_specification()
        }
    };

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

    eprintln!("The specification is well-typed.");

    Ok(())
}
