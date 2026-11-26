use std::borrow::Borrow;
use std::cell::Cell;
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::ops::ControlFlow;
use std::rc::Rc;
use std::sync::Arc;

use itertools::Itertools;
use log::debug;
use log::info;

use merc_aterm::ATerm;
use merc_aterm::ATermList;
use merc_aterm::ATermRead;
use merc_aterm::ATermSend;
use merc_aterm::ATermStreamable;
use merc_aterm::ATermWrite;
use merc_aterm::Symb;
use merc_aterm::Term;
use merc_aterm::structural_cmp;
use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_data::Mcrl2DataSpecification;
use merc_data::is_bool_literal;
use merc_data::is_undefined_real;
use merc_data::mark_occurrences;
use merc_data::undefined_real;
use merc_enumerate::EnumerationPlans;
use merc_enumerate::Enumerator;
use merc_enumerate::EnumeratorFrame;
use merc_enumerate::FreshVariableGenerator;
use merc_enumerate::Outcome;
use merc_explore::CacheLPS;
use merc_explore::CachingStrategy;
use merc_explore::ExplorationStrategy;
use merc_explore::LPS;
use merc_explore::StateEffect;
use merc_explore::Summand;
use merc_explore::configure_rayon_thread_pool;
use merc_explore::explore;
use merc_explore::explore_parallel;
use merc_io::TimeProgress;
use merc_lowering::Pretty;
use merc_lts::ConcurrentLtsBuilder;
use merc_lts::LtsAction;
use merc_lts::LtsBuilder;
use merc_lts::LtsMultiAction;
use merc_lts::MCRL2Action;
use merc_lts::MCRL2ActionLabel;
use merc_lts::MCRL2TimedMultiAction;
use merc_lts::PerStateDedup;
use merc_lts::StateIndex;
use merc_lts::TransitionLabel;
use merc_sabre::DataRewriter;
use merc_sabre::RewriteEngine;
use merc_sabre::RewriteSpecification;
use merc_sabre::Rewriter;
use merc_sabre::utilities::SliceSubstitution;
use merc_unsafety::ConcurrentIndexedSet;
use merc_utilities::MercError;
use merc_utilities::ShardedCounter;
use merc_utilities::Timing;

use crate::ActionSummand;
use crate::LinearProcessSpecification;
use crate::cfg_lps::CfgLinearProcessSpecification;
use crate::io_lps::action_label_term;
use crate::transformations::LpsTransformation;
use crate::transformations::TransformedLps;

/// Periodic progress reporter for LPS exploration, printing the number of
/// discovered states and transitions.
pub fn lps_progress() -> TimeProgress<(usize, usize)> {
    TimeProgress::new(
        |(states, transitions): (usize, usize)| {
            info!("Explored {states} states, {transitions} transitions...");
        },
        1,
    )
}

/// Explore the linear process specification explicitly, forwarding the
/// discovered transitions to `builder`.
///
/// The LPS is explored as given, any preprocessing must be applied beforehand.
///
/// When `control_flow` is set, a [`ControlFlowGraph`](crate::ControlFlowGraph)
/// is layered on top of the explicit LPS to prune summands whose control flow
/// guard cannot hold in the current state (see [`CfgLinearProcessSpecification`]).
/// The pruning never changes the explored transition system.
pub fn explore_lps_explicit<B>(
    builder: &mut B,
    lps: TransformedLps,
    caching: CachingStrategy,
    strategy: ExplorationStrategy,
    control_flow: bool,
    timing: &Timing,
) -> Result<B::LTS, MercError>
where
    B: LtsBuilder<LpsMultiActionLabel>,
{
    if control_flow {
        let lps = CfgLinearProcessSpecification::new(lps)?;
        info!(
            "Control flow analysis identified {} control flow parameter(s)",
            lps.control_flow_parameters().len()
        );
        let result = explore_lps_explicit_impl(builder, &lps, caching, strategy, timing);
        debug!("{}", lps.metrics());
        result
    } else {
        let lps = ExplicitLinearProcessSpecification::new(lps)?;
        debug!("{lps:?}");
        explore_lps_explicit_impl(builder, &lps, caching, strategy, timing)
    }
}

/// Shared exploration driver over any explicit-state [`LPS`] view producing
/// multi-action labels, used by both the plain explicit explorer and the
/// control-flow-pruning variant.
fn explore_lps_explicit_impl<B, L>(
    builder: &mut B,
    lps: &L,
    caching: CachingStrategy,
    strategy: ExplorationStrategy,
    timing: &Timing,
) -> Result<B::LTS, MercError>
where
    B: LtsBuilder<LpsMultiActionLabel>,
    L: LPS<Value = usize, Label = LpsMultiActionLabel, StateInfo = (), Summand = ExplicitSummand>,
{
    // Only layer the enumeration cache on top of the LPS when a caching strategy
    // is actually requested; otherwise explore the bare LPS directly.
    match caching {
        CachingStrategy::None => run_explore_explicit(builder, lps, strategy, timing),
        _ => {
            let cached = CacheLPS::new(lps, caching);
            let result = run_explore_explicit(builder, &cached, strategy, timing)?;
            debug!("{}", cached.metrics());
            Ok(result)
        }
    }
}

/// Runs the sequential explicit exploration loop over any [`LPS`] view producing
/// multi-action labels, counting states and transitions and finalising the
/// builder. The view is either the bare LPS or one wrapped in [`CacheLPS`].
fn run_explore_explicit<B, M>(
    builder: &mut B,
    lps: &M,
    strategy: ExplorationStrategy,
    timing: &Timing,
) -> Result<B::LTS, MercError>
where
    B: LtsBuilder<LpsMultiActionLabel>,
    M: LPS<Value = usize, Label = LpsMultiActionLabel, StateInfo = ()>,
{
    // Count states and transitions in the exploration closures, driving the periodic progress
    // reporter from `on_transition`.
    let progress = lps_progress();
    let states = Cell::new(0usize);
    let transitions = Cell::new(0usize);

    // Different summands can instantiate to the same (label, successor) pair
    // for a given state, so deduplicate them.
    let mut dedup = PerStateDedup::new();

    let initial = explore(
        lps,
        strategy,
        timing,
        builder,
        |_b: &mut B, _state, _info: &()| {
            states.set(states.get() + 1);
            Ok(())
        },
        |b: &mut B, from, label: &LpsMultiActionLabel, to| {
            dedup.add(from, label, to, |from, label, to| {
                transitions.set(transitions.get() + 1);
                b.add_transition(from, label, to)
            })?;
            progress.print((states.get(), transitions.get() + dedup.len()));
            Ok(())
        },
    )?;

    dedup.flush(|from, label, to| {
        transitions.set(transitions.get() + 1);
        builder.add_transition(from, label, to)
    })?;

    info!(
        "Exploration complete: {} states, {} transitions",
        states.get(),
        transitions.get(),
    );
    builder.require_num_of_states(states.get());

    builder.finish(initial)
}

/// Explores the linear process specification explicitly in parallel across
/// `threads` worker threads, streaming the discovered transitions into
/// `builder`.
///
/// As for [`explore_lps_explicit`], the LPS is explored as given: preprocessing
/// it is the caller's responsibility.
pub fn explore_lps_explicit_parallel<B>(
    builder: &mut B,
    lps: TransformedLps,
    caching: CachingStrategy,
    threads: usize,
    control_flow: bool,
    pinned: bool,
    timing: &Timing,
) -> Result<B::LTS, MercError>
where
    B: ConcurrentLtsBuilder<LpsMultiActionLabel>,
{
    if control_flow {
        let lps = CfgLinearProcessSpecification::new(lps)?;
        info!(
            "Control flow analysis identified {} control flow parameter(s)",
            lps.control_flow_parameters().len()
        );
        let result = explore_lps_explicit_parallel_impl(builder, &lps, caching, threads, pinned, timing);
        debug!("{}", lps.metrics());
        result
    } else {
        let lps = ExplicitLinearProcessSpecification::new(lps)?;
        explore_lps_explicit_parallel_impl(builder, &lps, caching, threads, pinned, timing)
    }
}

/// Shared parallel exploration driver over any explicit-state [`LPS`] view,
/// used by both the plain explicit explorer and the control-flow-pruning
/// variant.
fn explore_lps_explicit_parallel_impl<B, L>(
    builder: &mut B,
    lps: &L,
    caching: CachingStrategy,
    threads: usize,
    pinned: bool,
    timing: &Timing,
) -> Result<B::LTS, MercError>
where
    B: ConcurrentLtsBuilder<LpsMultiActionLabel>,
    L: LPS<Value = usize, Label = LpsMultiActionLabel, StateInfo = (), Summand = ExplicitSummand> + Sync,
{
    let pool = configure_rayon_thread_pool(threads, pinned)?;

    // Only layer the enumeration cache on top of the LPS when a caching strategy
    // is actually requested; otherwise explore the bare LPS directly.
    match caching {
        CachingStrategy::None => run_explore_explicit_parallel(builder, lps, &pool, timing),
        _ => {
            let cached = CacheLPS::new(lps, caching);
            let result = run_explore_explicit_parallel(builder, &cached, &pool, timing)?;
            debug!("{}", cached.metrics());
            Ok(result)
        }
    }
}

/// Runs the parallel explicit exploration loop over any [`LPS`] view producing
/// multi-action labels, streaming the discovered transitions into the
/// concurrent builder. The view is either the bare LPS or one wrapped in
/// [`CacheLPS`].
fn run_explore_explicit_parallel<B, M>(
    builder: &mut B,
    lps: &M,
    pool: &rayon::ThreadPool,
    timing: &Timing,
) -> Result<B::LTS, MercError>
where
    B: ConcurrentLtsBuilder<LpsMultiActionLabel>,
    M: LPS<Value = usize, Label = LpsMultiActionLabel, StateInfo = ()> + Sync,
{
    // Shared (immutable) reference to the builder used by the worker threads;
    // `ConcurrentLtsBuilder` requires `Sync`, so the workers can add transitions
    // concurrently while the builder synchronises internally.
    let builder_ref: &B = builder;

    // Shared state/transition counts.
    let states = ShardedCounter::new();
    let transitions = ShardedCounter::new();
    let progress = lps_progress();

    // Forwards a deduplicated transition into the shared builder, counting it only once it is
    // actually written (see the `Local` doc below).
    let flush_one = |from, label: &LpsMultiActionLabel, to| {
        transitions.increment();
        builder_ref.add_transition_shared(from, label, to)
    };

    let initial = timing.measure("explore", || -> Result<_, MercError> {
        pool.install(|| {
            // Each worker gets its own `PerStateDedup`: work is distributed per *state*.
            let (initial, mut locals) = explore_parallel(
                lps,
                PerStateDedup::<LpsMultiActionLabel>::new,
                |_local: &mut PerStateDedup<LpsMultiActionLabel>, _state, _info: &()| {
                    states.increment();
                    Ok(())
                },
                |local: &mut PerStateDedup<LpsMultiActionLabel>, from, label: &LpsMultiActionLabel, to| {
                    if progress.is_due() {
                        progress.print((states.get() as usize, transitions.get() as usize));
                    }
                    local.add(from, label, to, flush_one)
                },
            )?;

            // Each worker's last state is still sitting in its buffer; flush them all now that
            // exploration is complete.
            for local in &mut locals {
                local.flush(flush_one)?;
            }

            Ok(initial)
        })
    })?;

    let total_states = states.get() as usize;
    let total_transitions = transitions.get() as usize;
    info!("Exploration complete: {total_states} states, {total_transitions} transitions");

    // Finalise the builder, recording the total number of states so isolated
    // (deadlock) states are still reflected in the result.
    builder.require_num_of_states(total_states);

    builder.finish(initial)
}

/// The multi-action of a transition, as a `TimedMultAct(actions, time)` term in which the actions
/// are ordered, so that equal multi-actions are the same term.
///
/// Backed by an [ATermSend], so a label can be created on one worker thread and stored in the
/// enumeration cache and the concurrent LTS builders that are shared between threads.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LpsMultiActionLabel {
    term: ATermSend,
}

impl LpsMultiActionLabel {
    /// The multi-action term of this label on the current thread.
    pub fn as_aterm(&self) -> ATerm {
        self.term.protect()
    }

    /// Converts this label into the typed multi-action used by the binary `.lts` format.
    ///
    /// # Errors
    ///
    /// Fails for a timed multi-action, which that format does not support.
    pub fn to_lts_multi_action(&self) -> Result<LtsMultiAction<LtsAction>, MercError> {
        LtsMultiAction::from_mcrl2_aterm(self.as_aterm())
    }
}

/// Adapts an [`LtsBuilder<LtsMultiAction<LtsAction>>`] into one that accepts
/// [`LpsMultiActionLabel`]s, converting each one as it is added.
pub struct LtsMultiActionAdapter<B> {
    inner: B,
}

impl<B> LtsMultiActionAdapter<B> {
    /// Wraps `inner`, a builder over the typed `.lts` multi-action representation.
    pub fn new(inner: B) -> Self {
        Self { inner }
    }
}

impl<B: LtsBuilder<LtsMultiAction<LtsAction>>> LtsBuilder<LpsMultiActionLabel> for LtsMultiActionAdapter<B> {
    type LTS = B::LTS;

    fn add_transition<Q>(&mut self, from: StateIndex, label: &Q, to: StateIndex) -> Result<(), MercError>
    where
        LpsMultiActionLabel: Borrow<Q>,
        Q: ?Sized + ToOwned<Owned = LpsMultiActionLabel> + Eq + Hash,
    {
        let converted = label.to_owned().to_lts_multi_action()?;
        self.inner.add_transition(from, &converted, to)
    }

    fn finish(&mut self, initial_state: StateIndex) -> Result<Self::LTS, MercError> {
        self.inner.finish(initial_state)
    }

    fn num_of_transitions(&self) -> usize {
        self.inner.num_of_transitions()
    }

    fn num_of_states(&self) -> usize {
        self.inner.num_of_states()
    }

    fn require_num_of_states(&mut self, num_states: usize) {
        self.inner.require_num_of_states(num_states)
    }
}

impl<B: ConcurrentLtsBuilder<LtsMultiAction<LtsAction>>> ConcurrentLtsBuilder<LpsMultiActionLabel>
    for LtsMultiActionAdapter<B>
{
    fn add_transition_shared<Q>(&self, from: StateIndex, label: &Q, to: StateIndex) -> Result<(), MercError>
    where
        LpsMultiActionLabel: Borrow<Q>,
        Q: ?Sized + ToOwned<Owned = LpsMultiActionLabel> + Eq + Hash,
    {
        let converted = label.to_owned().to_lts_multi_action()?;
        self.inner.add_transition_shared(from, &converted, to)
    }
}

impl fmt::Display for LpsMultiActionLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let term = self.as_aterm();
        let actions = ATermList::<ATerm>::from(term.arg(0).protect());
        if actions.is_empty() {
            write!(f, "tau")?;
        } else {
            let actions = actions.iter().format_with("|", |action, f| {
                f(&action.arg(0).arg(0).get_head_symbol().name())?;
                let arguments: Vec<DataExpression> = ATermList::<DataExpression>::from(action.arg(1).protect())
                    .iter()
                    .map(|argument| DataExpression::from(argument.protect()))
                    .collect();
                if !arguments.is_empty() {
                    f(&format_args!("({})", arguments.iter().map(Pretty::from).format(", ")))?;
                }
                Ok(())
            });
            write!(f, "{actions}")?;
        }

        let time = DataExpression::from(term.arg(1).protect());
        if !is_undefined_real(&time) {
            write!(f, " @ {}", Pretty::from(&time))?;
        }
        Ok(())
    }
}

impl fmt::Debug for LpsMultiActionLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl TransitionLabel for LpsMultiActionLabel {
    fn tau_label() -> Self {
        LpsMultiActionLabel {
            term: ATermSend::from(multi_action_term(Vec::new(), &undefined_real()).into()),
        }
    }

    fn is_tau_label(&self) -> bool {
        ATermList::<ATerm>::from(self.as_aterm().arg(0).protect()).is_empty()
    }

    fn matches_label(&self, label: &str) -> bool {
        ATermList::<ATerm>::from(self.as_aterm().arg(0).protect())
            .iter()
            .any(|action| action.arg(0).arg(0).get_head_symbol().name() == label)
    }

    fn from_index(_i: usize) -> Self {
        panic!("Cannot generate a random multi-action label for an LPS");
    }
}

/// The term `TimedMultAct(actions, time)`, in which `actions` are put in a canonical order.
fn multi_action_term(mut actions: Vec<MCRL2Action>, time: &DataExpression) -> MCRL2TimedMultiAction {
    actions.sort_by(|left, right| structural_cmp(&left.copy().into(), &right.copy().into()));
    MCRL2TimedMultiAction::new(ATermList::from_double_iter(actions.into_iter()), time.copy())
}

/// The terms of a [TransformedLps] that every worker thread protects for itself, see
/// [ExplicitLinearProcessSpecification].
struct ExplorationInput {
    lps: LinearProcessSpecification,
    /// The data specification extended by the preprocessing, from which the rewriter is built.
    data_specification: Mcrl2DataSpecification,
    /// The constants introduced by the preprocessing, with their values.
    constants: Vec<(DataVariable, DataExpression)>,
}

impl ATermStreamable for ExplorationInput {
    fn write<W: ATermWrite>(&self, writer: &mut W) -> Result<(), MercError> {
        self.lps.write(writer)?;
        self.data_specification.write(writer)?;
        writer.write_aterm_iter(self.constants.iter().map(|(variable, _)| variable.clone().into()))?;
        writer.write_aterm_iter(self.constants.iter().map(|(_, value)| value.clone().into()))
    }

    fn read<R: ATermRead>(reader: &mut R) -> Result<Self, MercError> {
        let lps = LinearProcessSpecification::read(reader)?;
        let data_specification = Mcrl2DataSpecification::read(reader)?;
        let variables = reader
            .read_aterm_iter()?
            .map(|term| Ok(DataVariable::try_from_term(term?)?))
            .collect::<Result<Vec<_>, MercError>>()?;
        let values = reader
            .read_aterm_iter()?
            .map(|term| Ok(DataExpression::try_from_term(term?)?))
            .collect::<Result<Vec<_>, MercError>>()?;
        Ok(ExplorationInput {
            lps,
            data_specification,
            constants: variables.into_iter().zip(values).collect(),
        })
    }
}

/// Interns the data expressions observed as parameter values, shared by all worker threads.
type ValueTable = ConcurrentIndexedSet<ATermSend>;

/// Explicit-state view of a [TransformedLps] that implements the [merc_explore::LPS] trait.
///
/// State vectors are indices into a shared table of the parameter values in normal form. The terms
/// of the LPS are stored as a single [SendStreamable], so that the view can be shared between the
/// worker threads, and every thread reads back its own copy in its [ExplicitContext].
pub struct ExplicitLinearProcessSpecification {
    input: SendStreamable<ExplorationInput>,
    summands: Vec<ExplicitSummand>,
    values: Arc<ValueTable>,
    initial_state: Vec<usize>,
    rewriter: Rewriter,
}

impl ExplicitLinearProcessSpecification {
    /// Prepares `lps` for explicit exploration. Summation variables that shadow a process
    /// parameter are renamed first, see [LpsTransformation::ResolveNameClashes].
    ///
    /// # Errors
    ///
    /// Fails when `lps` has global variables, see
    /// [LpsTransformation::InstantiateGlobalVariables](crate::LpsTransformation::InstantiateGlobalVariables),
    /// since these would otherwise end up in the states as distinct values.
    pub fn new(lps: TransformedLps) -> Result<Self, MercError> {
        if !lps.lps().global_variables.is_empty() {
            return Err(format!(
                "Cannot explore an LPS with global variables ({}), these must be instantiated first",
                lps.lps()
                    .global_variables
                    .iter()
                    .map(|variable| variable.name())
                    .format(", ")
            )
            .into());
        }

        // The summands below assume that no summation variable shadows a parameter.
        let lps = if lps.lps().has_name_clashes() {
            lps.transform(LpsTransformation::ResolveNameClashes)?
        } else {
            lps
        };

        let values = Arc::new(ValueTable::new());
        let parameters = &lps.lps().process_parameters;
        let constants: Vec<DataVariable> = lps
            .constant_assignments()
            .map(|(variable, _)| variable.clone())
            .collect();
        let summands = lps
            .lps()
            .action_summands
            .iter()
            .enumerate()
            .map(|(index, summand)| ExplicitSummand::new(index, summand, parameters, &constants, values.clone()))
            .collect();

        let mut result = ExplicitLinearProcessSpecification {
            input: SendStreamable::new(&ExplorationInput {
                lps: lps.lps().clone(),
                data_specification: lps.data_specification().clone(),
                constants: lps.constant_assignments().cloned().collect(),
            })?,
            summands,
            values,
            initial_state: Vec::new(),
            rewriter: lps.rewriter_kind(),
        };

        let mut context = result.create_context();
        result.initial_state = lps
            .lps()
            .initial_state
            .iter()
            .map(|value| result.intern_normal_form(&mut context, value))
            .collect();
        Ok(result)
    }

    /// The process parameter variables in declaration order.
    pub fn parameters(&self) -> Vec<DataVariable> {
        self.input().lps.process_parameters
    }

    /// The user defined part of the data specification, which is the part an exploration result
    /// stores since the rest can be generated from it.
    pub fn user_data_specification(&self) -> Mcrl2DataSpecification {
        self.input().lps.data_specification
    }

    /// The value interned at `index`, as protected on the current thread.
    pub fn value(&self, index: usize) -> DataExpression {
        self.values
            .get_by_index(index)
            .expect("an interned value must be present")
            .protect()
            .into()
    }

    /// Rewrites `value` to normal form and interns it, returning its index as used in the state
    /// vectors.
    pub fn intern_normal_form(&self, context: &mut ExplicitContext, value: &DataExpression) -> usize {
        let normal_form = context.rewrite_closed(value);
        context.cache.intern(&self.values, &normal_form)
    }

    /// The terms of the LPS, protected on the current thread.
    fn input(&self) -> ExplorationInput {
        self.input
            .local()
            .expect("an exploration input reads back what it writes")
    }
}

/// The per-thread view on the values of the [ValueTable], avoiding the shared table and the
/// protection of a term in the common case.
#[derive(Default)]
struct ValueCache {
    indices: HashMap<DataExpression, usize>,
    values: Vec<Option<DataExpression>>,
}

impl ValueCache {
    fn intern(&mut self, table: &ValueTable, value: &DataExpression) -> usize {
        if let Some(index) = self.indices.get(value) {
            return *index;
        }

        let (index, _) = table.insert(ATermSend::from(value.clone().into()));
        self.indices.insert(value.clone(), index);
        index
    }

    fn get(&mut self, table: &ValueTable, index: usize) -> &DataExpression {
        if index >= self.values.len() {
            self.values.resize_with(index + 1, || None);
        }
        self.values[index].get_or_insert_with(|| {
            table
                .get_by_index(index)
                .expect("an interned value must be present")
                .protect()
                .into()
        })
    }
}

/// The terms of an [ExplicitSummand], protected on the current thread.
struct ThreadSummand {
    condition: DataExpression,
    summation_variables: Vec<DataVariable>,
    /// The domain of the substitution: the summation variables, followed by the parameters that
    /// are read, followed by the constants that are used.
    variables: Vec<DataVariable>,
    /// The image of the substitution, parallel to `variables`.
    values: Vec<DataExpression>,
    /// The new values of the written parameters.
    writes: Vec<DataExpression>,
    actions: Vec<(MCRL2ActionLabel, Vec<DataExpression>)>,
    time: Option<DataExpression>,
}

/// Per-thread enumeration context for an [ExplicitLinearProcessSpecification].
pub struct ExplicitContext {
    rewriter: DataRewriter,
    enumerator: Enumerator,
    frame: EnumeratorFrame,
    summands: Vec<ThreadSummand>,
    constant_variables: Vec<DataVariable>,
    constant_values: Vec<DataExpression>,
    cache: ValueCache,
    labels: HashMap<ATerm, LpsMultiActionLabel>,
    next_state: Vec<usize>,
}

impl ExplicitContext {
    /// Rewrites `term`, in which at most the constants introduced by preprocessing occur free.
    pub(crate) fn rewrite_closed(&mut self, term: &DataExpression) -> DataExpression {
        let sigma = SliceSubstitution::new(&self.constant_variables, &self.constant_values);
        self.rewriter.rewrite_with(term, &sigma)
    }

    /// The summation variables of the summand with the given index on this thread.
    pub(crate) fn summation_variables(&self, index: usize) -> &[DataVariable] {
        &self.summands[index].summation_variables
    }

    /// The condition of the summand with the given index on this thread.
    pub(crate) fn condition(&self, index: usize) -> &DataExpression {
        &self.summands[index].condition
    }

    /// The new values of the parameters that the summand with the given index writes.
    pub(crate) fn writes(&self, index: usize) -> &[DataExpression] {
        &self.summands[index].writes
    }
}

/// A single summand of the LPS, prepared for explicit enumeration.
pub struct ExplicitSummand {
    index: usize,
    read_indices: Vec<usize>,
    write_indices: Vec<usize>,
    /// The constants, as indices in the constant assignments, that occur in this summand.
    constants: Vec<usize>,
    values: Arc<ValueTable>,
}

impl ExplicitSummand {
    fn new(
        index: usize,
        summand: &crate::ActionSummand,
        parameters: &[DataVariable],
        constants: &[DataVariable],
        values: Arc<ValueTable>,
    ) -> Self {
        debug_assert!(
            summand
                .summation_variables
                .iter()
                .all(|variable| !parameters.contains(variable)),
            "A summation variable shadows a process parameter"
        );

        // An assignment `d := d` leaves `d` unchanged.
        let (write_indices, writes): (Vec<usize>, Vec<&DataExpression>) = summand
            .next_state
            .iter()
            .enumerate()
            .filter_map(|(position, value)| Some((position, value.as_ref()?)))
            .filter(|(position, value)| DataExpression::from(parameters[*position].clone()) != **value)
            .unzip();

        let mut terms: Vec<&DataExpression> = writes.clone();
        terms.push(&summand.condition);
        terms.extend(summand.action.time.iter());
        for action in &summand.action.actions {
            terms.extend(action.arguments.iter());
        }

        let occurring = |variables: &[DataVariable]| -> Vec<usize> {
            let mut occurs = vec![false; variables.len()];
            for term in &terms {
                mark_occurrences(*term, variables, &mut occurs);
            }
            (0..variables.len()).filter(|&position| occurs[position]).collect()
        };

        Self {
            index,
            read_indices: occurring(parameters),
            constants: occurring(constants),
            write_indices,
            values,
        }
    }

    /// The parameters that are written, as positions in the state vector, parallel to the
    /// [ExplicitContext::writes] of this summand.
    pub(crate) fn write_indices(&self) -> &[usize] {
        &self.write_indices
    }

    /// The terms of `summand`, which is this summand of the LPS as read back on the current thread.
    fn thread_summand(
        &self,
        summand: &ActionSummand,
        parameters: &[DataVariable],
        constants: &[(DataVariable, DataExpression)],
    ) -> ThreadSummand {
        let summation_variables = summand.summation_variables.clone();

        let mut variables = summation_variables.clone();
        variables.extend(self.read_indices.iter().map(|&position| parameters[position].clone()));
        variables.extend(self.constants.iter().map(|&index| constants[index].0.clone()));

        let placeholder = DataExpression::bool_literal(false);
        let mut values = vec![placeholder; summation_variables.len() + self.read_indices.len()];
        values.extend(self.constants.iter().map(|&index| constants[index].1.clone()));

        ThreadSummand {
            condition: summand.condition.clone(),
            summation_variables,
            variables,
            values,
            writes: self
                .write_indices
                .iter()
                .map(|&position| {
                    summand.next_state[position]
                        .clone()
                        .expect("a written parameter is assigned")
                })
                .collect(),
            actions: summand
                .action
                .actions
                .iter()
                .map(|action| (action_label_term(&action.label), action.arguments.clone()))
                .collect(),
            time: summand.action.time.clone(),
        }
    }
}

impl LPS for ExplicitLinearProcessSpecification {
    type Value = usize;
    type Label = LpsMultiActionLabel;
    type StateInfo = ();
    type Summand = ExplicitSummand;

    fn initial_state(&self) -> Vec<usize> {
        self.initial_state.clone()
    }

    fn summands(&self) -> &[Self::Summand] {
        &self.summands
    }

    fn create_context(&self) -> ExplicitContext {
        let ExplorationInput {
            lps,
            data_specification,
            constants,
        } = self.input();
        let specification = RewriteSpecification::from_data_specification(&data_specification);
        let plans = Rc::new(EnumerationPlans::build(&data_specification));

        ExplicitContext {
            rewriter: DataRewriter::new(self.rewriter, &specification),
            enumerator: Enumerator::new(
                plans,
                FreshVariableGenerator::new("@enum", std::iter::empty::<String>()),
            ),
            frame: EnumeratorFrame::default(),
            summands: self
                .summands
                .iter()
                .zip(&lps.action_summands)
                .map(|(summand, terms)| summand.thread_summand(terms, &lps.process_parameters, &constants))
                .collect(),
            constant_variables: constants.iter().map(|(variable, _)| variable.clone()).collect(),
            constant_values: constants.into_iter().map(|(_, value)| value).collect(),
            cache: ValueCache::default(),
            labels: HashMap::new(),
            next_state: Vec::new(),
        }
    }

    fn prepare<'a>(&'a self, _context: &mut ExplicitContext, _state: &'a [usize]) -> impl Iterator<Item = usize> + 'a {
        0..self.summands.len()
    }

    fn state_info(&self, _state: &[usize], _context: &ExplicitContext) -> Self::StateInfo {}
}

/// Rewrites the effects of a summand under the substitution of `variables` by `values`, and reports
/// the resulting transition.
#[allow(clippy::too_many_arguments)]
fn report_solution<F>(
    summand: &ExplicitSummand,
    rewriter: &mut DataRewriter,
    variables: &[DataVariable],
    values: &[DataExpression],
    writes: &[DataExpression],
    actions: &[(MCRL2ActionLabel, Vec<DataExpression>)],
    time: &Option<DataExpression>,
    cache: &mut ValueCache,
    labels: &mut HashMap<ATerm, LpsMultiActionLabel>,
    next_state: &mut [usize],
    report: &mut F,
) -> Result<(), MercError>
where
    F: FnMut(&LpsMultiActionLabel, &[usize]) -> Result<(), MercError>,
{
    let sigma = SliceSubstitution::new(variables, values);
    for (&position, write) in summand.write_indices.iter().zip(writes) {
        let value = rewriter.rewrite_with(write, &sigma);
        next_state[position] = cache.intern(&summand.values, &value);
    }

    let action_terms = actions
        .iter()
        .map(|(label, arguments)| {
            MCRL2Action::new(
                label.copy(),
                ATermList::from_double_iter(
                    arguments
                        .iter()
                        .map(|argument| rewriter.rewrite_with(argument, &sigma))
                        .collect::<Vec<_>>()
                        .into_iter(),
                ),
            )
        })
        .collect();
    let time = match time {
        Some(time) => rewriter.rewrite_with(time, &sigma),
        None => undefined_real(),
    };
    let term: ATerm = multi_action_term(action_terms, &time).into();

    let label = labels.entry(term).or_insert_with_key(|term| LpsMultiActionLabel {
        term: ATermSend::from(term.clone()),
    });
    report(label, next_state)
}

impl Summand for ExplicitSummand {
    type Value = usize;
    type Label = LpsMultiActionLabel;
    type Context = ExplicitContext;

    fn read_positions(&self) -> &[usize] {
        &self.read_indices
    }

    fn effect(&self) -> StateEffect<'_> {
        StateEffect::Positions(&self.write_indices)
    }

    fn enumerate<F>(&self, context: &mut Self::Context, state: &[usize], mut report: F) -> Result<(), MercError>
    where
        F: FnMut(&Self::Label, &[usize]) -> Result<(), MercError>,
    {
        let ExplicitContext {
            rewriter,
            enumerator,
            frame,
            summands,
            cache,
            labels,
            next_state,
            ..
        } = context;
        let ThreadSummand {
            condition,
            summation_variables,
            variables,
            values,
            writes,
            actions,
            time,
        } = &mut summands[self.index];

        let num_summation_variables = summation_variables.len();
        for (offset, &position) in self.read_indices.iter().enumerate() {
            values[num_summation_variables + offset] = cache.get(&self.values, state[position]).clone();
        }

        let condition = rewriter.rewrite_with(
            condition,
            &SliceSubstitution::new(
                &variables[num_summation_variables..],
                &values[num_summation_variables..],
            ),
        );
        if is_bool_literal(&condition, false) {
            return Ok(());
        }

        next_state.clear();
        next_state.extend_from_slice(state);

        if summation_variables.is_empty() {
            if !is_bool_literal(&condition, true) {
                return Err(undecided_condition(&condition));
            }
            return report_solution(
                self,
                rewriter,
                variables,
                values,
                writes,
                actions,
                time,
                cache,
                labels,
                next_state,
                &mut report,
            );
        }

        let outcome =
            enumerator.enumerate_normalized(frame, rewriter, summation_variables, condition, |rewriter, solution| {
                values[..num_summation_variables].clone_from_slice(solution.values());
                match report_solution(
                    self,
                    rewriter,
                    variables,
                    values,
                    writes,
                    actions,
                    time,
                    cache,
                    labels,
                    next_state,
                    &mut report,
                ) {
                    Ok(()) => ControlFlow::Continue(()),
                    Err(error) => ControlFlow::Break(error),
                }
            });

        match outcome {
            Outcome::Exhausted => Ok(()),
            Outcome::Stopped(error) => Err(error),
            Outcome::LimitReached => Err("The enumeration of a summation variable hit a limit".into()),
            Outcome::Undecided(condition) => Err(undecided_condition(&condition)),
            Outcome::NotEnumerable(variable, reason) => Err(format!(
                "Summation variable {} cannot be enumerated: {reason:?}",
                variable.name()
            )
            .into()),
        }
    }
}

/// The error for a closed condition that did not rewrite to a boolean.
fn undecided_condition(condition: &DataExpression) -> MercError {
    format!(
        "The condition of a summand did not rewrite to true or false: {}",
        Pretty::from(condition)
    )
    .into()
}

impl fmt::Debug for ExplicitLinearProcessSpecification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ExplicitLinearProcessSpecification:")?;
        for summand in &self.summands {
            writeln!(
                f,
                "  summand {}: read {:?}, write {:?}",
                summand.index, summand.read_indices, summand.write_indices
            )?;
        }
        Ok(())
    }
}
