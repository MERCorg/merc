use std::cell::RefCell;
use std::fmt;
use std::marker::PhantomData;

use hashbrown::HashMap;
use itertools::Itertools;
use log::info;
use rustc_hash::FxBuildHasher;
use streaming_iterator::StreamingIterator;

use merc_collections::IndexedSet;
use merc_collections::Odometer;
use merc_collections::VecBag;
use merc_data::DataExpression;
use merc_io::TimeProgress;
use merc_lts::ConcurrentLtsBuilder;
use merc_lts::LTS;
use merc_lts::LabelIndex;
use merc_lts::LabelledTransitionSystem;
use merc_lts::LtsAction;
use merc_lts::LtsBuilder;
use merc_lts::LtsBuilderMem;
use merc_lts::LtsMultiAction;
use merc_lts::MutexLtsBuilder;
use merc_lts::PerStateDedup;
use merc_lts::SimpleAction;
use merc_lts::StateIndex;
use merc_lts::StateLabels;
use merc_lts::TransitionLabel;
use merc_syntax::CommExpr;
use merc_syntax::MultiActionLabel;
use merc_utilities::MercError;
use merc_utilities::ShardedCounter;
use merc_utilities::Timing;

use crate::ExplorationStrategy;
use crate::LPS;
use crate::StateEffect;
use crate::Summand;
use crate::configure_rayon_thread_pool;
use crate::explore;
use crate::explore_parallel;

/// The trait for labels used in the composition operator.
pub trait CombineLabel: TransitionLabel + Ord {
    /// Returns true iff the data arguments of two actions are compatible for communication.
    /// Defaults to `true` for label types without structured arguments (e.g., `String`).
    fn comm_args_compatible(&self, _other: &Self) -> bool {
        true
    }

    /// Creates a new action as the result of a communication expression with the given `label`.
    /// `representative` is one of the matched input actions and provides argument values.
    fn from_comm(label: String, representative: &Self) -> Self;
}

impl CombineLabel for SimpleAction {
    fn comm_args_compatible(&self, other: &Self) -> bool {
        self.arguments() == other.arguments()
    }

    fn from_comm(label: String, representative: &Self) -> Self {
        SimpleAction::new(label, representative.arguments().to_vec())
    }
}

impl CombineLabel for LtsAction {
    fn comm_args_compatible(&self, other: &Self) -> bool {
        self.arguments() == other.arguments()
    }

    fn from_comm(label: String, representative: &Self) -> Self {
        LtsAction::new(label, representative.arguments().to_vec())
    }
}

/// Computes the parallel composition hide(H, allow(A, comm(C, L1 || ... || Ln))).
///
/// The `builder` is used to construct the resulting LTS, which can also be
/// stored immediately in a file. Returns the result of [`LtsBuilder::finish`].
///
/// # Errors
///
/// Returns an error if `parallel_composition` is empty, if `allow` contains the
/// tau action, or if any expression in `comm` has a tau action or fewer than
/// two actions on its left-hand side, shares a left-hand side action with
/// another expression, or has its right-hand side appear in another
/// expression's left-hand side.
pub fn combine_lts<L: LTS<Label = LtsMultiAction<A>>, A: CombineLabel, B: LtsBuilder<L::Label>>(
    builder: &mut B,
    parallel_composition: Vec<L>,
    hide: &[String],
    allow: &[MultiActionLabel],
    comm: &[CommExpr],
    timing: &Timing,
) -> Result<B::LTS, MercError> {
    check_arguments(&parallel_composition, allow, comm)?;
    let lps = ComposedLts::new(&parallel_composition, hide, allow, comm);

    // Two different combinations of underlying transitions (e.g. from different LTSs in the
    // composition) can reduce to the same (label, successor) pair for a given state once hiding,
    // allow, and communication are applied. So deduplicate then,
    let mut dedup = PerStateDedup::new();

    let progress = TimeProgress::new(
        |(states, transitions): (usize, usize)| {
            info!("Explored {states} states, {transitions} transitions...");
        },
        1,
    );

    let mut num_of_states = 0;
    let initial = explore(
        &lps,
        ExplorationStrategy::Dfs,
        timing,
        &mut (&mut *builder, &mut dedup),
        |(builder, dedup), _state, _info| {
            num_of_states += 1;
            if num_of_states % 1024 == 0 {
                progress.print((num_of_states, builder.num_of_transitions() + dedup.len()));
            }
            Ok(())
        },
        |(builder, dedup), from, label, to| {
            dedup.add(from, label, to, |from, label, to| {
                builder.add_transition(from, label, to)
            })
        },
    )?;

    dedup.flush(|from, label, to| builder.add_transition(from, label, to))?;

    info!(
        "Composition complete: {} states, {} transitions",
        num_of_states,
        builder.num_of_transitions(),
    );

    builder.require_num_of_states(num_of_states);
    builder.finish(initial)
}

/// Computes the parallel composition like [`combine_lts`], using `threads` worker threads.
///
/// The states of the result are numbered sparsely, i.e. there can be indices without transitions that are not
/// reachable from the initial state.
///
/// # Errors
///
/// See [`combine_lts`].
pub fn combine_lts_parallel<L, A, B>(
    builder: &mut B,
    parallel_composition: Vec<L>,
    hide: &[String],
    allow: &[MultiActionLabel],
    comm: &[CommExpr],
    threads: usize,
    timing: &Timing,
) -> Result<B::LTS, MercError>
where
    L: LTS<Label = LtsMultiAction<A>> + Sync,
    A: CombineLabel + Send + Sync,
    B: ConcurrentLtsBuilder<L::Label>,
{
    check_arguments(&parallel_composition, allow, comm)?;

    let lps = ComposedLts::new(&parallel_composition, hide, allow, comm);
    let pool = configure_rayon_thread_pool(threads, false)?;

    let builder_ref: &B = builder;
    let states = ShardedCounter::new();
    let transitions = ShardedCounter::new();
    let progress = TimeProgress::new(
        |(states, transitions): (usize, usize)| {
            info!("Explored {states} states, {transitions} transitions...");
        },
        1,
    );

    // Forwards a deduplicated transition into the shared builder, see [`combine_lts`].
    let flush_one = |from, label: &L::Label, to| {
        transitions.increment();
        builder_ref.add_transition_shared(from, label, to)
    };

    let initial = timing.measure("explore", || -> Result<_, MercError> {
        pool.install(|| {
            // Every worker deduplicates the transitions of the states that it explores.
            let (initial, locals) = explore_parallel(
                &lps,
                || (PerStateDedup::new(), 0usize),
                |(_, num_of_states), _state, _info| {
                    states.increment();
                    *num_of_states += 1;
                    if *num_of_states % 1024 == 0 && progress.is_due() {
                        progress.print((states.get() as usize, transitions.get() as usize));
                    }
                    Ok(())
                },
                |(dedup, _), from, label, to| dedup.add(from, label, to, flush_one),
            )?;

            for (mut dedup, _) in locals {
                dedup.flush(flush_one)?;
            }
            Ok(initial)
        })
    })?;

    let num_of_states = states.get() as usize;
    info!(
        "Composition complete: {num_of_states} states, {} transitions",
        transitions.get()
    );

    builder.require_num_of_states(num_of_states);
    builder.finish(initial)
}

/// Computes the parallel composition like [`combine_lts_parallel`], for LTSs in the binary LTS format.
///
/// The arguments of their actions are terms that cannot be shared between threads, so they are interned by an
/// [`ActionArguments`] table first and restored in the result. The state labels of the LTSs are not used.
///
/// # Errors
///
/// See [`combine_lts`].
pub fn combine_lts_actions_parallel<S: StateLabels>(
    parallel_composition: Vec<LabelledTransitionSystem<LtsMultiAction<LtsAction>, S>>,
    hide: &[String],
    allow: &[MultiActionLabel],
    comm: &[CommExpr],
    threads: usize,
    timing: &Timing,
) -> Result<LabelledTransitionSystem<LtsMultiAction<LtsAction>>, MercError> {
    let arguments = RefCell::new(ActionArguments::default());
    let parallel_composition = parallel_composition
        .into_iter()
        .map(|lts| {
            lts.with_state_labels(())
                .relabel(|label| Ok(arguments.borrow_mut().intern(&label)))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut builder = MutexLtsBuilder::new(LtsBuilderMem::new(Vec::new(), Vec::new()));
    let result = combine_lts_parallel(&mut builder, parallel_composition, hide, allow, comm, threads, timing)?;

    let arguments = arguments.into_inner();
    result.relabel(|label| Ok(arguments.restore(&label)))
}

/// An [`LtsAction`] whose arguments are replaced by their index in an [`ActionArguments`] table.
///
/// Composition only compares the arguments of actions for equality, which interning preserves. Unlike the terms in
/// the arguments of an [`LtsAction`], this label can be shared between threads.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InternedAction {
    label: String,
    arguments: usize,
}

impl fmt::Display for InternedAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.label, self.arguments)
    }
}

impl TransitionLabel for InternedAction {
    fn is_tau_label(&self) -> bool {
        false
    }

    fn tau_label() -> Self {
        panic!("InternedAction does not support tau labels. Use LtsMultiAction for that.")
    }

    fn matches_label(&self, label: &str) -> bool {
        self.label == label
    }

    fn from_index(i: usize) -> Self {
        InternedAction {
            label: LtsAction::from_index(i).label().to_owned(),
            arguments: 0,
        }
    }
}

impl CombineLabel for InternedAction {
    fn comm_args_compatible(&self, other: &Self) -> bool {
        self.arguments == other.arguments
    }

    fn from_comm(label: String, representative: &Self) -> Self {
        InternedAction {
            label,
            arguments: representative.arguments,
        }
    }
}

/// Interns the arguments of [`LtsAction`]s, see [`InternedAction`].
#[derive(Default)]
pub struct ActionArguments {
    arguments: IndexedSet<Vec<DataExpression>>,
}

impl ActionArguments {
    /// Returns the multi-action in which the arguments of every action are interned.
    pub fn intern(&mut self, multi_action: &LtsMultiAction<LtsAction>) -> LtsMultiAction<InternedAction> {
        LtsMultiAction::new(
            multi_action
                .actions()
                .iter()
                .map(|action| InternedAction {
                    label: action.label().to_owned(),
                    arguments: *self.arguments.insert(action.arguments().to_vec()).0,
                })
                .collect(),
        )
    }

    /// Returns the multi-action in which the arguments of every action are restored.
    ///
    /// # Panics
    ///
    /// Panics if the arguments were not interned by this table.
    pub fn restore(&self, multi_action: &LtsMultiAction<InternedAction>) -> LtsMultiAction<LtsAction> {
        LtsMultiAction::new(
            multi_action
                .actions()
                .iter()
                .map(|action| {
                    let arguments = self
                        .arguments
                        .get_by_index(action.arguments)
                        .expect("The arguments must be interned by this table");
                    LtsAction::new(action.label.clone(), arguments.clone())
                })
                .collect(),
        )
    }
}

/// Checks the arguments of [`combine_lts`], see its documentation.
fn check_arguments<L>(
    parallel_composition: &[L],
    allow: &[MultiActionLabel],
    comm: &[CommExpr],
) -> Result<(), MercError> {
    if parallel_composition.is_empty() {
        return Err("At least one LTS is required for composition.".into());
    }

    if allow.iter().any(|allow| allow.is_tau_label()) {
        return Err("Allow set cannot contain the tau action.".into());
    }

    check_communications(comm)
}

/// Checks the restrictions on the communication expressions that [`combine_lts`] documents.
fn check_communications(comm: &[CommExpr]) -> Result<(), MercError> {
    for (i, comm_i) in comm.iter().enumerate() {
        if comm_i.from.is_tau_label() {
            return Err("Communication expressions cannot have tau actions on the left-hand side.".into());
        }

        if comm_i.from.actions.len() < 2 {
            return Err(format!(
                "Communication expressions must have at least two actions on the left-hand side, but {comm_i} does not."
            )
            .into());
        }

        for comm_j in comm
            .iter()
            .enumerate()
            .filter(|&(j, _)| j != i)
            .map(|(_, comm_j)| comm_j)
        {
            // The left hand side of a communication cannot overlap with any other communication's left hand side. This
            // is the definition used in mCRL2, for example a|b -> a is a valid communication expression.
            if comm_i.from.actions.iter().any(|a| comm_j.from.actions.contains(a)) {
                return Err(format!(
                    "Communication expressions cannot have overlapping left-hand sides, i.e. {comm_i} and {comm_j}."
                )
                .into());
            }

            // The right hand side of a communication cannot be in another communication's left hand side.
            if comm_j.from.actions.contains(&comm_i.to) {
                return Err(
                    format!("Communication expressions cannot have right-hand side in another communication's left-hand side, i.e. {comm_i} and {comm_j}.").into(),
                );
            }
        }
    }

    Ok(())
}

/// The parallel composition hide(H, allow(A, comm(C, L1 || ... || Ln))) as an [`LPS`], so that it can be explored by
/// [`explore`] and [`crate::explore_parallel`].
///
/// The state vector consists of one state per LTS. There is one summand for every non-empty subset of the LTSs, which
/// takes one transition in each LTS of the subset simultaneously and leaves the other LTSs unchanged.
struct ComposedLts<'a, L> {
    lts_list: &'a [L],
    hide: &'a [String],
    allow: &'a [MultiActionLabel],
    comm: &'a [CommExpr],

    /// For every LTS and label index, whether the label can be part of an allowed multi-action, see
    /// [`usable_action_names`]. Transitions with other labels are never combined.
    usable_labels: Vec<Vec<bool>>,

    summands: Vec<ComposedSummand<'a, L>>,
}

impl<'a, L: LTS<Label = LtsMultiAction<A>>, A: CombineLabel> ComposedLts<'a, L> {
    fn new(lts_list: &'a [L], hide: &'a [String], allow: &'a [MultiActionLabel], comm: &'a [CommExpr]) -> Self {
        let usable_names = usable_action_names(allow, comm);
        let is_usable = |label: &L::Label| {
            label
                .actions()
                .iter()
                .all(|action| usable_names.iter().any(|name| action.matches_label(name)))
        };

        Self {
            lts_list,
            hide,
            allow,
            comm,
            usable_labels: lts_list
                .iter()
                .map(|lts| lts.labels().iter().map(is_usable).collect())
                .collect(),
            summands: non_empty_subsets(lts_list.len())
                .into_iter()
                .map(|components| ComposedSummand {
                    components,
                    lts: PhantomData,
                })
                .collect(),
        }
    }
}

impl<'a, L: LTS<Label = LtsMultiAction<A>>, A: CombineLabel> LPS for ComposedLts<'a, L> {
    type Value = usize;
    type Label = LtsMultiAction<A>;
    type StateInfo = ();
    type Summand = ComposedSummand<'a, L>;

    fn initial_state(&self) -> Vec<usize> {
        self.lts_list
            .iter()
            .map(|lts| lts.initial_state_index().value())
            .collect()
    }

    fn summands(&self) -> &[Self::Summand] {
        &self.summands
    }

    fn create_context(&self) -> ComposedContext<'a, L> {
        ComposedContext {
            labels: CombinedLabels::new(self.lts_list, self.hide, self.allow, self.comm),
            choices: vec![Vec::new(); self.lts_list.len()],
            odometer: Odometer::new(),
            transition: ParallelTransition::default(),
        }
    }

    fn prepare<'b>(
        &'b self,
        context: &mut ComposedContext<'a, L>,
        state: &'b [usize],
    ) -> impl Iterator<Item = usize> + 'b {
        // Collect the usable outgoing transitions of every LTS once, so that the summands can share them.
        for (i, (lts, choices)) in self.lts_list.iter().zip(&mut context.choices).enumerate() {
            choices.clear();
            choices.extend(
                lts.outgoing_transitions(StateIndex::new(state[i]))
                    .filter(|transition| self.usable_labels[i][transition.label])
                    .map(|transition| (transition.label, transition.to.value())),
            );
        }

        // A summand in which an LTS cannot move has no parallel transitions, see [`ParallelTransitions::new`].
        0..self.summands.len()
    }

    fn state_info(&self, _state: &[usize], _context: &ComposedContext<'a, L>) {}
}

/// The summand of a [`ComposedLts`] in which the LTSs in `components` take a transition simultaneously.
struct ComposedSummand<'a, L> {
    /// The indices of the LTSs that move, in increasing order.
    components: Vec<usize>,
    lts: PhantomData<&'a [L]>,
}

/// Returns every non-empty subset of `0..n` as a list in increasing order. The subsets are ordered like the binary
/// numbers 1 to 2^n - 1, in which element 0 is the least significant bit.
fn non_empty_subsets(n: usize) -> Vec<Vec<usize>> {
    let mut odometer = Odometer::new();
    odometer.reset(std::iter::repeat_n(2, n));

    let mut subsets = Vec::new();
    while odometer.increment().is_some() {
        // The last digit changes fastest, so it corresponds to element 0.
        subsets.push(odometer.digits().iter().rev().positions(|&digit| digit == 1).collect());
    }
    subsets
}

impl<'a, L: LTS<Label = LtsMultiAction<A>>, A: CombineLabel> Summand for ComposedSummand<'a, L> {
    type Value = usize;
    type Label = LtsMultiAction<A>;
    type Context = ComposedContext<'a, L>;

    fn enumerate<F>(&self, context: &mut Self::Context, state: &[usize], mut report: F) -> Result<(), MercError>
    where
        F: FnMut(&Self::Label, &[usize]) -> Result<(), MercError>,
    {
        let ComposedContext {
            labels,
            choices,
            odometer,
            transition,
        } = context;

        let mut transitions = ParallelTransitions::new(&self.components, choices, state, odometer, transition);
        while let Some(transition) = transitions.next() {
            if let Some(label) = labels.label(&transition.labels) {
                report(label, &transition.target)?;
            }
        }
        Ok(())
    }

    fn read_positions(&self) -> &[usize] {
        &self.components
    }

    fn effect(&self) -> StateEffect<'_> {
        StateEffect::Positions(&self.components)
    }
}

/// The per-thread enumeration context of a [`ComposedLts`].
struct ComposedContext<'a, L: LTS> {
    labels: CombinedLabels<'a, L>,

    /// The usable outgoing (label, target) pairs of every LTS in the current state, filled by [`LPS::prepare`].
    choices: Vec<Vec<(LabelIndex, usize)>>,

    /// The buffers of [`ParallelTransitions`], kept to avoid allocating them for every summand.
    odometer: Odometer,
    transition: ParallelTransition,
}

/// A transition of a parallel composition before its label is computed.
#[derive(Default)]
struct ParallelTransition {
    /// The (LTS index, label index) pair of every LTS that moves, in increasing order of LTS index.
    labels: Vec<(usize, LabelIndex)>,
    /// The state of every LTS after the transition.
    target: Vec<usize>,
}

/// The parallel transitions in which every LTS in `components` takes one of its `choices`, and the other LTSs stay in
/// their state.
///
/// This is a [`StreamingIterator`] that updates a single [`ParallelTransition`] in place, changing only the LTSs whose
/// choice changed.
struct ParallelTransitions<'c> {
    components: &'c [usize],
    choices: &'c [Vec<(LabelIndex, usize)>],
    /// The current choice of every LTS in `components`.
    odometer: &'c mut Odometer,
    transition: &'c mut ParallelTransition,
    position: Position,
}

#[derive(Clone, Copy)]
enum Position {
    Start,
    At,
    Done,
}

impl<'c> ParallelTransitions<'c> {
    /// Creates the iterator over the parallel transitions from `state`, where `choices[i]` contains the (label,
    /// target) pairs of the outgoing transitions of LTS `i`. The `odometer` and `transition` are reused buffers.
    ///
    /// There are no parallel transitions when an LTS in `components` has no choices.
    fn new(
        components: &'c [usize],
        choices: &'c [Vec<(LabelIndex, usize)>],
        state: &[usize],
        odometer: &'c mut Odometer,
        transition: &'c mut ParallelTransition,
    ) -> Self {
        let position = if odometer.reset(components.iter().map(|&i| choices[i].len())) {
            transition.labels.clear();
            transition
                .labels
                .extend(components.iter().map(|&i| (i, LabelIndex::new(0))));
            transition.target.clear();
            transition.target.extend_from_slice(state);
            Position::Start
        } else {
            Position::Done
        };

        Self {
            components,
            choices,
            odometer,
            transition,
            position,
        }
    }

    /// Updates the transition for the components from position `first` onwards to their current choice.
    fn update_from(&mut self, first: usize) {
        for (k, &i) in self.components.iter().enumerate().skip(first) {
            let (label, to) = self.choices[i][self.odometer.digits()[k]];
            self.transition.labels[k] = (i, label);
            self.transition.target[i] = to;
        }
    }
}

impl StreamingIterator for ParallelTransitions<'_> {
    type Item = ParallelTransition;

    fn advance(&mut self) {
        let changed = match self.position {
            Position::Start => Some(0),
            Position::At => self.odometer.increment(),
            Position::Done => None,
        };
        self.position = match changed {
            Some(first) => {
                self.update_from(first);
                Position::At
            }
            None => Position::Done,
        };
    }

    fn get(&self) -> Option<&ParallelTransition> {
        matches!(self.position, Position::At).then_some(&*self.transition)
    }
}

/// Returns the action names that can occur in a multi-action allowed by `allow` after applying `comm`.
///
/// These are the names in `allow`, and the names on the left-hand side of a communication whose result is such a name.
/// A multi-action containing any other name is not allowed, whatever it is combined with: communication only replaces
/// names on its left-hand side by its result.
fn usable_action_names(allow: &[MultiActionLabel], comm: &[CommExpr]) -> Vec<String> {
    let mut names: Vec<String> = allow
        .iter()
        .flat_map(|label| label.actions.iter().map(|name| name.node.clone()))
        .collect();
    // The result of a communication cannot occur on the left-hand side of another, so one pass suffices.
    for expr in comm {
        if names.contains(&expr.to.node) {
            names.extend(expr.from.actions.iter().map(|name| name.node.clone()));
        }
    }
    names.sort_unstable();
    names.dedup();
    names
}

/// Computes the labels of parallel transitions, i.e. $\tau_I(\gamma_C(\alpha_{j_0} | \cdots | \alpha_{j_m}))$ when
/// it is allowed by $A$, where LTS $j_k$ takes a transition labelled $\alpha_{j_k}$.
///
/// This label only depends on the label indices taken by the participating LTSs, so it is computed once for every
/// combination of (LTS, label index) pairs. A combination is only stored when it occurs in a parallel transition, so
/// there are at most as many entries as generated parallel transitions, and in practice far fewer.
struct CombinedLabels<'a, L: LTS> {
    lts_list: &'a [L],
    hide: &'a [String],
    allow: Vec<SortedMultiActionLabel>,
    comm: &'a [CommExpr],

    /// The label for every combination of (LTS index, label index) pairs, or `None` if it is not allowed.
    cache: HashMap<Vec<(usize, LabelIndex)>, Option<L::Label>, FxBuildHasher>,
}

impl<'a, L: LTS<Label = LtsMultiAction<A>>, A: CombineLabel> CombinedLabels<'a, L> {
    fn new(lts_list: &'a [L], hide: &'a [String], allow: &[MultiActionLabel], comm: &'a [CommExpr]) -> Self {
        Self {
            lts_list,
            hide,
            // Pre-sort the allow set for efficient matching.
            allow: allow.iter().map(SortedMultiActionLabel::new).collect(),
            comm,
            cache: HashMap::default(),
        }
    }

    /// Returns the label of the parallel transition in which the LTSs take the labels of the given (LTS index, label
    /// index) pairs, in increasing order of LTS index, or `None` if it is not allowed.
    fn label(&mut self, key: &[(usize, LabelIndex)]) -> Option<&LtsMultiAction<A>> {
        let Self {
            lts_list,
            hide,
            allow,
            comm,
            cache,
        } = self;
        cache
            .entry_ref(key)
            .or_insert_with(|| combined_label(lts_list, key, hide, allow, comm))
            .as_ref()
    }

    /// Returns the number of distinct combinations of labels seen so far.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.cache.len()
    }
}

/// Returns $\tau_I(\gamma_C(\alpha))$, where $\alpha$ is the multi-action of the given (LTS index, label index) pairs,
/// or `None` if $\gamma_C(\alpha)$ is not allowed.
fn combined_label<L: LTS<Label = LtsMultiAction<A>>, A: CombineLabel>(
    lts_list: &[L],
    labels: &[(usize, LabelIndex)],
    hide: &[String],
    allow: &[SortedMultiActionLabel],
    comm: &[CommExpr],
) -> Option<LtsMultiAction<A>> {
    // Apply communication to alpha = alpha_{j_0} | ... | alpha_{j_m}.
    let multi_action = communicate(
        comm,
        labels
            .iter()
            .flat_map(|&(lts_idx, label_idx)| lts_list[lts_idx].labels()[label_idx].actions())
            .cloned()
            .collect(),
    );

    // Check allow: alpha in A ∪ {tau}.
    if !is_allowed(allow, &SortedLtsMultiAction::new(&multi_action)) {
        return None;
    }

    // Apply hide: alpha = tau_I(alpha).
    Some(hide_action(hide, multi_action))
}

/// Applies the communication operator $\gamma_C$ to the multi-action consisting of `actions`.
///
/// For each communication expression $a_1 | \cdots | a_n \rightarrow c$ in $C$,
/// repeatedly finds matching sub-multisets of actions (with equal arguments)
/// and replaces them with the result action $c$.
fn communicate<L: CombineLabel>(comm: &[CommExpr], mut actions: Vec<L>) -> LtsMultiAction<L> {
    for expr in comm {
        // Try to find a matching sub-multiset for this communication expression.
        while let Some(replacement) = find_communication_match(&actions, expr) {
            actions = replacement;
        }
    }

    LtsMultiAction::new(VecBag::from_vec(actions))
}

/// Tries to find actions matching a single communication expression
/// $a_1 | \cdots | a_n \rightarrow c$ within the given action list.
///
/// Returns the resulting action list with the matched actions replaced by the
/// communicated action, or `None` if no match was found.
fn find_communication_match<L: CombineLabel>(actions: &[L], expr: &CommExpr) -> Option<Vec<L>> {
    // For each action name in the communication expression's left-hand side,
    // find a matching action with the same label. All matched actions must
    // have the same arguments.
    //
    // We iterate over every candidate for the *first* required name so that we
    // do not greedily commit to a particular argument set.  Once the first
    // action is chosen its arguments fix the constraint for all remaining
    // names, so a single greedy pass over the rest is correct.
    let mut matched = vec![false; actions.len()];
    let first_name = expr.from.actions.first()?;

    'outer: for first_idx in 0..actions.len() {
        if !actions[first_idx].matches_label(first_name) {
            continue;
        }

        // Attempt to complete the match using actions[first_idx] as the
        // argument reference.
        matched.fill(false);
        matched[first_idx] = true;

        for required_name in expr.from.actions.iter().skip(1) {
            let mut found = false;
            for (i, action) in actions.iter().enumerate() {
                if matched[i] {
                    continue;
                }
                if action.matches_label(required_name) && action.comm_args_compatible(&actions[first_idx]) {
                    matched[i] = true;
                    found = true;
                    break;
                }
            }
            if !found {
                continue 'outer;
            }
        }

        // Build the result: remove matched actions and add the communicated action.
        let mut result: Vec<L> = actions
            .iter()
            .enumerate()
            .filter(|(i, _)| !matched[*i])
            .map(|(_, a)| a.clone())
            .collect();

        result.push(L::from_comm(expr.to.node.clone(), &actions[first_idx]));
        return Some(result);
    }

    None
}

/// Returns true iff the given multi-action is allowed by the allow operator.
///
/// A multi-action is allowed if:
/// - The action is tau (always allowed), or
/// - The action label names, compared as sorted multisets, match one of the
///   entries in the allow set.
fn is_allowed<L: CombineLabel>(allow: &[SortedMultiActionLabel], action: &SortedLtsMultiAction<L>) -> bool {
    if action.is_tau_label() {
        return true;
    }

    allow.iter().any(|allowed| {
        allowed.actions.len() == action.len()
            && allowed
                .actions
                .iter()
                .zip(action.iter_actions())
                .all(|(name, a)| a.matches_label(name))
    })
}

/// Applies the hide operator $\tau_I$ to a multi-action.
///
/// Removes all actions whose label is in the hide set $I$. If all actions are
/// hidden the result is the tau action (empty multi-action).
fn hide_action<L: CombineLabel>(hide: &[String], mut action: LtsMultiAction<L>) -> LtsMultiAction<L> {
    if !hide.is_empty() {
        action.retain(|a| !hide.iter().any(|h| a.matches_label(h)));
    }
    action
}

/// A multi-action label with action names sorted, for proper multiset
/// comparison with [`SortedLtsMultiAction`].
struct SortedMultiActionLabel {
    /// Action names in sorted order.
    actions: Vec<String>,
}

impl SortedMultiActionLabel {
    fn new(label: &MultiActionLabel) -> Self {
        let mut actions: Vec<String> = label.actions.iter().map(|name| name.node.clone()).collect();
        actions.sort();
        SortedMultiActionLabel { actions }
    }
}

/// A view of an [`LtsMultiAction`] for multiset comparison with
/// [`SortedMultiActionLabel`].
///
/// `VecBag<L>` iterates in `L`'s sorted order, this only works if `L`'s `Ord`
/// implementation is consistent with the label-matching logic in `is_allowed`
/// and the communication matching in `find_communication_match`.
struct SortedLtsMultiAction<'a, L: Ord> {
    action: &'a LtsMultiAction<L>,
}

impl<'a, L: TransitionLabel> SortedLtsMultiAction<'a, L> {
    fn new(action: &'a LtsMultiAction<L>) -> Self {
        // Check if the L Ord is consistent.
        debug_assert!(
            action
                .actions()
                .iter()
                .zip(action.actions().iter().sorted())
                .all(|(a, b)| a == b),
            "LtsMultiAction's internal order must be consistent with the label-matching logic"
        );

        SortedLtsMultiAction { action }
    }

    fn is_tau_label(&self) -> bool {
        self.action.is_tau_label()
    }

    fn iter_actions(&self) -> impl Iterator<Item = &L> {
        self.action.actions().iter()
    }

    fn len(&self) -> usize {
        self.action.actions().len()
    }
}

#[cfg(test)]
mod tests {
    use merc_collections::VecBag;
    use merc_data::DataExpression;
    use merc_lts::LtsAction;
    use merc_lts::LtsMultiAction;
    use merc_lts::SimpleAction;
    use merc_syntax::CommExpr;
    use merc_syntax::MultiActionLabel;
    use merc_syntax::Span;
    use merc_syntax::respan;

    use itertools::Itertools;
    use streaming_iterator::StreamingIterator;

    use merc_lts::LTS;
    use merc_lts::LabelIndex;
    use merc_lts::LabelledTransitionSystem;
    use merc_lts::StateIndex;
    use merc_lts::TransitionLabel;
    use merc_lts::random_lts;
    use merc_syntax::parse_action_names;
    use merc_syntax::parse_allow_action_names;
    use merc_syntax::parse_comm_expr_set;
    use merc_utilities::random_test;

    use super::ActionArguments;
    use super::CombineLabel;
    use super::CombinedLabels;
    use super::ComposedLts;
    use super::InternedAction;
    use super::Odometer;
    use super::ParallelTransition;
    use super::ParallelTransitions;
    use super::SortedLtsMultiAction;
    use super::SortedMultiActionLabel;
    use super::combined_label;
    use super::find_communication_match;
    use super::is_allowed;
    use super::non_empty_subsets;
    use crate::LPS;
    use crate::Summand;

    type TestLts = LabelledTransitionSystem<LtsMultiAction<SimpleAction>>;

    /// An LTS with the given labels after tau (label index 0), in which the transitions are (from, label index, to).
    fn make_lts(labels: &[&str], num_of_states: usize, transitions: &[(usize, usize, usize)]) -> TestLts {
        let labels = std::iter::once(LtsMultiAction::tau_label())
            .chain(labels.iter().map(|label| LtsMultiAction::from_string(label).unwrap()))
            .collect();
        LabelledTransitionSystem::new(
            StateIndex::new(0),
            Some(num_of_states),
            || {
                transitions
                    .iter()
                    .map(|&(from, label, to)| (StateIndex::new(from), LabelIndex::new(label), StateIndex::new(to)))
            },
            labels,
        )
    }

    /// A random LTS with multi-action labels a, b, c, ... and tau.
    fn random_test_lts(rng: &mut impl rand::Rng, num_of_states: usize, num_of_labels: u32) -> TestLts {
        random_lts::<String, _>(rng, num_of_states, num_of_labels)
            .relabel(|label| LtsMultiAction::from_string(&label))
            .unwrap()
    }

    /// The (LTS index, label index) pairs as a key of [`CombinedLabels`].
    fn key(pairs: &[(usize, usize)]) -> Vec<(usize, LabelIndex)> {
        pairs
            .iter()
            .map(|&(lts, label)| (lts, LabelIndex::new(label)))
            .collect()
    }

    /// Returns the transitions of `lps` from `state` as (label, target) pairs.
    fn enumerate_all(
        lps: &ComposedLts<'_, TestLts>,
        state: &[usize],
    ) -> Vec<(LtsMultiAction<SimpleAction>, Vec<usize>)> {
        let mut context = lps.create_context();
        let summands: Vec<usize> = lps.prepare(&mut context, state).collect();
        let mut result = Vec::new();
        for index in summands {
            lps.summands()[index]
                .enumerate(&mut context, state, |label, target| {
                    result.push((label.clone(), target.to_vec()));
                    Ok(())
                })
                .unwrap();
        }
        result
    }

    fn simple(label: &str, args: &[&str]) -> SimpleAction {
        SimpleAction::new(label.to_owned(), args.iter().map(|s| s.to_string()).collect())
    }

    fn comm(from: &[&str], to: &str) -> CommExpr {
        CommExpr::new(
            MultiActionLabel::new(from.iter().map(|s| respan(Span::default(), s.to_string())).collect()),
            respan(Span::default(), to.to_owned()),
        )
    }

    /// Builds the allow set from label-name multisets.
    fn allow(entries: &[&[&str]]) -> Vec<SortedMultiActionLabel> {
        entries
            .iter()
            .map(|names| {
                SortedMultiActionLabel::new(&MultiActionLabel::new(
                    names.iter().map(|s| respan(Span::default(), s.to_string())).collect(),
                ))
            })
            .collect()
    }

    /// Builds a multi-action from actions; `VecBag` orders them internally.
    fn multi_action(actions: Vec<SimpleAction>) -> LtsMultiAction<SimpleAction> {
        LtsMultiAction::new(VecBag::from_vec(actions))
    }

    fn is_allowed_by(entries: &[&[&str]], actions: Vec<SimpleAction>) -> bool {
        let action = multi_action(actions);
        is_allowed(&allow(entries), &SortedLtsMultiAction::new(&action))
    }

    /// The allow operator matches on label-name multisets only, independent of
    /// arguments. This locks in the name-primary ordering that [`is_allowed`]
    /// relies on: here `a`'s argument sorts *after* `b`'s, so an argument-first
    /// ordering of the bag would misalign the zip and wrongly reject `{a, b}`.
    #[test]
    fn allow_matches_multiset_ignoring_arguments() {
        // Arguments chosen so an argument-first sort would reorder the names.
        assert!(is_allowed_by(
            &[&["a", "b"]],
            vec![simple("a", &["9"]), simple("b", &["1"])]
        ));

        // Repeated names compare as a multiset, not a set.
        assert!(is_allowed_by(
            &[&["a", "a", "b"]],
            vec![simple("a", &["1"]), simple("a", &["2"]), simple("b", &[])]
        ));
        assert!(!is_allowed_by(
            &[&["a", "b", "b"]],
            vec![simple("a", &["1"]), simple("a", &["2"]), simple("b", &[])]
        ));

        // A differing length or name is rejected; tau is always allowed.
        assert!(!is_allowed_by(
            &[&["a", "b"]],
            vec![simple("a", &[]), simple("b", &[]), simple("b", &[])]
        ));
        assert!(!is_allowed_by(&[&["a", "c"]], vec![simple("a", &[]), simple("b", &[])]));
        assert!(is_allowed_by(&[&["a", "b"]], vec![]));
    }

    /// Regression: when the first candidate for the first required name has
    /// incompatible args, the function must try subsequent candidates.
    ///
    /// Multi-action: a(1) | a(2) | b(2)
    /// Comm: a | b -> c
    ///
    /// Greedy first-pick would choose a(1), then fail to match b because
    /// b(2) has different args.  The correct match is a(2) | b(2) -> c(2).
    #[test]
    fn first_candidate_incompatible_retries() {
        let actions = vec![simple("a", &["1"]), simple("a", &["2"]), simple("b", &["2"])];
        let expr = comm(&["a", "b"], "c");

        let result = find_communication_match(&actions, &expr);
        assert!(result.is_some(), "should find a(2)|b(2) -> c(2)");

        let result = result.unwrap();
        // Remaining multi-action: a(1) and c(2)  (a(2) and b(2) consumed)
        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|a| a == &simple("a", &["1"])));
        assert!(result.iter().any(|a| a == &simple("c", &["2"])));
    }

    /// When all candidates are genuinely incompatible, the function should
    /// still return `None`.
    #[test]
    fn no_compatible_pair_returns_none() {
        let actions = vec![simple("a", &["1"]), simple("b", &["2"])];
        let expr = comm(&["a", "b"], "c");

        assert!(find_communication_match(&actions, &expr).is_none());
    }

    /// Basic case with no data args still works.
    #[test]
    fn no_args_basic_comm() {
        let actions = vec![simple("a", &[]), simple("b", &[])];
        let expr = comm(&["a", "b"], "c");

        let result = find_communication_match(&actions, &expr).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], simple("c", &[]));
    }

    /// Only the given components move, the last one fastest, and the other LTSs stay in their state.
    #[test]
    fn parallel_transitions_enumerates_choices_of_components() {
        let choices = vec![
            vec![(LabelIndex::new(1), 10), (LabelIndex::new(2), 11)],
            vec![(LabelIndex::new(3), 12)],
            vec![(LabelIndex::new(4), 13), (LabelIndex::new(5), 14)],
        ];
        let (mut odometer, mut transition) = (Odometer::new(), ParallelTransition::default());

        let mut transitions = ParallelTransitions::new(&[0, 2], &choices, &[0, 1, 2], &mut odometer, &mut transition);
        let mut result = Vec::new();
        while let Some(transition) = transitions.next() {
            result.push((transition.labels.clone(), transition.target.clone()));
        }
        assert_eq!(
            result,
            vec![
                (key(&[(0, 1), (2, 4)]), vec![10, 1, 13]),
                (key(&[(0, 1), (2, 5)]), vec![10, 1, 14]),
                (key(&[(0, 2), (2, 4)]), vec![11, 1, 13]),
                (key(&[(0, 2), (2, 5)]), vec![11, 1, 14]),
            ]
        );

        // An LTS without choices has no parallel transitions.
        let choices = vec![vec![(LabelIndex::new(1), 10)], vec![]];
        let mut transitions = ParallelTransitions::new(&[0, 1], &choices, &[0, 1], &mut odometer, &mut transition);
        assert!(transitions.next().is_none());
    }

    #[test]
    fn non_empty_subsets_counts_in_binary() {
        assert_eq!(non_empty_subsets(0), Vec::<Vec<usize>>::new());
        assert_eq!(
            non_empty_subsets(3),
            vec![
                vec![0],
                vec![1],
                vec![0, 1],
                vec![2],
                vec![0, 2],
                vec![1, 2],
                vec![0, 1, 2]
            ]
        );
    }

    /// Interning keeps exactly the actions with equal arguments compatible for communication, and restoring returns
    /// the original actions.
    #[test]
    fn action_arguments_intern_and_restore() {
        let single = |label: &str, argument: &str| {
            LtsMultiAction::new(VecBag::from_vec(vec![LtsAction::new(
                label.to_owned(),
                vec![DataExpression::from_string(argument).unwrap()],
            )]))
        };
        let (a_one, b_one, b_two) = (single("a", "1"), single("b", "1"), single("b", "f(2)"));

        let mut arguments = ActionArguments::default();
        let interned: Vec<LtsMultiAction<InternedAction>> = [&a_one, &b_one, &b_two]
            .iter()
            .map(|action| arguments.intern(action))
            .collect();
        let action = |i: usize| interned[i].actions().iter().next().unwrap();

        assert!(action(0).comm_args_compatible(action(1)));
        assert!(!action(1).comm_args_compatible(action(2)));
        assert!(action(0).matches_label("a") && action(2).matches_label("b"));
        for (original, interned) in [&a_one, &b_one, &b_two].into_iter().zip(&interned) {
            assert_eq!(&arguments.restore(interned), original);
        }
    }

    /// Equal label indices in different LTSs must not share a cache entry.
    #[test]
    fn combined_labels_distinguishes_ltss() {
        let ltss = vec![make_lts(&["a"], 2, &[(0, 1, 1)]), make_lts(&["b"], 2, &[(0, 1, 1)])];
        let allow = parse_allow_action_names("{a, a|b}").unwrap();
        let mut labels = CombinedLabels::new(&ltss, &[], &allow, &[]);

        assert_eq!(
            labels.label(&key(&[(0, 1)])),
            Some(&LtsMultiAction::from_string("a").unwrap())
        );
        assert_eq!(labels.label(&key(&[(1, 1)])), None);
        assert_eq!(
            labels.label(&key(&[(0, 1), (1, 1)])),
            Some(&LtsMultiAction::from_string("a|b").unwrap())
        );
        assert_eq!(labels.len(), 3);
    }

    /// Labels that are not allowed are cached as well, and a combination is only computed once.
    #[test]
    fn combined_labels_caches_blocked_combinations() {
        let ltss = vec![make_lts(&["a", "b"], 1, &[]), make_lts(&["b"], 1, &[])];
        let comm = parse_comm_expr_set("{a|b -> c}").unwrap();
        let allow = parse_allow_action_names("{c}").unwrap();
        let hide = parse_action_names("{c}").unwrap();
        let mut labels = CombinedLabels::new(&ltss, &hide, &allow, &comm);

        for _ in 0..2 {
            // b|b is not allowed, a|b communicates to c, which is allowed and hidden.
            assert_eq!(labels.label(&key(&[(0, 2), (1, 1)])), None);
            assert_eq!(
                labels.label(&key(&[(0, 1), (1, 1)])),
                Some(&LtsMultiAction::tau_label())
            );
        }
        assert_eq!(labels.len(), 2);
    }

    /// The transitions of the composition are exactly the combinations of outgoing transitions, in which every LTS
    /// either stays or takes a transition, whose label computed without the cache is allowed.
    #[test]
    fn random_composition_matches_naive_enumeration() {
        random_test(100, |rng| {
            let ltss = vec![
                random_test_lts(rng, 5, 5),
                random_test_lts(rng, 5, 5),
                random_test_lts(rng, 5, 5),
            ];
            let comm = parse_comm_expr_set("{a|b -> c}").unwrap();
            // b is only usable through the communication, and d is not usable.
            let allow = parse_allow_action_names("{a, c, a|c}").unwrap();
            let hide = parse_action_names("{a}").unwrap();
            let sorted_allow: Vec<SortedMultiActionLabel> = allow.iter().map(SortedMultiActionLabel::new).collect();
            let lps = ComposedLts::new(&ltss, &hide, &allow, &comm);

            for state in ltss.iter().map(|lts| 0..lts.num_of_states()).multi_cartesian_product() {
                // Every LTS either stays (None) or takes one of its outgoing transitions.
                let mut expected = Vec::new();
                let choices: Vec<Vec<Option<(LabelIndex, usize)>>> = ltss
                    .iter()
                    .zip(&state)
                    .map(|(lts, &s)| {
                        std::iter::once(None)
                            .chain(
                                lts.outgoing_transitions(StateIndex::new(s))
                                    .map(|t| Some((t.label, t.to.value()))),
                            )
                            .collect()
                    })
                    .collect();
                for combination in choices
                    .iter()
                    .map(|choice| choice.iter().copied())
                    .multi_cartesian_product()
                {
                    let pairs: Vec<(usize, LabelIndex)> = combination
                        .iter()
                        .enumerate()
                        .filter_map(|(i, choice)| choice.map(|(label, _)| (i, label)))
                        .collect();
                    if pairs.is_empty() {
                        continue;
                    }
                    let target = (0..ltss.len())
                        .map(|i| combination[i].map_or(state[i], |(_, to)| to))
                        .collect();
                    if let Some(label) = combined_label(&ltss, &pairs, &hide, &sorted_allow, &comm) {
                        expected.push((label, target));
                    }
                }

                let mut result = enumerate_all(&lps, &state);
                result.sort();
                expected.sort();
                assert_eq!(result, expected);
            }
        });
    }
}
