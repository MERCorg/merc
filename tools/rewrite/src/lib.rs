use std::fmt::Debug;

use clap::ValueEnum;

use log::info;
use merc_aterm::ATerm;
use merc_data::DataExpression;
use merc_data::to_untyped_data_expression;
use merc_sabre::InnermostRewriter;
use merc_sabre::NaiveRewriter;
use merc_sabre::RewriteEngine;
use merc_sabre::RewriteMetrics;
use merc_sabre::RewriteSpecification;
use merc_sabre::SabreRewriter;
use merc_sabre_compiling::SabreCompilingRewriter;
use merc_utilities::MercError;
use merc_utilities::Timing;

/// Selects the rewriter to use.
#[derive(ValueEnum, Debug, Clone)]
pub enum Rewriter {
    Naive,
    /// An innermost rewriter that uses set automata for efficient matching.
    Innermost,
    /// A variant of innermost that generates Rust code for the rewrite rules and compiles it to a dynamic library.
    InnermostCompiling,
    /// A set automaton-based rewriter that applies rules outermost.
    Sabre,
}

/// Rewrites the given REC specification.
///
/// The terms of a REC specification are untyped aterms, so each is first
/// converted into the untyped [DataExpression] form the rewriters expect; an
/// mCRL2 specification instead supplies already type-checked and lowered terms
/// to [rewrite_terms] directly.
pub fn rewrite_rec(
    rewriter: Rewriter,
    spec: &RewriteSpecification,
    syntax_terms: &[ATerm],
    output: bool,
    timing: &Timing,
) -> Result<(), MercError> {
    let terms: Vec<DataExpression> = syntax_terms
        .iter()
        .map(|term| to_untyped_data_expression(term.clone(), None))
        .collect();

    rewrite_terms(rewriter, spec, &terms, output, timing)
}

/// Rewrites every term to normal form with the selected rewriter, printing the
/// results when `output` is set.
///
/// The rewriter is constructed once for the whole batch: building the set
/// automaton of a full mCRL2 specification dominates the cost of rewriting a
/// handful of terms.
pub fn rewrite_terms(
    rewriter: Rewriter,
    spec: &RewriteSpecification,
    terms: &[DataExpression],
    output: bool,
    timing: &Timing,
) -> Result<(), MercError> {
    /// Rewrites every term with `engine`, printing each result when asked, and logs the metrics
    /// of each rewrite for an engine that has `metrics`.
    fn rewrite_all<E, M>(engine: &mut E, terms: &[DataExpression], output: bool, timing: &Timing, metrics: M)
    where
        E: RewriteEngine,
        M: Fn(&E) -> Option<&RewriteMetrics>,
    {
        timing.measure("rewrite_rec", || {
            for term in terms {
                let result = engine.rewrite(term);
                if output {
                    println!("{}", result)
                }
                if cfg!(feature = "metrics")
                    && let Some(metrics) = metrics(engine)
                {
                    info!("metrics: {metrics:#}");
                }
            }
        });
    }

    match rewriter {
        Rewriter::Naive => {
            let mut inner = timing.measure("rewriter_construction", || NaiveRewriter::new(spec));
            rewrite_all(&mut inner, terms, output, timing, |_| None);
        }
        Rewriter::Innermost => {
            let mut inner = timing.measure("rewriter_construction", || InnermostRewriter::new(spec));
            rewrite_all(&mut inner, terms, output, timing, |engine| Some(engine.metrics()));
        }
        Rewriter::InnermostCompiling => {
            let mut inner = timing.measure("rewriter_construction", || {
                SabreCompilingRewriter::new(spec, true, false)
            })?;
            rewrite_all(&mut inner, terms, output, timing, |_| None);
        }
        Rewriter::Sabre => {
            let mut sa = timing.measure("rewriter_construction", || SabreRewriter::new(spec));
            rewrite_all(&mut sa, terms, output, timing, |engine| Some(engine.metrics()));
        }
    }

    Ok(())
}
