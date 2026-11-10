use std::collections::HashMap;

use merc_aterm::Term;
use merc_data::DataExpression;
use merc_data::DataExpressionKind;
use merc_data::DataVariable;
use merc_data::is_closed;

/// What a [ControlFlowGraph] needs to know about one summand.
pub struct CfgSummand<'a> {
    /// The guard of the summand.
    pub condition: &'a DataExpression,
    /// The parameters that the summand changes, as positions in the parameters, with their new
    /// value.
    pub writes: Vec<(usize, &'a DataExpression)>,
}

/// One edge of the control flow graph of a parameter, which a summand that constrains or changes
/// that parameter contributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CfgEdge<V> {
    /// Position into [ControlFlowGraph::control_flow_parameters].
    pub position: usize,

    /// The value that the parameter must have for the summand to fire, or `None` when the guard
    /// does not constrain it.
    pub source: Option<V>,

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

/// The outcome of a control flow analysis. A parameter `d` is a *control flow parameter* when every
/// summand that changes it assigns a closed value, and at least one summand has a conjunct `d == c`
/// with `c` closed in its guard. Its values then behave like the locations of a program counter.
pub struct ControlFlowGraph<V> {
    control_flow_parameters: Vec<usize>,
    edges: Vec<Vec<CfgEdge<V>>>,
}

impl<V> ControlFlowGraph<V> {
    /// Analyses `summands`, whose guards and assignments are expressed over `parameters`.
    ///
    /// `rewrite` brings a term in normal form before it is tested for closedness, and `intern`
    /// turns a location in normal form into the representation `V` in which it is compared.
    pub fn new<F, G>(
        parameters: &[DataVariable],
        summands: &[CfgSummand<'_>],
        mut rewrite: F,
        mut intern: G,
    ) -> Self 
    where F: FnMut(&DataExpression) -> DataExpression,
          G: FnMut(&DataExpression) -> V,    
    {
        let analyses: Vec<SummandAnalysis> = summands
            .iter()
            .map(|summand| analyse_summand(summand, parameters, &mut rewrite))
            .collect();

        let control_flow_parameters: Vec<usize> = (0..parameters.len())
            .filter(|&parameter| is_control_flow_parameter(parameter, &analyses))
            .collect();

        let mut edges = Vec::with_capacity(analyses.len());
        for analysis in &analyses {
            let mut summand_edges = Vec::new();
            for (position, parameter) in control_flow_parameters.iter().enumerate() {
                let source = analysis.source.get(parameter).map(&mut intern);
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

        Self {
            control_flow_parameters,
            edges,
        }
    }

    /// The positions of the control flow parameters among the parameters.
    pub fn control_flow_parameters(&self) -> &[usize] {
        &self.control_flow_parameters
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

fn analyse_summand(
    summand: &CfgSummand<'_>,
    parameters: &[DataVariable],
    rewrite: &mut impl FnMut(&DataExpression) -> DataExpression,
) -> SummandAnalysis {
    let mut conjuncts = Vec::new();
    collect_conjuncts(summand.condition, &mut conjuncts);

    let mut source = HashMap::new();
    for conjunct in &conjuncts {
        if let Some((index, value)) = as_parameter_equality(conjunct, parameters, rewrite) {
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

/// The name of the function symbol that `expression` applies, if any.
fn application_of(expression: &DataExpression) -> Option<String> {
    match expression.kind() {
        DataExpressionKind::Application(application) => {
            match DataExpression::from(application.arg(0).protect()).kind() {
                DataExpressionKind::FunctionSymbol(symbol) => Some(symbol.name().to_string()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Collects the top-level conjuncts of `expression`, flattening nested `&&`.
fn collect_conjuncts(expression: &DataExpression, conjuncts: &mut Vec<DataExpression>) {
    if application_of(expression).as_deref() == Some("&&") {
        for argument in expression.data_arguments() {
            collect_conjuncts(&argument.protect(), conjuncts);
        }
    } else {
        conjuncts.push(expression.clone());
    }
}

/// When `expression` is `d == c` or `c == d` for a parameter `d` and a closed `c`, returns the
/// position of `d` and the normal form of `c`.
fn as_parameter_equality<F>(
    expression: &DataExpression,
    parameters: &[DataVariable],
    rewrite: &mut F,
) -> Option<(usize, DataExpression)> 
    where F: FnMut(&DataExpression) -> DataExpression
{
    if application_of(expression).as_deref() != Some("==") {
        return None;
    }

    let arguments: Vec<DataExpression> = expression.data_arguments().map(|argument| argument.protect()).collect();
    let [left, right] = arguments.as_slice() else {
        return None;
    };

    match_parameter_constant(left, right, parameters, rewrite)
        .or_else(|| match_parameter_constant(right, left, parameters, rewrite))
}

fn match_parameter_constant<F>(
    variable: &DataExpression,
    constant: &DataExpression,
    parameters: &[DataVariable],
    rewrite: &mut F,
) -> Option<(usize, DataExpression)> 
    where F: FnMut(&DataExpression) -> DataExpression
{
    let DataExpressionKind::Variable(variable) = variable.kind() else {
        return None;
    };

    let index = parameters.iter().position(|parameter| parameter.copy() == variable)?;
    closed_value(constant, rewrite).map(|value| (index, value))
}

/// The normal form of `term` when that is closed.
fn closed_value<F>(
    term: &DataExpression,
    rewrite: &mut F,
) -> Option<DataExpression> 
    where F: FnMut(&DataExpression) -> DataExpression
{
    let normal_form = rewrite(term);
    is_closed(&normal_form).then_some(normal_form)
}
