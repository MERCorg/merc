use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_data::Mcrl2DataSpecification;
use merc_sabre::DataRewriter;
use merc_sabre::RewriteSpecification;
use merc_sabre::Rewriter;
use merc_utilities::MercError;

use crate::LinearProcessSpecification;
use crate::transformations::OnePointEliminations;
use crate::transformations::complete_data_specification;
use crate::transformations::instantiate_global_variables;
use crate::transformations::normalise_sorts;
use crate::transformations::one_point_rule_rewrite;
use crate::transformations::order_summand_variables;
use crate::transformations::replace_constants_by_variables;
use crate::transformations::resolve_name_clashes;

/// A transformation that [TransformedLps::transform] can apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LpsTransformation {
    /// Replace the global variables by arbitrary values of their sorts.
    InstantiateGlobalVariables,

    /// Order the summation variables of every summand to make enumeration more efficient.
    OrderSummandVariables,

    /// Rename summation variables that have the name of a process parameter.
    ResolveNameClashes,

    /// Eliminates summation variables whose value the condition determines.
    OnePointRuleRewrite,

    /// Replaces closed subterms by variables, so that these are only rewritten once.
    ReplaceConstantsByVariables,
}

/// The transformations that the exploration tools of the mCRL2 toolset apply before exploring an
/// LPS, in the order in which they are applied.
pub const DEFAULT_PREPROCESSING: &[LpsTransformation] = &[
    LpsTransformation::InstantiateGlobalVariables,
    LpsTransformation::OrderSummandVariables,
    LpsTransformation::ResolveNameClashes,
    LpsTransformation::OnePointRuleRewrite,
    LpsTransformation::ReplaceConstantsByVariables,
];

/// A transformation that has been applied, with the information it produced.
#[derive(Clone)]
pub enum AppliedTransformation {
    InstantiateGlobalVariables {
        /// The value that was chosen for every global variable.
        assignments: Vec<(DataVariable, DataExpression)>,
    },
    OrderSummandVariables,
    ResolveNameClashes {
        /// The `(new, old)` variable pairs, for every action summand.
        renamings: Vec<Vec<(DataVariable, DataVariable)>>,
    },
    OnePointRuleRewrite(OnePointEliminations),
    ReplaceConstantsByVariables {
        /// The normal forms of the variables that were introduced.
        assignments: Vec<(DataVariable, DataExpression)>,
    },
}

impl AppliedTransformation {
    /// The transformation that was applied.
    pub fn kind(&self) -> LpsTransformation {
        match self {
            AppliedTransformation::InstantiateGlobalVariables { .. } => LpsTransformation::InstantiateGlobalVariables,
            AppliedTransformation::OrderSummandVariables => LpsTransformation::OrderSummandVariables,
            AppliedTransformation::ResolveNameClashes { .. } => LpsTransformation::ResolveNameClashes,
            AppliedTransformation::OnePointRuleRewrite(_) => LpsTransformation::OnePointRuleRewrite,
            AppliedTransformation::ReplaceConstantsByVariables { .. } => LpsTransformation::ReplaceConstantsByVariables,
        }
    }
}

/// A linear process together with everything needed to rewrite its data expressions, and a record
/// of the transformations that were applied to it.
///
/// Its sorts are normalised, see [normalise_sorts], and the data specification from which a
/// rewriter can be built is complete, see [complete_data_specification].
#[derive(Clone)]
pub struct TransformedLps {
    lps: LinearProcessSpecification,
    data_specification: Mcrl2DataSpecification,
    rewriter: Rewriter,

    /// The transformations that have been applied to this LPS, in order.
    transformations: Vec<AppliedTransformation>,
}

/// Applies the given `transformations` in order, see [DEFAULT_PREPROCESSING] for the usual ones.
///
/// # Errors
///
/// Fails when one of the `transformations` fails, see [TransformedLps::transform].
pub fn preprocess(lps: TransformedLps, transformations: &[LpsTransformation]) -> Result<TransformedLps, MercError> {
    transformations
        .iter()
        .try_fold(lps, |lps, &transformation| lps.transform(transformation))
}

impl TransformedLps {
    /// Normalises the sorts of `lps` and completes its data specification.
    pub fn new(lps: &LinearProcessSpecification) -> Result<Self, MercError> {
        let lps = normalise_sorts(lps);
        let data_specification = complete_data_specification(&lps)?;
        Ok(TransformedLps {
            lps,
            data_specification,
            rewriter: Rewriter::default(),
            transformations: Vec::new(),
        })
    }

    /// Selects the rewrite engine that is used to explore this LPS.
    pub fn with_rewriter(mut self, rewriter: Rewriter) -> Self {
        self.rewriter = rewriter;
        self
    }

    /// Applies the given transformation and records it.
    ///
    /// A transformation does not check which transformations were applied before, only (in debug
    /// builds) that the LPS has the shape it needs, see for example
    /// [one_point_rule_rewrite](crate::transformations::one_point_rule_rewrite).
    ///
    /// # Errors
    ///
    /// Fails when a global variable has a sort without a representative.
    pub fn transform(self, kind: LpsTransformation) -> Result<Self, MercError> {
        let TransformedLps {
            lps,
            data_specification,
            rewriter,
            mut transformations,
        } = self;

        let (lps, applied) = match kind {
            LpsTransformation::InstantiateGlobalVariables => {
                let (lps, assignments) = instantiate_global_variables(lps, &data_specification)?;
                (lps, AppliedTransformation::InstantiateGlobalVariables { assignments })
            }
            LpsTransformation::OrderSummandVariables => (
                order_summand_variables(lps, &data_specification),
                AppliedTransformation::OrderSummandVariables,
            ),
            LpsTransformation::ResolveNameClashes => {
                let (lps, renamings) = resolve_name_clashes(lps);
                (lps, AppliedTransformation::ResolveNameClashes { renamings })
            }
            LpsTransformation::OnePointRuleRewrite => {
                let specification = RewriteSpecification::from_data_specification(&data_specification);
                let (lps, eliminations) = one_point_rule_rewrite(lps, &mut DataRewriter::new(rewriter, &specification));
                (lps, AppliedTransformation::OnePointRuleRewrite(eliminations))
            }
            LpsTransformation::ReplaceConstantsByVariables => {
                let specification = RewriteSpecification::from_data_specification(&data_specification);
                let (lps, assignments) =
                    replace_constants_by_variables(lps, &mut DataRewriter::new(rewriter, &specification));
                (lps, AppliedTransformation::ReplaceConstantsByVariables { assignments })
            }
        };

        transformations.push(applied);
        Ok(TransformedLps {
            lps,
            data_specification,
            rewriter,
            transformations,
        })
    }

    /// The rewrite engine that is used to explore this LPS.
    pub fn rewriter_kind(&self) -> Rewriter {
        self.rewriter
    }

    /// The linear process.
    ///
    /// After [LpsTransformation::ReplaceConstantsByVariables] its summands contain free variables
    /// that are only meaningful together with [Self::constant_assignments].
    pub fn lps(&self) -> &LinearProcessSpecification {
        &self.lps
    }

    /// The data specification including all system defined equations.
    pub fn data_specification(&self) -> &Mcrl2DataSpecification {
        &self.data_specification
    }

    /// The transformations that have been applied to this LPS, in order.
    pub fn transformations(&self) -> &[AppliedTransformation] {
        &self.transformations
    }

    /// The normal forms of the variables introduced by every application of
    /// [LpsTransformation::ReplaceConstantsByVariables].
    pub fn constant_assignments(&self) -> impl Iterator<Item = &(DataVariable, DataExpression)> {
        self.transformations.iter().flat_map(|applied| match applied {
            AppliedTransformation::ReplaceConstantsByVariables { assignments } => assignments.as_slice(),
            _ => &[],
        })
    }
}
