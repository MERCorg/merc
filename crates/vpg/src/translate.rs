use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt;
use std::ops::ControlFlow;

use log::debug;
use log::info;

use log::warn;
use merc_collections::ByteCompressedVec;
use merc_collections::IndexedSet;
use merc_io::TimeProgress;
use merc_lts::LTS;
use merc_lts::LabelledTransitionSystem;
use merc_lts::StateIndex;
use merc_lts::Transition;
use merc_lts::TransitionLabel;
use merc_syntax::ActFrm;
use merc_syntax::ActFrmBinaryOp;
use merc_syntax::ActFrmKind;
use merc_syntax::FixedPointOperator;
use merc_syntax::FreshStateVarGenerator;
use merc_syntax::ModalityOperator;
use merc_syntax::MultiAction;
use merc_syntax::RegFrm;
use merc_syntax::RegFrmKind;
use merc_syntax::StateFrm;
use merc_syntax::StateFrmKind;
use merc_syntax::StateFrmOp;
use merc_syntax::StateVarDecl;
use merc_syntax::Traverse;
use merc_syntax::UntypedStateFrmSpec;
use merc_syntax::respan;
use merc_typecheck::FormulaType;
use merc_typecheck::ModalEquationSystem;
use merc_typecheck::ModalSpecification;
use merc_utilities::MercError;
use merc_utilities::Span;

use crate::ParityGame;
use crate::Player;
use crate::Priority;
use crate::VertexIndex;
use crate::compute_reachable;

/// Type checks `spec` (running [`translate_regular_formulas`] first) and translates it against
/// `lts` into a parity game. `spec`'s own `act` declarations are optional: with none, every action
/// is accepted as a "simple action" — a plain LTS label, matched structurally rather than typed —
/// see [`merc_typecheck::ModalSpecification`].
pub fn translate(
    lts: &LabelledTransitionSystem<String>,
    mut spec: UntypedStateFrmSpec,
) -> Result<ParityGame, MercError> {
    // Parses all labels into MultiAction once
    let labels = lts
        .labels()
        .iter()
        .map(|label| {
            if label.is_tau_label() {
                Ok(MultiAction::tau())
            } else {
                MultiAction::parse(label)
            }
        })
        .collect::<Result<Vec<MultiAction>, MercError>>()?;

    let mut identifier_generator = FreshStateVarGenerator::new(&spec.formula);
    spec.formula = translate_regular_formulas(spec.formula, &mut identifier_generator);
    debug!("Translated regular formulas: {}", spec.formula);

    let checked = ModalSpecification::from_untyped(spec, FormulaType::Bool)?;
    let equation_system = checked.equation_system();
    debug!("{}", equation_system);

    // Warn about any labels that are used in the formula but do not correspond to any label in the LTS.
    warn_unknown_action_labels(equation_system, &labels);

    let mut algorithm: Translation<'_, _, ()> = Translation::new(lts, &labels, equation_system);
    algorithm.translate(lts.initial_state_index(), 0, |_| (), |_, _| Ok(()))?;

    // Construct the parity game from the collected vertices and edges, where the `()` edge label is ignored.
    let vertices = algorithm.vertices();
    let result = ParityGame::from_edges(
        VertexIndex::new(0),
        vertices.iter().map(|(player, _)| *player).collect(),
        ByteCompressedVec::with_iter(vertices.iter().map(|(_, priority)| *priority)),
        true,
        || algorithm.edges.iter().map(|(s, _, t)| (*s, *t)),
    );

    // Check that all vertices are reachable from the initial vertex. After
    // totality it could be that the true or false nodes are not reachable.
    if cfg!(debug_assertions) {
        let (_, reachable_vertices) = compute_reachable(&result);
        debug_assert!(
            reachable_vertices.iter().all(|v| v.is_some()),
            "Not all vertices are reachable from the initial vertex"
        );
    }

    Ok(result)
}

/// Produces a warning for each label that is used in the formula but does not correspond to any label in the LTS.
pub fn warn_unknown_action_labels(equation_system: &ModalEquationSystem, labels: &[MultiAction]) {
    for i in 0..equation_system.len() {
        warn_unknown_action_labels_in(equation_system.equation(i).body(), labels);
    }
}

/// A traversal covers a single node type, so the modalities, the regular formulas they carry and
/// the action formulas inside those are three nested traversals.
fn warn_unknown_action_labels_in(formula: &StateFrm, labels: &[MultiAction]) {
    formula.visit::<(), _>(|statefrm| {
        if let StateFrmKind::Modality { formula, .. } = &statefrm.node {
            formula.visit::<(), _>(|regfrm| {
                if let RegFrmKind::Action(act_frm) = &regfrm.node {
                    act_frm.visit::<(), _>(|act_frm| {
                        if let ActFrmKind::MultAct(action) = &act_frm.node
                            && !labels.contains(action)
                        {
                            warn!(
                                "Label {} in formula does not correspond to any label in the LTS",
                                action
                            );
                        }

                        ControlFlow::Continue(())
                    });
                }

                ControlFlow::Continue(())
            });
        }

        ControlFlow::Continue(())
    });
}

/// Translates regular formulas in modalities to fixpoint equations.
///
/// # Details
///
/// Applies these transformations:
///
/// ```plain
/// [a*]phi = nu I. [a]I && phi
/// [a+]phi = [a](nu I. [a]I && phi)
/// [a+b]phi = [a]phi || [b]phi
/// [a.b]phi = [a][b]phi
/// ```
///
/// and symmetrical for the diamond operator:
/// ```plain
/// <a*>phi = mu I. <a>I || phi
/// <a+>phi = <a>(mu I. <a>I || phi)
/// ```
fn translate_regular_formulas(formula: StateFrm, identifier_generator: &mut FreshStateVarGenerator) -> StateFrm {
    let translated = formula.apply::<Infallible, _>(|subformula| {
        if let StateFrmKind::Modality {
            operator,
            formula,
            expr,
        } = &subformula.node
        {
            return match &formula.node {
                merc_syntax::RegFrmKind::Action(_action_frm) => Ok(None),
                merc_syntax::RegFrmKind::Iteration(reg_frm) => {
                    // Generate the I equation and replace the regular formula with it.
                    let iteration_var = identifier_generator.generate("I");
                    Ok(Some(translate_regular_formulas(
                        convert_regular_iteration(*operator, reg_frm, iteration_var, operator, expr),
                        identifier_generator,
                    )))
                }
                merc_syntax::RegFrmKind::Plus(reg_frm) => {
                    // Generate the I equation and replace the regular formula with it.
                    let iteration_var = identifier_generator.generate("I");
                    Ok(Some(
                        StateFrmKind::Modality {
                            operator: *operator,
                            formula: *reg_frm.clone(),
                            expr: Box::new(translate_regular_formulas(
                                convert_regular_iteration(*operator, reg_frm, iteration_var, operator, expr),
                                identifier_generator,
                            )),
                        }
                        .into(),
                    ))
                }
                merc_syntax::RegFrmKind::Sequence { lhs, rhs } => Ok(Some(translate_regular_formulas(
                    StateFrmKind::Modality {
                        operator: *operator,
                        formula: *lhs.clone(),
                        expr: Box::new(
                            StateFrmKind::Modality {
                                operator: *operator,
                                formula: *rhs.clone(),
                                expr: expr.clone(),
                            }
                            .into(),
                        ),
                    }
                    .into(),
                    identifier_generator,
                ))),
                merc_syntax::RegFrmKind::Choice { lhs, rhs } => Ok(Some(translate_regular_formulas(
                    StateFrmKind::Binary {
                        op: StateFrmOp::Disjunction,
                        lhs: Box::new(
                            StateFrmKind::Modality {
                                operator: *operator,
                                formula: *lhs.clone(),
                                expr: expr.clone(),
                            }
                            .into(),
                        ),
                        rhs: Box::new(
                            StateFrmKind::Modality {
                                operator: *operator,
                                formula: *rhs.clone(),
                                expr: expr.clone(),
                            }
                            .into(),
                        ),
                    }
                    .into(),
                    identifier_generator,
                ))),
            };
        }

        Ok(None)
    });

    match translated {
        Ok(formula) => formula,
        Err(error) => match error {},
    }
}

/// Convert an iteration regular formula to a fixpoint formula
///
/// # Details
///
/// The `modality` is the modality of the outer formula.
fn convert_regular_iteration(
    modality: ModalityOperator,
    reg_frm: &RegFrm,
    iteration_var: String,
    operator: &ModalityOperator,
    expr: &StateFrm,
) -> StateFrm {
    StateFrmKind::FixedPoint {
        operator: if modality == ModalityOperator::Box {
            FixedPointOperator::Greatest
        } else {
            FixedPointOperator::Least
        },
        variable: StateVarDecl::new(respan(Span::default(), iteration_var.clone()), Vec::new()),
        body: Box::new(
            StateFrmKind::Binary {
                op: if modality == ModalityOperator::Box {
                    StateFrmOp::Conjunction
                } else {
                    StateFrmOp::Disjunction
                },
                lhs: Box::new(
                    StateFrmKind::Modality {
                        operator: *operator,
                        formula: reg_frm.clone(),
                        expr: Box::new(StateFrmKind::Id(respan(Span::default(), iteration_var), Vec::new()).into()),
                    }
                    .into(),
                ),
                rhs: Box::new(expr.clone()),
            }
            .into(),
        ),
    }
    .into()
}

/// Is used to distinguish between StateFrm and Equation vertices in the vertex map.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Formula<'a> {
    StateFrm(&'a StateFrm),
    Equation(usize),
}

impl fmt::Display for Formula<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Formula::StateFrm(statefrm) => write!(f, "{}", statefrm),
            Formula::Equation(i) => write!(f, "Equation({})", i),
        }
    }
}

/// Local struct to keep track of the translation state. The generic bound `E` is the
/// type of the edge labels.
///
/// Implements the translation from (s, Ψ) pairs to VPG vertices and edges.
/// However, to avoid the complication of merging sub-results we immediately
/// store the vertices and edges into mutable vectors. Furthermore, to avoid
/// stack overflows we use a breadth-first search approach with a queue. This
/// means that during queuing we immediately assign a fresh index to each (s, Ψ)
/// pair (if it does not yet exist) and then queue it to assign its actual
/// values later on.
pub(crate) struct Translation<'a, L, E> {
    vertex_map: IndexedSet<(StateIndex, Formula<'a>)>,
    vertices: Vec<Option<(Player, Priority)>>,
    edges: Vec<(VertexIndex, E, VertexIndex)>,

    /// Temporary storage used to check for duplicated outgoing edges.
    seen_modality_targets: HashMap<VertexIndex, usize>,

    /// Used for the breadth first search.
    queue: Vec<(StateIndex, Formula<'a>, VertexIndex)>,

    /// The parsed labels of the LTS.
    parsed_labels: &'a Vec<MultiAction>,

    /// The labelled transition system being translated.
    lts: &'a L,

    /// A reference to the modal equation system being translated.
    equation_system: &'a ModalEquationSystem,

    /// Use to print progress information.
    progress: TimeProgress<usize>,
}

impl<'a, L: LTS, E> Translation<'a, L, E> {
    /// Creates a new translation instance.
    pub(crate) fn new(
        lts: &'a L,
        parsed_labels: &'a Vec<MultiAction>,
        equation_system: &'a ModalEquationSystem,
    ) -> Self {
        let progress: TimeProgress<usize> = TimeProgress::new(
            |num_of_vertices: usize| {
                info!("Translated {} vertices...", num_of_vertices);
            },
            1,
        );

        Self {
            vertex_map: IndexedSet::new(),
            vertices: Vec::new(),
            edges: Vec::new(),
            seen_modality_targets: HashMap::new(),
            queue: Vec::new(),
            lts,
            parsed_labels,
            equation_system,
            progress,
        }
    }

    /// Perform the translation for the given `initial_state` and `initial_equation_index`.
    ///
    /// The `labelling` function is used to compute the edge label for the
    /// outgoing edges of this vertex. The argument is the transition
    /// corresponding to the modality, or None if there is no modality (e.g.,
    /// for conjunctions).
    pub(crate) fn translate<F, C>(
        &mut self,
        initial_state: StateIndex,
        initial_equation_index: usize,
        labelling: F,
        combine_labelling: C,
    ) -> Result<(), MercError>
    where
        F: Fn(Option<Transition>) -> E,
        C: Fn(&mut E, E) -> Result<(), MercError>,
    {
        // We store (state, formula, N) into the queue, where N is the vertex number assigned to this pair. This means
        // that during the traversal we can assume this N to exist.
        self.queue = vec![(
            initial_state,
            Formula::Equation(initial_equation_index),
            VertexIndex::new(0),
        )];
        self.vertex_map
            .insert((initial_state, Formula::Equation(initial_equation_index)));
        self.vertices.push(None); // Placeholder for the initial vertex

        while let Some((s, formula, vertex_index)) = self.queue.pop() {
            debug!("Translating vertex {}: (s={}, formula={})", vertex_index, s, formula);
            self.progress.print(self.vertices.len());
            match formula {
                Formula::StateFrm(f) => {
                    self.translate_vertex(s, f, vertex_index, &labelling, &combine_labelling)?;
                }
                Formula::Equation(i) => {
                    self.translate_equation(s, i, vertex_index, &labelling);
                }
            }
        }

        Ok(())
    }

    /// Returns the collected vertices with placeholders removed.
    pub(crate) fn vertices(&self) -> Vec<(Player, Priority)> {
        debug_assert!(
            self.vertices.iter().all(|v| v.is_some()),
            "All vertices should be assigned before retrieving the vertices"
        );

        self.vertices
            .iter()
            .filter_map(|vertex| vertex.as_ref().copied())
            .collect()
    }

    /// Returns the collected edges, where the edge label is ignored.
    pub(crate) fn edges(&self) -> &Vec<(VertexIndex, E, VertexIndex)> {
        &self.edges
    }

    /// Translate a single vertex (s, Ψ) into the variability parity game vertex
    /// and its outgoing edges.
    fn translate_vertex<F, C>(
        &mut self,
        s: StateIndex,
        formula: &'a StateFrm,
        vertex_index: VertexIndex,
        labelling: &F,
        combine_labelling: &C,
    ) -> Result<(), MercError>
    where
        F: Fn(Option<Transition>) -> E,
        C: Fn(&mut E, E) -> Result<(), MercError>,
    {
        match &formula.node {
            StateFrmKind::True => {
                // (s, true) → odd, 0
                self.set_vertex(vertex_index, Player::Odd, Priority::new(0));
            }
            StateFrmKind::False => {
                // (s, false) → even, 0
                self.set_vertex(vertex_index, Player::Even, Priority::new(0));
            }
            StateFrmKind::Binary { op, lhs, rhs } => {
                match op {
                    StateFrmOp::Conjunction => {
                        // (s, Ψ_1 ∧ Ψ_2) →_P odd, (s, Ψ_1) and (s, Ψ_2), 0
                        self.set_vertex(vertex_index, Player::Odd, Priority::new(0));
                        let s_psi_1 = self.queue_vertex(s, Formula::StateFrm(lhs));
                        let s_psi_2 = self.queue_vertex(s, Formula::StateFrm(rhs));

                        self.edges.push((vertex_index, labelling(None), s_psi_1));
                        self.edges.push((vertex_index, labelling(None), s_psi_2));
                    }
                    StateFrmOp::Disjunction => {
                        // (s, Ψ_1 ∨ Ψ_2) →_P even, (s, Ψ_1) and (s, Ψ_2), 0
                        self.set_vertex(vertex_index, Player::Even, Priority::new(0));
                        let s_psi_1 = self.queue_vertex(s, Formula::StateFrm(lhs));
                        let s_psi_2 = self.queue_vertex(s, Formula::StateFrm(rhs));

                        self.edges.push((vertex_index, labelling(None), s_psi_1));
                        self.edges.push((vertex_index, labelling(None), s_psi_2));
                    }
                    _ => {
                        unimplemented!("Cannot translate binary operator in {}", formula);
                    }
                }
            }
            StateFrmKind::Resolved(_name, _args, id) => {
                let (i, _equation) = self
                    .equation_system
                    .find_equation_by_id(*id)
                    .expect("Variable must correspond to an equation");

                self.set_vertex(vertex_index, Player::Odd, Priority::new(0)); // The priority and owner do not matter here
                let equation_vertex = self.queue_vertex(s, Formula::Equation(i));
                self.edges.push((vertex_index, labelling(None), equation_vertex));
            }
            StateFrmKind::Modality {
                operator,
                formula,
                expr,
            } => {
                self.translate_modality_vertex(
                    s,
                    *operator,
                    formula,
                    expr,
                    vertex_index,
                    labelling,
                    combine_labelling,
                )?;
            }
            _ => {
                unimplemented!("Cannot translate formula {}", formula);
            }
        }

        Ok(())
    }

    /// Translates a modality vertex `(s, [a]Ψ)` or `(s, <a>Ψ)` into the variability parity game vertex and its outgoing edges.
    ///
    /// # Details
    ///
    /// Applies the following transformations:
    ///
    /// > `(s, [a] Ψ)` → odd, `(s', Ψ)` for all s' with s -a-> s', 0
    /// > `(s, <a> Ψ)` → even, `(s', Ψ)` for all s' with s -a-> s', 0
    #[allow(clippy::too_many_arguments)]
    fn translate_modality_vertex<F, C>(
        &mut self,
        s: StateIndex,
        operator: ModalityOperator,
        formula: &'a RegFrm,
        expr: &'a StateFrm,
        vertex_index: VertexIndex,
        labelling: &F,
        combine_labelling: &C,
    ) -> Result<(), MercError>
    where
        F: Fn(Option<Transition>) -> E,
        C: Fn(&mut E, E) -> Result<(), MercError>,
    {
        let player = match operator {
            ModalityOperator::Box => Player::Odd,
            ModalityOperator::Diamond => Player::Even,
        };

        self.set_vertex(vertex_index, player, Priority::new(0));
        self.seen_modality_targets.clear();

        for transition in self.lts.outgoing_transitions(s) {
            let action = &self.parsed_labels[*transition.label];

            if match_regular_formula(formula, action) {
                let s_prime_psi = self.queue_vertex(transition.to, Formula::StateFrm(expr));
                let label = labelling(Some(transition));

                if let Some(edge_index) = self.seen_modality_targets.get(&s_prime_psi).copied() {
                    combine_labelling(&mut self.edges[edge_index].1, label)?;
                } else {
                    let edge_index = self.edges.len();
                    self.edges.push((vertex_index, label, s_prime_psi));
                    self.seen_modality_targets.insert(s_prime_psi, edge_index);
                }
            }
        }

        Ok(())
    }

    /// Applies the translation to the given (s, equation) vertex.
    fn translate_equation<F>(&mut self, s: StateIndex, equation_index: usize, vertex_index: VertexIndex, labelling: &F)
    where
        F: Fn(Option<Transition>) -> E,
    {
        let equation = self.equation_system.equation(equation_index);
        match equation.operator() {
            FixedPointOperator::Least => {
                // (s, μ X. Ψ) →_P odd, (s, Ψ[x := μ X. Ψ]), 2 * floor(AD(Ψ)/2) + 1. In Rust division is already floor.
                self.set_vertex(
                    vertex_index,
                    Player::Odd,
                    Priority::new(2 * (self.equation_system.alternation_depth(equation_index) / 2) + 1),
                );
                let s_psi = self.queue_vertex(s, Formula::StateFrm(equation.body()));
                self.edges.push((vertex_index, labelling(None), s_psi));
            }
            FixedPointOperator::Greatest => {
                // (s, ν X. Ψ) →_P even, (s, Ψ[x := ν X. Ψ]), 2 * floor(AD(Ψ)/2). In Rust division is already floor.
                self.set_vertex(
                    vertex_index,
                    Player::Even,
                    Priority::new(2 * (self.equation_system.alternation_depth(equation_index) / 2)),
                );
                let s_psi = self.queue_vertex(s, Formula::StateFrm(equation.body()));
                self.edges.push((vertex_index, labelling(None), s_psi));
            }
        }
    }

    /// Assigns a concrete vertex value exactly once.
    fn set_vertex(&mut self, vertex_index: VertexIndex, player: Player, priority: Priority) {
        debug_assert!(
            self.vertices[vertex_index].is_none(),
            "Vertex {vertex_index} was assigned more than once"
        );
        self.vertices[vertex_index] = Some((player, priority));
    }

    /// Queues a new pair to be translated, returning its vertex index.
    fn queue_vertex(&mut self, s: StateIndex, formula: Formula<'a>) -> VertexIndex {
        let (index, inserted) = self.vertex_map.insert((s, formula.clone()));
        let vertex_index = VertexIndex::new(*index);

        if inserted {
            // New vertex, assign placeholder values
            self.vertices.resize(*vertex_index + 1, None);
            self.queue.push((s, formula, vertex_index));
        }

        vertex_index
    }
}

/// Returns true iff the given action matches the regular formula.
fn match_regular_formula(formula: &RegFrm, action: &MultiAction) -> bool {
    match &formula.node {
        RegFrmKind::Action(action_formula) => match_action_formula(action_formula, action),
        RegFrmKind::Choice { lhs, rhs } => match_regular_formula(lhs, action) || match_regular_formula(rhs, action),
        _ => {
            unimplemented!("Cannot translate regular formula {}", formula);
        }
    }
}

/// Returns true iff the given action matches the action formula.
fn match_action_formula(formula: &ActFrm, action: &MultiAction) -> bool {
    match &formula.node {
        ActFrmKind::True => true,
        ActFrmKind::False => false,
        ActFrmKind::MultAct(expected_action) => match_multi_action(expected_action, action),
        ActFrmKind::Binary { op, lhs, rhs } => match op {
            ActFrmBinaryOp::Union => match_action_formula(lhs, action) || match_action_formula(rhs, action),
            ActFrmBinaryOp::Intersect => match_action_formula(lhs, action) && match_action_formula(rhs, action),
            _ => {
                unimplemented!("Cannot translate binary operator {}", formula);
            }
        },
        ActFrmKind::Negation(expr) => !match_action_formula(expr, action),
        _ => {
            unimplemented!("Cannot translate action formula {}", formula);
        }
    }
}

/// Returns true iff the two multi-actions denote the same multi-set of actions.
///
/// The order of actions within a multi-action is irrelevant, but the multiplicity of each action
/// must match exactly (e.g. `a | a | b` does not match `a | b | b`).
fn match_multi_action(expected: &MultiAction, actual: &MultiAction) -> bool {
    if expected.actions.len() != actual.actions.len() {
        return false;
    }

    // Compare as multisets without cloning the action data (notably the action
    // identifiers): sort references and compare element-wise.
    let mut expected_actions: Vec<_> = expected.actions.iter().collect();
    let mut actual_actions: Vec<_> = actual.actions.iter().collect();
    expected_actions.sort_unstable();
    actual_actions.sort_unstable();
    expected_actions == actual_actions
}

#[cfg(test)]
mod tests {
    use merc_lts::read_aut;
    use merc_macros::merc_test;
    use merc_syntax::UntypedStateFrmSpec;

    use crate::PG;
    use crate::compute_reachable;
    use crate::translate;

    #[merc_test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_translate_abp_infinitely_often_receive_d1() {
        let lts = read_aut(include_bytes!("../../../examples/lts/abp.aut") as &[u8]).unwrap();
        let spec =
            UntypedStateFrmSpec::parse(include_str!("../../../examples/pbes/infinitely_often_receive_d1.mcf")).unwrap();

        // No `act` declarations: the formula's `r1(d1)` is a "simple action", the label as written
        // in `abp.aut` itself, with no data specification to check `d1` against.
        let pg = translate(&lts, spec).unwrap();

        assert!(pg.is_total(), "The translated parity game should be total");
        assert!(
            compute_reachable(&pg).1.iter().all(|v| v.is_some()),
            "All vertices should be reachable from the initial vertex"
        );
    }
}
