//! Authors: Maurice Laveaux and Sjef van Loo

use std::collections::HashMap;
use std::fmt;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;

use log::debug;
use oxidd::BooleanFunction;
use oxidd::Manager;
use oxidd::ManagerRef;
use oxidd::bdd::BDDFunction;
use oxidd::bdd::BDDManagerRef;

use merc_lts::LTS;
use merc_lts::LabelIndex;
use merc_lts::LabelledTransitionSystem;
use merc_lts::StateIndex;
use merc_lts::Transition;
use merc_lts::TransitionLabel;
use merc_lts::read_aut;
use merc_symbolic::FormatConfigSet;
use merc_syntax::DataExpr;
use merc_syntax::DataExprKind;
use merc_syntax::MultiAction;
use merc_utilities::MercError;

/// Reads a .aut file as feature transition system by using the associated
/// feature diagram.
///
/// # Details
///
/// The action labels of a feature transition system are annotated with a
/// special `BDD` struct that is defined as:
///
/// > `struct BDD = node(var, true, false) | tt | ff`
///
/// The `features` map contains the mapping from variable names to their
/// corresponding BDD variable, which *must* be defined in the BDD manager.
pub fn read_fts<R: Read>(
    manager_ref: &BDDManagerRef,
    reader: R,
    features: HashMap<String, BDDFunction>,
) -> Result<FeatureTransitionSystem<String>, MercError> {
    // Read the underlying LTS, where the labels are in plain text
    let aut = read_aut(reader)?;

    // Parse the labels as data expressions
    let mut feature_labels = Vec::new();
    for label in aut.labels() {
        let action = if label.is_tau_label() {
            MultiAction::tau()
        } else {
            MultiAction::parse(label)?
        };

        debug!("Parsed action: {}", action);
        if action.actions.len() > 1 {
            return Err(MercError::from(format!(
                "Cannot read feature transition system: action \"{}\" has multiple actions",
                label
            )));
        }

        if let Some(action) = action.actions.first() {
            if let Some(arg) = action.args.first() {
                let expr = data_expr_to_bdd(manager_ref, &features, arg)?;
                debug!("Feature label {}", FormatConfigSet(&expr));
                feature_labels.push(expr);
            } else {
                feature_labels.push(manager_ref.with_manager_shared(|manager| BDDFunction::t(manager)));
            }
        } else {
            // This is the tau action, is always enabled.
            feature_labels.push(manager_ref.with_manager_shared(|manager| BDDFunction::t(manager)));
        }
    }

    let original_labels = aut.labels().to_vec();

    // Find the associated feature for each label and relabel the LTS accordingly.
    let aut = aut.relabel_all(|label| {
        let associated_feature = feature_labels[original_labels
            .iter()
            .position(|orig| *orig == label)
            .ok_or("label must have an associated feature")?]
        .clone();

        // This is fine for the tau label.
        Ok(FeaturedLabel::new(label, associated_feature))
    })?;

    Ok(FeatureTransitionSystem::new(aut, features))
}

/// Converts the given data expression into a BDD function.
///
/// The input should be a data expression of the shape: expr = node(var, expr, expr) | tt | ff.
fn data_expr_to_bdd(
    manager_ref: &BDDManagerRef,
    variables: &HashMap<String, BDDFunction>,
    expr: &DataExpr,
) -> Result<BDDFunction, MercError> {
    match &expr.node {
        DataExprKind::Application { function, arguments } => {
            match &function.node {
                // A node must be of the shape 'node(var, true_branch, false_branch)'
                DataExprKind::Id(name) => {
                    if name == "node" {
                        let variable = format!("{}", arguments[0]);
                        let then_branch = data_expr_to_bdd(manager_ref, variables, &arguments[1])?;
                        let else_branch = data_expr_to_bdd(manager_ref, variables, &arguments[2])?;
                        Ok(variables
                            .get(&variable)
                            .ok_or(format!("Variable \"{}\" not found in feature diagram", variable))?
                            .ite(&then_branch, &else_branch)?)
                    } else {
                        unimplemented!("Conversion of data expression to BDD not implemented for this function");
                    }
                }
                _ => unimplemented!("Conversion of data expression to BDD not implemented for this function"),
            }
        }
        DataExprKind::Id(name) => {
            // Deal with the base cases.
            match name.as_str() {
                "tt" => Ok(manager_ref.with_manager_shared(|manager| BDDFunction::t(manager))),
                "ff" => Ok(manager_ref.with_manager_shared(|manager| BDDFunction::f(manager))),
                _ => unimplemented!("Cannot convert data expression \"{expr}\" to BDD"),
            }
        }
        _ => unimplemented!("Cannot convert data expression \"{expr}\" to BDD"),
    }
}

/// A feature diagram represented as a BDD.
pub struct FeatureDiagram {
    /// The mapping from feature names to their BDD variable.
    features: HashMap<String, BDDFunction>,

    /// The feature names in the order they are defined in the input.
    feature_names: Vec<String>,

    /// Stores the set of products as a BDD function.
    configuration: BDDFunction,
}

impl FeatureDiagram {
    /// Reads feature diagram from the input.
    ///
    /// # Details
    ///
    /// The first line is a list of variable names, separated by commas. The
    /// second line is the initial configuration, represented as a data
    /// expression. This function will initialize the BDD manager with the
    /// variables read from the first line, and assumes that the manager has no
    /// variables yet defined.
    pub fn from_reader<R: Read>(manager_ref: &BDDManagerRef, input: R) -> Result<Self, MercError> {
        manager_ref.with_manager_exclusive(|manager| {
            debug_assert_eq!(
                manager.num_vars(),
                0,
                "A BDD manager can only hold the variables for a single feature diagram"
            )
        });

        let input = BufReader::new(input);
        let mut line_iter = input.lines();
        let first_line = line_iter.next().ok_or("Expected variable names line")??;

        let variable_names: Vec<String> = first_line.split(',').map(|s| s.trim().to_string()).collect();
        let variables = manager_ref.with_manager_exclusive(|manager| -> Result<Vec<BDDFunction>, MercError> {
            Ok(manager
                .add_named_vars(variable_names.iter())
                .map_err(|e| format!("Failed to create variables: {e}"))?
                .map(|i| BDDFunction::var(manager, i))
                .collect::<Result<Vec<_>, _>>()?)
        })?;

        let variables = HashMap::from_iter(variable_names.iter().cloned().zip(variables));

        let second_line = line_iter.next().ok_or("Expected initial configuration line")??;
        let initial_configuration = data_expr_to_bdd(manager_ref, &variables, &DataExpr::parse(&second_line)?)?;

        Ok(Self {
            features: variables,
            feature_names: variable_names,
            configuration: initial_configuration,
        })
    }

    /// Returns the configuration of the feature diagram.
    pub fn configuration(&self) -> &BDDFunction {
        &self.configuration
    }

    /// Returns the feature variables used in the feature diagram.
    pub fn features(&self) -> &HashMap<String, BDDFunction> {
        &self.features
    }

    /// Returns the feature names used in the feature diagram, in the order they are defined in the input.
    pub fn feature_names(&self) -> &[String] {
        &self.feature_names
    }
}

impl fmt::Debug for FeatureDiagram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "features = {}", self.feature_names().join(", "))
    }
}

/// A feature transition system, i.e., a labelled transition system
/// where each label is associated with a feature expression.
pub struct FeatureTransitionSystem<L: TransitionLabel> {
    /// The underlying labelled transition system.
    lts: LabelledTransitionSystem<FeaturedLabel<L>>,

    /// The features associated with this feature transition system.
    features: HashMap<String, BDDFunction>,
}

impl<L: TransitionLabel> FeatureTransitionSystem<L> {
    /// Creates a new feature transition system.
    pub fn new(lts: LabelledTransitionSystem<FeaturedLabel<L>>, features: HashMap<String, BDDFunction>) -> Self {
        Self { lts, features }
    }

    /// Returns the features used in the feature diagram.
    pub fn features(&self) -> &HashMap<String, BDDFunction> {
        &self.features
    }
}

/// A transition label associated with a feature expression.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FeaturedLabel<L: TransitionLabel> {
    label: L,
    feature_expr: BDDFunction,
}

impl<L: TransitionLabel> FeaturedLabel<L> {
    /// Creates a new featured label with the given label and feature expression.
    pub fn new(label: L, feature_expr: BDDFunction) -> Self {
        Self { label, feature_expr }
    }

    /// Returns the label of this featured label.
    pub fn label(&self) -> &L {
        &self.label
    }

    /// Returns the feature expression of this featured label.
    pub fn feature_expr(&self) -> &BDDFunction {
        &self.feature_expr
    }
}

impl<L: TransitionLabel> TransitionLabel for FeaturedLabel<L> {
    delegate::delegate! {
        to self.label {
            fn matches_label(&self, label: &str) -> bool;
            fn is_tau_label(&self) -> bool;
        }
    }

    fn from_index(_i: usize) -> Self {
        panic!("Cannot create FeaturedLabel from index");
    }

    fn tau_label() -> Self {
        // TODO: This is annoying, but we need a BDDManager to create the
        // feature label. Perhaps this should be refactored to not require the
        // tau label to be a constant.
        panic!("Cannot create a tau label for FeaturedLabel");
    }
}

impl<L: TransitionLabel> fmt::Debug for FeaturedLabel<L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} [{}]", self.label, FormatConfigSet(&self.feature_expr))
    }
}

impl<L: TransitionLabel> fmt::Display for FeaturedLabel<L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // This is also used for the output in .aut format, so should be valid labels.
        write!(f, "{}", self.label)
    }
}

impl<L: TransitionLabel> LTS for FeatureTransitionSystem<L> {
    type Label = FeaturedLabel<L>;

    fn merge_disjoint<Lts>(self, _other: &Lts) -> (LabelledTransitionSystem<FeaturedLabel<L>>, StateIndex) {
        unimplemented!("Merging feature transition systems is not yet implemented")
    }

    delegate::delegate! {
        to self.lts {
            fn initial_state_index(&self) -> StateIndex;
            fn num_of_states(&self) -> usize;
            fn num_of_labels(&self) -> usize;
            fn num_of_transitions(&self) -> usize;
            fn labels(&self) -> &[FeaturedLabel<L>];
            fn outgoing_transitions(&self, state_index: StateIndex) -> impl Iterator<Item = Transition>;
            fn iter_states(&self) -> impl Iterator<Item = StateIndex> + '_;
            fn is_hidden_label(&self, label_index: LabelIndex) -> bool;
        }
    }
}

#[cfg(test)]
mod tests {
    use merc_macros::merc_test;
    use merc_symbolic::BDD_CACHE_CAPACITY;
    use merc_symbolic::BDD_NODE_CAPACITY;

    use crate::FeatureDiagram;
    use crate::read_fts;

    #[merc_test]
    #[cfg_attr(miri, ignore)] // Oxidd does not support miri (specifically the crossbeam-epoch dependency)
    fn test_read_minepump_fts() {
        let manager_ref = oxidd::bdd::new_manager(BDD_NODE_CAPACITY, BDD_CACHE_CAPACITY, 1);

        let feature_diagram = FeatureDiagram::from_reader(
            &manager_ref,
            include_bytes!("../../../examples/vpg/minepump_fts.fd") as &[u8],
        )
        .unwrap();

        let _result = read_fts(
            &manager_ref,
            include_bytes!("../../../examples/vpg/minepump_fts.aut") as &[u8],
            feature_diagram.features,
        )
        .unwrap();
    }
}
