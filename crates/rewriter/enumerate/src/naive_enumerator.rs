#![forbid(unsafe_code)]

use ahash::AHashSet;
use ahash::HashMap;
use merc_data::DataApplication;
use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_data::bool_literal;
use merc_sabre::RewriteEngine;

use crate::enumeration_plan::EnumerationPlanId;
use crate::enumeration_plan::EnumerationPlans;
use crate::enumeration_plan::SortEnumerability;
use crate::enumerator::cartesian_product;

/// A brute-force enumerator that serves as a reference in tests.
///
/// Generates every ground term of each variable's sort up to a size bound,
/// substitutes all variables at once and rewrites the body for every
/// combination. Solutions that need larger terms are never found.
pub struct NaiveEnumerator<'a, R> {
    rewriter: &'a mut R,
    plans: &'a EnumerationPlans,
    max_size: u32,
}

impl<'a, R: RewriteEngine> NaiveEnumerator<'a, R> {
    /// Considers ground terms with at most `max_size` nested constructor
    /// applications per variable.
    pub fn new(rewriter: &'a mut R, plans: &'a EnumerationPlans, max_size: u32) -> Self {
        NaiveEnumerator {
            rewriter,
            plans,
            max_size,
        }
    }

    /// Returns every substitution of `vars` for which `body` rewrites to `true`.
    pub fn enumerate_all(&mut self, vars: &[DataVariable], body: &DataExpression) -> AHashSet<Vec<DataExpression>> {
        let per_variable: Vec<Vec<DataExpression>> = vars
            .iter()
            .map(|v| match self.plans.get(&v.sort()) {
                Some(sort_id) => self.terms_up_to_size(sort_id, self.max_size),
                None => Vec::new(),
            })
            .collect();

        let true_literal = bool_literal(true);
        let mut results = AHashSet::new();
        for combination in cartesian_product(&per_variable) {
            let sigma: HashMap<DataVariable, DataExpression> =
                vars.iter().cloned().zip(combination.iter().cloned()).collect();

            if self.rewriter.rewrite_with(body, &sigma) == true_literal {
                results.insert(combination);
            }
        }
        results
    }

    /// Every ground term of `sort_id` with at most `size` nested constructor
    /// applications.
    fn terms_up_to_size(&mut self, sort_id: EnumerationPlanId, size: u32) -> Vec<DataExpression> {
        let plans = self.plans;
        let plan = plans.plan(sort_id);
        if size == 0
            || !matches!(
                plan.enumerability(),
                SortEnumerability::Finite | SortEnumerability::InfiniteEnumerable
            )
        {
            return Vec::new();
        }

        let mut result = Vec::new();
        for constructor in plan.constructors() {
            // Every argument gets the full remaining budget, which over-approximates
            // the size but keeps this simple.
            let per_argument: Vec<Vec<DataExpression>> = constructor
                .arguments()
                .iter()
                .map(|&arg_sort| self.terms_up_to_size(arg_sort, size - 1))
                .collect();

            for combination in cartesian_product(&per_argument) {
                // `rewrite_with` requires normal-form images, and constructors
                // such as `@c0` are not automatically one.
                let value: DataExpression = if combination.is_empty() {
                    constructor.symbol().clone().into()
                } else {
                    DataApplication::with_args(constructor.symbol(), &combination).into()
                };
                result.push(self.rewriter.rewrite(&value));
            }
        }
        result
    }
}
