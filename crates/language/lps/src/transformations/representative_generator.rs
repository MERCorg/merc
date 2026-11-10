use std::collections::HashMap;
use std::collections::HashSet;

use merc_aterm::ATerm;
use merc_aterm::Term;
use merc_data::DataApplication;
use merc_data::DataExpression;
use merc_data::DataFunctionSymbol;
use merc_data::Mcrl2DataSpecification;
use merc_data::SortArrow;
use merc_data::SortExpression;
use merc_data::is_function_sort;

/// Generates an arbitrary element of a sort, following the representative generator of mCRL2.
struct RepresentativeGenerator<'a> {
    specification: &'a Mcrl2DataSpecification,

    /// Cache of previously computed representatives for each sort.
    cache: HashMap<SortExpression, DataExpression>,
}

impl<'a> RepresentativeGenerator<'a> {
    fn new(specification: &'a Mcrl2DataSpecification) -> Self {
        RepresentativeGenerator {
            specification,
            cache: HashMap::new(),
        }
    }

    fn representative(&mut self, sort: &SortExpression) -> Option<DataExpression> {
        self.find(sort, &mut HashSet::new())
    }

    fn find(&mut self, sort: &SortExpression, visited: &mut HashSet<SortExpression>) -> Option<DataExpression> {
        if visited.contains(sort) {
            return None;
        }

        if let Some(result) = self.cache.get(sort) {
            return Some(result.clone());
        }

        let specification = self.specification;
        let result = if is_function_sort(sort) {
            // The lambda term that mCRL2 falls back on is a binder, which is not supported yet.
            minimal_symbol(specification.mappings(), sort).map(DataExpression::from)
        } else if let Some(constant) = minimal_symbol(specification.constructors(), sort) {
            Some(DataExpression::from(constant))
        } else {
            visited.insert(sort.clone());
            let result = self
                .find_application(specification.constructors(), sort, visited)
                .or_else(|| minimal_symbol(specification.mappings(), sort).map(DataExpression::from))
                .or_else(|| self.find_application(specification.mappings(), sort, visited));
            visited.remove(sort);
            result
        };

        if let Some(result) = &result {
            self.cache.insert(sort.clone(), result.clone());
        }
        result
    }

    /// Finds a term `f(t₁, …, tₙ)` of the given `sort`, for one of the given function `symbols`.
    fn find_application(
        &mut self,
        symbols: &[DataFunctionSymbol],
        sort: &SortExpression,
        visited: &mut HashSet<SortExpression>,
    ) -> Option<DataExpression> {
        'symbols: for symbol in symbols {
            let symbol_sort = symbol.sort().protect();
            if !is_function_sort(&symbol_sort) {
                continue;
            }

            let arrow = SortArrow::from(ATerm::from(symbol_sort));
            if arrow.codomain().protect() != *sort {
                continue;
            }

            let mut arguments = Vec::new();
            for domain in arrow.domain().iter() {
                match self.find(&SortExpression::from(domain.protect()), visited) {
                    Some(argument) => arguments.push(argument),
                    None => continue 'symbols,
                }
            }
            return Some(DataApplication::with_args(symbol, &arguments).into());
        }
        None
    }
}

/// The function symbol of exactly `sort` with the lexicographically smallest name, if any.
fn minimal_symbol(symbols: &[DataFunctionSymbol], sort: &SortExpression) -> Option<DataFunctionSymbol> {
    symbols
        .iter()
        .filter(|symbol| symbol.sort() == sort.copy())
        .min_by(|left, right| left.name().value().cmp(right.name().value()))
        .cloned()
}
