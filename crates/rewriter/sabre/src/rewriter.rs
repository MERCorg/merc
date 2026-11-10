use crate::InnermostRewriter;
use merc_data::DataExpression;
// TODO: need a merc_rewrite? crate to link both the sabre rewriter and other rewriters.
// use crate::InnermostCompilingRewriter;
use crate::NaiveRewriter;
use crate::RewriteEngine;
use crate::RewriteSpecification;
use crate::SabreRewriter;

/// The rewrite engines that can drive the exploration of an LPS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum Rewriter {
    Naive,
    /// An innermost rewriter that uses set automata for efficient matching.
    #[default]
    Innermost,
    // /// A variant of innermost that generates Rust code for the rewrite rules and compiles it to a dynamic library.
    // InnermostCompiling,
    /// A set automaton-based rewriter that applies rules outermost.
    Sabre,
}

/// A rewriter of the selected [RewriterKind].
pub enum DataRewriter {
    Naive(NaiveRewriter),
    Innermost(InnermostRewriter),
    // InnermostCompiling(InnermostCompilingRewriter),
    Sabre(SabreRewriter),
}

impl DataRewriter {
    /// Constructs the rewriter of the given `kind` for `specification`.
    pub fn new(kind: Rewriter, specification: &RewriteSpecification) -> Self {
        match kind {
            Rewriter::Naive => DataRewriter::Naive(NaiveRewriter::new(specification)),
            Rewriter::Innermost => DataRewriter::Innermost(InnermostRewriter::new(specification)),
            // Rewriter::InnermostCompiling => DataRewriter::InnermostCompiling(InnermostCompilingRewriter::new(specification)),
            Rewriter::Sabre => DataRewriter::Sabre(SabreRewriter::new(specification)),
        }
    }
}

impl RewriteEngine for DataRewriter {
    fn rewrite(&mut self, term: &DataExpression) -> DataExpression {
        match self {
            DataRewriter::Naive(rewriter) => rewriter.rewrite(term),
            DataRewriter::Innermost(rewriter) => rewriter.rewrite(term),
            // DataRewriter::InnermostCompiling(rewriter) => rewriter.rewrite(term),
            DataRewriter::Sabre(rewriter) => rewriter.rewrite(term),
        }
    }
}
