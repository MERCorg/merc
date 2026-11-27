use std::collections::HashMap;
use std::hash::Hash;

use log::debug;

use merc_data::BoolConnective;
use merc_data::BoolKind;
use merc_data::DataExpression;
use merc_data::DataExpressionKind;
use merc_data::DataVariable;
use merc_data::SortExpression;
use merc_data::is_bool_literal;
use merc_data::is_closed;
use merc_utilities::unique_chain_operands;

/// What a [ControlFlowGraph] needs to know about one summand.
pub struct CfgSummand<'a> {
    /// The summation variables, none of which may be a parameter.
    pub summation_variables: &'a [DataVariable],
    /// The guard of the summand.
    pub condition: &'a DataExpression,
    /// The parameters that the summand changes, as positions in the parameters, with their new
    /// value.
    pub writes: Vec<(usize, &'a DataExpression)>,
}

/// One edge of the control flow graph of a parameter, which a summand that constrains or changes
/// that parameter contributes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CfgEdge<V> {
    /// Position into [ControlFlowGraph::control_flow_parameters].
    pub position: usize,

    /// The location, see [ControlFlowGraph::location], that the parameter must be in for the
    /// summand to fire, or `None` when the guard does not constrain it.
    pub source: Option<usize>,

    /// The location that the summand moves the parameter to.
    pub target: CfgTarget<V>,
}

/// The location that a [CfgEdge] arrives at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CfgTarget<V> {
    /// The summand leaves the parameter unchanged.
    Unchanged,

    /// The summand assigns the parameter this closed value.
    Constant(V),

    /// The summand assigns the parameter a non-constant expression.
    Unknown,
}

/// The outcome of a control flow analysis, following the control flow graphs of
/// van de Pol and Timmer (*State Space Reduction of Linear Processes Using Control Flow
/// Reconstruction*, ATVA 2009).
///
/// A parameter `d` is a *control flow parameter* when every summand that changes it assigns a closed
/// value, at least one summand has a conjunct `d == c` with `c` closed in its guard, and `==`
/// decides the equality of every two of its *location terms*: its initial value, and the values of
/// the guards and assignments. Its values then behave like the locations of a program counter.
///
/// The locations of the paper are values, while location terms are normal forms, and different
/// normal forms can be equal when `==` is coarser than syntactic equality. The location terms are
/// therefore grouped into *locations*, the classes of terms that `==` makes equal, and the edges are
/// between locations. A parameter for which `==` does not rewrite to `true` or `false` for some pair
/// of location terms is not a control flow parameter, since its locations are unknown.
pub struct ControlFlowGraph<V> {
    control_flow_parameters: Vec<usize>,

    /// For every control flow parameter, the location of each of its location terms.
    locations: Vec<HashMap<V, usize>>,

    edges: Vec<Vec<CfgEdge<V>>>,
}

impl<V: Hash + Eq> ControlFlowGraph<V> {
    /// Analyses `summands`, whose guards and assignments are expressed over `parameters`. No
    /// summation variable may shadow a parameter, see
    /// [resolve_name_clashes](crate::transformations::resolve_name_clashes).
    ///
    /// `rewrite` brings a term in normal form before it is tested for closedness and decides the
    /// equality of location terms, and `intern` turns a location term in normal form into the
    /// representation `V` in which the state values are compared with it.
    pub fn new<F, G>(
        parameters: &[DataVariable],
        initial_state: &[DataExpression],
        summands: &[CfgSummand<'_>],
        mut rewrite: F,
        mut intern: G,
    ) -> Self
    where
        F: FnMut(&DataExpression) -> DataExpression,
        G: FnMut(&DataExpression) -> V,
    {
        debug_assert!(
            summands
                .iter()
                .flat_map(|summand| summand.summation_variables)
                .all(|variable| !parameters.contains(variable)),
            "A summation variable shadows a process parameter"
        );

        let analyses: Vec<SummandAnalysis> = summands
            .iter()
            .map(|summand| analyse_summand(summand, parameters, &mut rewrite))
            .collect();

        // Of every candidate, the location of each location term: its initial value, and every
        // value that a guard compares it with or that a summand assigns to it.
        let mut control_flow_parameters = Vec::new();
        let mut location_terms: Vec<HashMap<DataExpression, usize>> = Vec::new();
        for parameter in (0..parameters.len()).filter(|&parameter| is_control_flow_parameter(parameter, &analyses)) {
            let mut terms = vec![rewrite(&initial_state[parameter])];
            for analysis in &analyses {
                terms.extend(analysis.source.get(&parameter).cloned());
                terms.extend(analysis.target.get(&parameter).cloned().flatten());
            }
            terms.sort();
            terms.dedup();

            let sort = parameters[parameter].sort().protect();
            match locations(&terms, &sort, &mut rewrite) {
                Some(locations) => {
                    control_flow_parameters.push(parameter);
                    location_terms.push(terms.into_iter().zip(locations).collect());
                }
                None => debug!(
                    "Parameter {} is not a control flow parameter, since == does not decide the equality of its locations",
                    parameters[parameter].name()
                ),
            }
        }

        let mut edges = Vec::with_capacity(analyses.len());
        for analysis in &analyses {
            let mut summand_edges = Vec::new();
            for (position, parameter) in control_flow_parameters.iter().enumerate() {
                // Every source is a location term of the parameter.
                let source = analysis
                    .source
                    .get(parameter)
                    .map(|source| location_terms[position][source]);
                let target = match analysis.target.get(parameter) {
                    None => CfgTarget::Unchanged,
                    Some(None) => CfgTarget::Unknown,
                    Some(Some(value)) => CfgTarget::Constant(intern(value)),
                };

                if source.is_some() || matches!(target, CfgTarget::Constant(_)) {
                    summand_edges.push(CfgEdge {
                        position,
                        source,
                        target,
                    });
                }
            }
            edges.push(summand_edges);
        }

        let locations = location_terms
            .into_iter()
            .map(|terms| {
                terms
                    .into_iter()
                    .map(|(term, location)| (intern(&term), location))
                    .collect()
            })
            .collect();

        Self {
            control_flow_parameters,
            locations,
            edges,
        }
    }

    /// The positions of the control flow parameters among the parameters.
    pub fn control_flow_parameters(&self) -> &[usize] {
        &self.control_flow_parameters
    }

    /// The location of the value `value` of the control flow parameter at `position` into
    /// [Self::control_flow_parameters], or `None` when it is not one of its location terms. Every
    /// reachable value of a control flow parameter is a location term, provided that it is in the
    /// same normal form.
    pub fn location(&self, position: usize, value: &V) -> Option<usize> {
        self.locations[position].get(value).copied()
    }

    /// The edges that the summand with the given index contributes.
    pub fn edges(&self, index: usize) -> &[CfgEdge<V>] {
        &self.edges[index]
    }
}

/// The locations of one summand, keyed by the position of the parameter.
struct SummandAnalysis {
    /// The closed values that the guard requires.
    source: HashMap<usize, DataExpression>,

    /// For every changed parameter, its new value when that is closed.
    target: HashMap<usize, Option<DataExpression>>,
}

fn analyse_summand<F>(summand: &CfgSummand<'_>, parameters: &[DataVariable], rewrite: &mut F) -> SummandAnalysis
where
    F: FnMut(&DataExpression) -> DataExpression,
{
    let mut source = HashMap::new();
    for conjunct in unique_chain_operands(summand.condition.copy(), BoolConnective::And) {
        if let Some((index, value)) = as_parameter_equality(&conjunct.protect(), parameters, rewrite) {
            source.entry(index).or_insert(value);
        }
    }

    let target = summand
        .writes
        .iter()
        .map(|(index, value)| (*index, closed_value(value, rewrite)))
        .collect();

    SummandAnalysis { source, target }
}

/// The location of every term of `terms`, numbered by the first term with that location, when `==`
/// on `sort` decides the equality of every pair of terms consistently, and `None` otherwise.
fn locations<F>(terms: &[DataExpression], sort: &SortExpression, rewrite: &mut F) -> Option<Vec<usize>>
where
    F: FnMut(&DataExpression) -> DataExpression,
{
    let mut locations: Vec<usize> = Vec::with_capacity(terms.len());
    for (index, term) in terms.iter().enumerate() {
        let mut equal = Vec::with_capacity(index);
        for other in &terms[..index] {
            let result = rewrite(&DataExpression::equal(sort, term, other));
            if is_bool_literal(&result, true) {
                equal.push(true);
            } else if is_bool_literal(&result, false) {
                equal.push(false);
            } else {
                return None;
            }
        }

        // A term belongs to the location of the first earlier term that it is equal to, and must then
        // be equal to exactly the terms of that location.
        let location = equal
            .iter()
            .position(|&equal| equal)
            .map_or(index, |other| locations[other]);
        if equal
            .iter()
            .zip(&locations)
            .any(|(&equal, &other)| equal != (other == location))
        {
            return None;
        }
        locations.push(location);
    }
    Some(locations)
}

fn is_control_flow_parameter(parameter: usize, analyses: &[SummandAnalysis]) -> bool {
    let mut constrained = false;
    for analysis in analyses {
        constrained |= analysis.source.contains_key(&parameter);
        if matches!(analysis.target.get(&parameter), Some(None)) {
            return false;
        }
    }
    constrained
}

/// When `expression` is `d == c` or `c == d` for a parameter `d` and a closed `c`, returns the position of `d` and the normal form of `c`.
fn as_parameter_equality<F>(
    expression: &DataExpression,
    parameters: &[DataVariable],
    rewrite: &mut F,
) -> Option<(usize, DataExpression)>
where
    F: FnMut(&DataExpression) -> DataExpression,
{
    let BoolKind::Equal(left, right) = expression.copy().bool_kind() else {
        return None;
    };
    let (left, right) = (left.protect(), right.protect());

    match_parameter_constant(&left, &right, parameters, rewrite)
        .or_else(|| match_parameter_constant(&right, &left, parameters, rewrite))
}

fn match_parameter_constant<F>(
    variable: &DataExpression,
    constant: &DataExpression,
    parameters: &[DataVariable],
    rewrite: &mut F,
) -> Option<(usize, DataExpression)>
where
    F: FnMut(&DataExpression) -> DataExpression,
{
    let DataExpressionKind::Variable(variable) = variable.kind() else {
        return None;
    };
    let index = parameters.iter().position(|parameter| parameter.copy() == variable)?;
    closed_value(constant, rewrite).map(|value| (index, value))
}

/// The normal form of `term` when that is closed.
fn closed_value<F>(term: &DataExpression, rewrite: &mut F) -> Option<DataExpression>
where
    F: FnMut(&DataExpression) -> DataExpression,
{
    let normal_form = rewrite(term);
    is_closed(&normal_form).then_some(normal_form)
}
