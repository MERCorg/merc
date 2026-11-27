use std::collections::HashSet;

use merc_aterm::ATerm;
use merc_aterm::Term;
use merc_data::DataEquation;
use merc_data::DataFunctionSymbol;
use merc_data::Mcrl2DataSpecification;
use merc_data::SortExpression;
use merc_data::is_sort_expression;
use merc_data::is_structured_sort;
use merc_lowering::lower_data_specification;
use merc_lowering::number_encoding;
use merc_lowering::raise_sort;
use merc_syntax::IdDecl;
use merc_syntax::SortDecl;
use merc_syntax::SortId;
use merc_syntax::SourceMap;
use merc_syntax::UntypedDataSpecification;
use merc_typecheck::DataSpecification;
use merc_utilities::MercError;
use merc_utilities::Span;

use crate::LinearProcessSpecification;

/// The prefix of the placeholder mappings that make the context sorts known to the type checker.
/// These never go through the parser, and the space guarantees that they cannot clash with a name
/// from the file.
const CONTEXT_MAPPING_PREFIX: &str = "lps context ";

/// Returns the data specification of `lps` extended with the system defined sorts, constructors,
/// mappings and equations, from which a rewriter can be built.
///
/// A `.lps` file stores only the user defined part of the data specification, and mCRL2 generates
/// the rest when it loads one. Here that is done by handing the declarations to the type checker,
/// which instantiates the templates of every sort that it is told about, and appending the
/// equations of the file as they are, so that these are not type checked a second time.
///
/// Expects the sorts of `lps` to be normalised, see
/// [normalise_sorts](crate::transformations::normalise_sorts).
pub fn complete_data_specification(lps: &LinearProcessSpecification) -> Result<Mcrl2DataSpecification, MercError> {
    let data = &lps.data_specification;

    let mut untyped = UntypedDataSpecification::default();
    for sort in data.sorts() {
        untyped
            .sort_declarations
            .push(SortDecl::new(sort.name().to_string(), None, Span::default()));
    }
    // An alias of any other sort has been expanded, so it occurs nowhere anymore.
    for alias in data.aliases() {
        let definition = alias.reference().protect();
        if is_structured_sort(&definition) {
            untyped.sort_declarations.push(SortDecl::new(
                alias.name().name().to_string(),
                Some(raise_sort(&definition)),
                Span::default(),
            ));
        }
    }

    let declare = |symbol: &DataFunctionSymbol| {
        IdDecl::<SortId>::new(
            symbol.name().to_string(),
            raise_sort(&symbol.sort().protect()),
            Span::default(),
        )
    };
    untyped.constructor_declarations = data
        .constructors()
        .iter()
        .map(|symbol| declare(symbol).retag())
        .collect();
    untyped.map_declarations = data.mappings().iter().map(|symbol| declare(symbol).retag()).collect();

    let context_sorts = collect_context_sorts(terms(lps));
    let encoding = number_encoding(terms(lps));
    untyped
        .map_declarations
        .extend(context_sorts.iter().enumerate().map(|(index, sort)| {
            IdDecl::<SortId>::new(
                format!("{CONTEXT_MAPPING_PREFIX}{index}"),
                raise_sort(sort),
                Span::default(),
            )
            .retag()
        }));

    let typed = DataSpecification::from_untyped_with(untyped, encoding, &mut SourceMap::new())
        .map_err(|error| format!("The data specification of the LPS is not well typed: {error}"))?;
    let lowered = lower_data_specification(&typed);

    let mut equations: Vec<DataEquation> = data.equations().to_vec();
    equations.extend_from_slice(lowered.equations());
    Ok(Mcrl2DataSpecification::new(
        lowered.sorts().to_vec(),
        lowered.aliases().to_vec(),
        lowered
            .constructors()
            .iter()
            .filter(|symbol| !symbol.name().value().starts_with(CONTEXT_MAPPING_PREFIX))
            .cloned()
            .collect(),
        lowered
            .mappings()
            .iter()
            .filter(|symbol| !symbol.name().value().starts_with(CONTEXT_MAPPING_PREFIX))
            .cloned()
            .collect(),
        equations,
    ))
}

/// Every top level term of `lps`, in no particular order.
fn terms(lps: &LinearProcessSpecification) -> impl Iterator<Item = ATerm> + '_ {
    let data = &lps.data_specification;
    let action_summands = lps.action_summands.iter().flat_map(|summand| {
        summand
            .summation_variables
            .iter()
            .cloned()
            .map(ATerm::from)
            .chain(std::iter::once(summand.condition.clone().into()))
            .chain(summand.action.time.iter().cloned().map(ATerm::from))
            .chain(summand.next_state.iter().flatten().cloned().map(ATerm::from))
            .chain(
                summand
                    .action
                    .actions
                    .iter()
                    .flat_map(|action| action.arguments.iter().cloned().map(ATerm::from)),
            )
    });
    let deadlock_summands = lps.deadlock_summands.iter().flat_map(|summand| {
        summand
            .summation_variables
            .iter()
            .cloned()
            .map(ATerm::from)
            .chain(std::iter::once(summand.condition.clone().into()))
            .chain(summand.time.iter().cloned().map(ATerm::from))
    });

    data.constructors()
        .iter()
        .cloned()
        .map(ATerm::from)
        .chain(data.mappings().iter().cloned().map(ATerm::from))
        .chain(data.equations().iter().cloned().map(ATerm::from))
        .chain(
            lps.action_labels
                .iter()
                .flat_map(|label| label.sorts.iter().cloned().map(ATerm::from)),
        )
        .chain(lps.global_variables.iter().cloned().map(ATerm::from))
        .chain(lps.process_parameters.iter().cloned().map(ATerm::from))
        .chain(lps.initial_state.iter().cloned().map(ATerm::from))
        .chain(action_summands)
        .chain(deadlock_summands)
}

/// Collects the sorts that occur in `terms`. For an LPS these are mCRL2's "context sorts": those
/// for which the system defined operations must exist even when no declaration mentions them.
fn collect_context_sorts<I: IntoIterator<Item = ATerm>>(terms: I) -> Vec<SortExpression> {
    let mut visited: HashSet<ATerm> = HashSet::new();
    let mut sorts: Vec<SortExpression> = Vec::new();
    let mut stack: Vec<ATerm> = terms.into_iter().collect();

    while let Some(term) = stack.pop() {
        if !visited.insert(term.clone()) {
            continue;
        }

        if is_sort_expression(&term) {
            sorts.push(SortExpression::from(term));
        } else {
            stack.extend(term.arguments().map(|argument| argument.protect()));
        }
    }

    sorts
}
