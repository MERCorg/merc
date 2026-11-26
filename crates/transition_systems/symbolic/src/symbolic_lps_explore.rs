use std::fmt;

use log::debug;

use merc_collections::IndexedSet;
use merc_explore::LPS;
use merc_explore::PermutedLps;
use merc_explore::Summand;
use merc_unsafety::WorkerLocal;
use merc_utilities::MercError;
use oxidd::HasWorkers;
use oxidd::LevelNo;
use oxidd::Manager;
use oxidd::ManagerRef;
use oxidd::WorkerPool;
use oxidd::ldd::LDDFunction;
use oxidd::ldd::LDDManagerRef;
use oxidd::ldd::RelationMeta;
use oxidd::ldd::Value;
use parking_lot::Mutex;

use crate::GroupLearner;
use crate::ReadWritePattern;
use crate::SummandGrouping;
use crate::SymbolicLPS;
use crate::TransitionGroup;
use crate::VariableOrder;
use crate::iter_par;
use crate::print_read_write_patterns;
use crate::print_transition_groups;
use crate::transition_group_pattern;

/// A symbolic LDD view of any [`merc_explore::LPS`].
///
/// This adapts the explicit-exploration abstraction (an `LPS` exposing summands
/// with read/write positions, a `prepare` step selecting the summands that fire
/// from a state, and per-summand enumeration) into the disjunctive LDD
/// transition relation consumed by [`crate::reachability`]. A single generic
/// implementation therefore drives symbolic exploration for both LPSs and PBESs
/// in SRF form (and anything else implementing `LPS`).
///
/// The wrapped `LPS` interns parameter values in its own space; the diagrams store dense LDD values
/// instead, so they stay compact regardless of how the `LPS` numbers its values. That second
/// interning, the [`Interning`], belongs to the `SymbolicLps` just as the first belongs to the
/// `LPS`: every exploration of it grows the same interning, so the diagrams of all runs use the same
/// LDD values. The value of the initial state at every position is LDD value 0.
///
/// Position `i` of the state vector is stored at level `i` of the diagrams. A variable order is
/// therefore not something this adapter applies: the constructors permute the state vector of the
/// `LPS` itself (see [`PermutedLps`]) up front, and everything below works in a single position
/// space.
pub struct SymbolicLps<L: LPS> {
    /// The wrapped `LPS` definition; the single source of summands and the
    /// `prepare`/`enumerate` machinery. Shared immutably with every group.
    lps: L,

    /// The symbolic transition groups, as determined by the [`SummandGrouping`].
    groups: Vec<SymbolicLpsGroup>,

    /// The encoded initial state, which is LDD value 0 at every position, see [`Interning`].
    initial_state: LDDFunction,

    /// The interning of the values and labels into dense LDD values, grown while learning.
    interning: Interning<L>,
}

/// The interning of the values and action labels of the `LPS` of a [`SymbolicLps`] into the dense
/// LDD values of its diagrams.
///
/// It only grows, and an LDD value keeps its meaning, so it describes the diagrams of every
/// exploration of the `SymbolicLps` so far. It is behind mutexes, since every worker of the pool
/// grows it while learning.
pub struct Interning<L: LPS> {
    /// Per state-vector position interning of `LPS` values into dense LDD values.
    columns: Vec<Mutex<IndexedSet<L::Value>>>,

    /// Interning of the action labels into dense LDD values, shared by every group so a label's
    /// index means the same thing everywhere, see [`TransitionGroup::action_label_index`].
    labels: Mutex<IndexedSet<L::Label>>,
}

impl<L: LPS> Interning<L> {
    /// The interning of the values of `initial_state` alone, which are LDD value 0 at every position.
    fn new(initial_state: Vec<L::Value>) -> Self {
        Self {
            columns: initial_state
                .into_iter()
                .map(|value| {
                    let mut column = IndexedSet::new();
                    column.insert(value);
                    Mutex::new(column)
                })
                .collect(),
            labels: Mutex::new(IndexedSet::new()),
        }
    }

    /// Returns the number of positions of the state vector.
    pub fn num_positions(&self) -> usize {
        self.columns.len()
    }

    /// Returns the values at `position`, where the `i`-th value is the one that the diagrams store as
    /// LDD value `i`.
    pub fn values(&self, position: usize) -> Vec<L::Value> {
        let column = self.columns[position].lock();
        (0..column.len())
            .map(|index| *column.get_by_index(index).expect("the interning is dense"))
            .collect()
    }

    /// Returns the action labels, where the `i`-th label is the one that the diagrams store as LDD
    /// value `i`.
    pub fn labels(&self) -> Vec<L::Label> {
        let labels = self.labels.lock();
        (0..labels.len())
            .map(|index| labels.get_by_index(index).expect("the interning is dense").clone())
            .collect()
    }
}

/// The choices that shape the symbolic encoding of an [`merc_explore::LPS`], mirroring the `--groups`
/// and `--reorder` options of the mCRL2 symbolic tools.
///
/// Neither option changes the reachable set, only the size of the decision diagrams and the amount of
/// work per exploration step.
#[derive(Clone, Debug, Default)]
pub struct SymbolicLpsOptions {
    /// How the summands are distributed over the transition groups.
    pub grouping: SummandGrouping,

    /// The order in which the state vector positions are stored in the diagram.
    pub order: VariableOrder,
}

impl<L: LPS> SymbolicLps<PermutedLps<L>> {
    /// Wraps `lps` into a symbolic LDD view, encoding its initial state, with one transition group
    /// per summand.
    ///
    /// See [SymbolicLps::with_options] for the levels that `manager` must have reserved.
    pub fn new(manager: &LDDManagerRef, lps: L) -> Result<Self, MercError> {
        SymbolicLps::with_options(manager, lps, &SymbolicLpsOptions::default())
    }

    /// Wraps `lps` into a symbolic LDD view, encoding its initial state, distributing its summands
    /// over the transition groups according to `grouping`.
    ///
    /// Joining summands into a single group trades the number of relational products per exploration
    /// step against the size of each transition relation; it does not change the reachable set.
    ///
    /// See [SymbolicLps::with_options] for the levels that `manager` must have reserved.
    pub fn with_grouping(manager: &LDDManagerRef, lps: L, grouping: &SummandGrouping) -> Result<Self, MercError> {
        let options = SymbolicLpsOptions {
            grouping: grouping.clone(),
            ..SymbolicLpsOptions::default()
        };
        SymbolicLps::with_options(manager, lps, &options)
    }

    /// Wraps `lps` into a symbolic LDD view, encoding its initial state, with the given grouping of
    /// its summands and order of its state vector positions.
    ///
    /// The order is applied by permuting the state vector of `lps` itself, so that position `i` of
    /// the LPS that is encoded below is stored at level `i` of the diagrams.
    pub fn with_options(manager: &LDDManagerRef, lps: L, options: &SymbolicLpsOptions) -> Result<Self, MercError> {
        let num_positions = lps.initial_state().len();

        // The `manager` must have at least `2 * n + 1` levels reserved. The
        // relations interleave a read and a write value per position, followed
        // by the action label.
        manager.with_manager_exclusive(|m| {
            m.add_vars(((2 * num_positions + 1) as LevelNo).saturating_sub(m.num_levels()));
        });

        let patterns = read_write_patterns(&lps, num_positions)?;
        debug!(
            "Read/write matrix of the summands:\n{}",
            print_read_write_patterns(&patterns)
        );

        // `order[level]` is the state vector position to store at `level` of the diagram, which is
        // realised by permuting the LPS; from here on the positions of `lps` *are* the levels.
        let order = options.order.compute(&patterns, num_positions)?;
        let lps = PermutedLps::new(lps, order)?;

        // The patterns of the permuted summands, i.e. the read/write matrix as the diagrams store it.
        let patterns = read_write_patterns(&lps, num_positions)?;

        // The interning starts with the values of the initial state, see `Interning::new`.
        let interning = Interning::new(lps.initial_state());
        let initial_state = manager.with_manager_shared(|m| LDDFunction::singleton(m, &vec![0; num_positions]))?;

        // Distribute the summands over the transition groups, and derive the read/write pattern each
        // group has to cover.
        let group_indices = options.grouping.compute(&patterns)?;
        let group_patterns = group_indices
            .iter()
            .map(|indices| transition_group_pattern(&patterns, indices))
            .collect::<Result<Vec<_>, MercError>>()?;

        if !matches!(options.grouping, SummandGrouping::None) || !matches!(options.order, VariableOrder::None) {
            debug!(
                "Read/write matrix of the transition groups (grouping {}):\n{}",
                options.grouping,
                print_transition_groups(&group_indices, &group_patterns)
            );
        }

        let mut groups = Vec::with_capacity(group_indices.len());
        for (indices, pattern) in group_indices.into_iter().zip(group_patterns) {
            // The short-vector encoding requires sorted read/write indices, which the patterns
            // already report in increasing order.
            let read_indices: Vec<Value> = pattern.read_positions().map(|p| p as Value).collect();
            let write_indices: Vec<Value> = pattern.write_positions().map(|p| p as Value).collect();

            let (meta, read_positions, write_positions, relation) =
                manager.with_manager_shared(|m| -> Result<_, MercError> {
                    let RelationMeta {
                        meta,
                        read_positions,
                        write_positions,
                    } = LDDFunction::relation_meta(m, &read_indices, &write_indices)?;
                    Ok((meta, read_positions, write_positions, LDDFunction::empty_set(m)))
                })?;

            groups.push(SymbolicLpsGroup {
                indices,
                has_labels: L::HAS_LABELS,
                read_indices,
                write_indices,
                read_positions,
                write_positions,
                meta,
                relation,
            });
        }

        Ok(SymbolicLps {
            lps,
            groups,
            initial_state,
            interning,
        })
    }
}

impl<L: LPS> SymbolicLps<L> {
    /// Returns the wrapped `LPS` definition.
    pub fn lps(&self) -> &L {
        &self.lps
    }

    /// Returns the interning of the values and labels into the LDD values of the diagrams, which
    /// grows during exploration.
    pub fn interning(&self) -> &Interning<L> {
        &self.interning
    }
}

/// Returns the read/write pattern of every summand of `lps` over the `num_positions` positions of its
/// state vector.
fn read_write_patterns<L: LPS>(lps: &L, num_positions: usize) -> Result<Vec<ReadWritePattern>, MercError> {
    lps.summands()
        .iter()
        .enumerate()
        .map(|(index, summand)| {
            // A symbolic transition relation is inherently positional: it stores only the read and
            // write columns, so there is no encoding for a summand whose next states change shape.
            let effect = summand.effect();
            let write_positions = effect.positions().ok_or_else(|| {
                MercError::from(format!(
                    "summand {index} has an opaque state effect, which symbolic exploration cannot encode"
                ))
            })?;

            ReadWritePattern::from_indices(num_positions, summand.read_positions(), write_positions)
                .map_err(|error| MercError::from(format!("summand {index}: {error}")))
        })
        .collect()
}

impl<L> SymbolicLPS for SymbolicLps<L>
where
    L: LPS + Sync,
    L::Value: Send,
    L::Label: Send,
{
    type Group = SymbolicLpsGroup;
    type Learner<'a>
        = ParallelLearner<'a, L>
    where
        Self: 'a;

    fn initial_state(&self) -> &LDDFunction {
        &self.initial_state
    }

    fn transition_groups(&self) -> &[Self::Group] {
        &self.groups
    }

    fn group_learner<'a>(&'a self, storage: &'a LDDManagerRef) -> Option<ParallelLearner<'a, L>> {
        Some(ParallelLearner::new(self, storage))
    }
}

/// A group of summands of a [`SymbolicLps`], encoded as a single short-vector LDD
/// transition relation that is learned on the fly during reachability.
pub struct SymbolicLpsGroup {
    /// Indices of the summands in this group within the summands of the `LPS`, in increasing order.
    indices: Vec<usize>,

    /// Whether the relation vectors end with the interned action label.
    has_labels: bool,

    /// Diagram levels read by the group (sorted), which are the state vector positions it reads.
    read_indices: Vec<Value>,

    /// Diagram levels written by the group (sorted), which are the state vector positions it writes.
    write_indices: Vec<Value>,

    /// Interleaved short-vector positions of `read_indices`.
    read_positions: Vec<LevelNo>,

    /// Interleaved short-vector positions of `write_indices`.
    write_positions: Vec<LevelNo>,

    /// Relational-product meta for `read_indices`/`write_indices`.
    meta: LDDFunction,

    /// The empty set: the relation is learned during exploration, which keeps it.
    relation: LDDFunction,
}

impl TransitionGroup for SymbolicLpsGroup {
    fn relation(&self) -> &LDDFunction {
        &self.relation
    }

    fn read_indices(&self) -> &[u32] {
        &self.read_indices
    }

    fn write_indices(&self) -> &[u32] {
        &self.write_indices
    }

    fn action_label_index(&self) -> Option<usize> {
        self.has_labels
            .then_some(self.read_indices.len() + self.write_indices.len())
    }

    fn meta(&self) -> &LDDFunction {
        &self.meta
    }
}

/// Reusable scratch buffers for learning the successors of one projected state at a time, see
/// [`learn_one_state`].
///
/// Every task of the parallel learner owns its own scratch state while sharing the interning.
struct LearnScratch<L: LPS> {
    /// Reusable full-length state buffer, seeded from the initial state so that positions no group
    /// ever writes keep their initial value.
    full_state: Vec<L::Value>,

    /// Reusable interleaved short vector for the relation singletons.
    interleaved: Vec<Value>,

    /// Reused buffer for the summands of the group that fire from the current state.
    applicable: Vec<usize>,
}

impl<L: LPS> LearnScratch<L> {
    /// Creates the scratch state for learning the successors of states of a group whose
    /// interleaved relation vectors have `vector_len` positions, with `num_summands` summands.
    fn new(lps: &L, vector_len: usize, num_summands: usize) -> Self {
        Self {
            full_state: lps.initial_state(),
            interleaved: vec![0; vector_len],
            applicable: Vec::with_capacity(num_summands),
        }
    }
}

impl SymbolicLpsGroup {
    /// The length of the relation vectors: the interleaved read and write positions, followed by
    /// the interned action label when the `LPS` has labels.
    fn vector_len(&self) -> usize {
        self.read_indices.len() + self.write_indices.len() + usize::from(self.has_labels)
    }
}

/// Learns the successors of one projected `short_state`, and unites the relation vector of every
/// transition into `relation` with [`LDDFunction::union_cube`].
///
/// `union_cube` never waits for the worker pool, so a worker does not run other work of the pool
/// while it holds its enumeration context.
fn learn_one_state<L: LPS>(
    lps: &L,
    group: &SymbolicLpsGroup,
    scratch: &mut LearnScratch<L>,
    enumerate: &mut <L::Summand as Summand>::Context,
    interning: &Interning<L>,
    relation: &mut LDDFunction,
    short_state: &[Value],
) -> Result<(), MercError> {
    debug_assert_eq!(
        short_state.len(),
        group.read_indices.len(),
        "Projected state must have one value per read index"
    );

    // Overlay the short read values into the full state buffer and the
    // interleaved read positions.
    for (k, &value) in short_state.iter().enumerate() {
        let position = group.read_indices[k] as usize;
        scratch.full_state[position] = *interning.columns[position]
            .lock()
            .get_by_index(value as usize)
            .expect("read value must already be interned");
        scratch.interleaved[group.read_positions[k] as usize] = value;
    }

    // Prepare the backend assignments for this state and collect the summands of this group
    // that may fire from it.
    scratch.applicable.clear();
    scratch.applicable.extend(
        lps.prepare(enumerate, &scratch.full_state)
            .filter(|i| group.indices.binary_search(i).is_ok()),
    );

    // Enumerate the successors of every applicable summand, building the write side of each
    // short transition vector and unioning it into the relation.
    //
    // A summand that does not write one of the group's write positions leaves it untouched
    // (the [`StateEffect::Positions`] contract), so `next_state` carries the source value
    // there, which the group reads by construction of its read positions.
    let action_position = group.read_indices.len() + group.write_indices.len();
    for &index in &scratch.applicable {
        let write_indices = &group.write_indices;
        let write_positions = &group.write_positions;
        let interleaved = &mut scratch.interleaved;

        lps.summands()[index].enumerate(enumerate, &scratch.full_state, |label, next_state| {
            for (m, &write_index) in write_indices.iter().enumerate() {
                let position = write_index as usize;
                let (value, _) = interning.columns[position].lock().insert(next_state[position]);
                interleaved[write_positions[m] as usize] = *value as Value;
            }

            if L::HAS_LABELS {
                let (label_index, _) = interning.labels.lock().insert(label.clone());
                interleaved[action_position] = *label_index as Value;
            }

            *relation = relation.union_cube(interleaved)?;
            Ok(())
        })?;
    }
    Ok(())
}

/// The [`GroupLearner`] of a [`SymbolicLps`], which can be used from every worker of the pool. It
/// iterates the projected states with `iter_par`, and every worker enumerates with an enumeration
/// context of its own.
///
/// The enumeration contexts are created on the first use of every worker and kept for the lifetime
/// of the learner, i.e. one exploration run, since creating a context can be expensive.
pub struct ParallelLearner<'a, L: LPS> {
    lps: &'a SymbolicLps<L>,
    contexts: WorkerContexts<'a, <L::Summand as Summand>::Context>,
}

impl<'a, L: LPS> ParallelLearner<'a, L> {
    /// Creates the learner of `lps` for one exploration run on the workers of `storage`.
    pub fn new(lps: &'a SymbolicLps<L>, storage: &'a LDDManagerRef) -> Self {
        Self {
            lps,
            contexts: WorkerContexts {
                local: WorkerLocal::new(storage.workers().current_num_threads()),
                storage,
            },
        }
    }
}

/// The enumeration context of every worker. They may be thread-affine, so they are dropped on their
/// workers.
struct WorkerContexts<'a, C> {
    local: WorkerLocal<C>,
    storage: &'a LDDManagerRef,
}

impl<C> Drop for WorkerContexts<'_, C> {
    fn drop(&mut self) {
        let local = &self.local;
        self.storage.workers().broadcast(|_| local.clear_current());
    }
}

impl<L> GroupLearner for ParallelLearner<'_, L>
where
    L: LPS + Sync,
    L::Value: Send,
    L::Label: Send,
{
    fn learn(&self, group: usize, projected: &LDDFunction) -> Result<LDDFunction, MercError> {
        let lps = &self.lps.lps;
        let group = &self.lps.groups[group];

        // Every task of `iter_par` learns into a relation of its own. oxidd does not expose how far
        // the calling operation may still split, so this always iterates in parallel; the pool's
        // split depth bounds the nested splitting.
        let (relation, _) = iter_par(
            self.contexts.storage,
            projected,
            |manager| {
                (
                    LDDFunction::empty_set(manager),
                    LearnScratch::new(lps, group.vector_len(), group.indices.len()),
                )
            },
            |(relation, scratch), short_state| {
                self.contexts.local.with(
                    || lps.create_context(),
                    |context| learn_one_state(lps, group, scratch, context, &self.lps.interning, relation, short_state),
                )
            },
            |(a, scratch), (b, _)| Ok((a.union(&b)?, scratch)),
        )?;
        Ok(relation)
    }
}

impl<L: LPS> fmt::Debug for SymbolicLps<L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "SymbolicLps:")?;
        for (i, group) in self.groups.iter().enumerate() {
            writeln!(f, "  {i}: {group:?}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for SymbolicLpsGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "summands {:?} (read {:?}, write {:?})",
            self.indices, self.read_indices, self.write_indices
        )
    }
}

#[cfg(test)]
mod tests {
    use merc_explore::StateEffect;
    use merc_utilities::Timing;

    use crate::ExplorationStrategy;
    use crate::LDD_CACHE_CAPACITY;
    use crate::LDD_NODE_CAPACITY;
    use crate::LddLenCache;
    use crate::ReachabilityOptions;
    use crate::ldd_len;
    use crate::reachability_with_options;

    use super::LPS;
    use super::MercError;
    use super::Summand;
    use super::SummandGrouping;
    use super::SymbolicLPS;
    use super::SymbolicLps;
    use super::SymbolicLpsOptions;
    use super::TransitionGroup;
    use super::Value;
    use super::VariableOrder;

    /// A grid [`LPS`]: summand `i` increments position `i` while it is below its bound, and one
    /// extra summand increments the first two positions at once. The diagonal summand couples two
    /// positions, so the transition groups are not all over a single position.
    struct GridLps {
        summands: Vec<GridSummand>,
        num_positions: usize,
    }

    /// A guarded increment of [`GridLps`], writing the positions it also reads.
    struct GridSummand {
        /// The positions incremented, which are exactly the ones read and written.
        positions: Vec<usize>,
        /// The guard: every incremented position must be below this bound.
        bound: usize,
    }

    impl GridLps {
        /// The grid over `bounds.len()` positions, with the diagonal summand over the first two.
        fn new(bounds: &[usize]) -> GridLps {
            let mut summands: Vec<GridSummand> = bounds
                .iter()
                .enumerate()
                .map(|(position, &bound)| GridSummand {
                    positions: vec![position],
                    bound,
                })
                .collect();
            summands.push(GridSummand {
                positions: vec![0, 1],
                bound: bounds[0].min(bounds[1]),
            });

            GridLps {
                summands,
                num_positions: bounds.len(),
            }
        }

        /// Like [`Self::new`], with one more summand that reads and writes nothing: a self-loop on
        /// every state, so that no state is a deadlock.
        fn with_self_loop(bounds: &[usize]) -> GridLps {
            let mut lps = GridLps::new(bounds);
            lps.summands.push(GridSummand {
                positions: Vec::new(),
                bound: 0,
            });
            lps
        }
    }

    impl LPS for GridLps {
        type Value = usize;
        type Label = ();
        type StateInfo = ();
        type Summand = GridSummand;

        fn initial_state(&self) -> Vec<usize> {
            vec![0; self.num_positions]
        }

        fn summands(&self) -> &[GridSummand] {
            &self.summands
        }

        fn create_context(&self) -> Vec<usize> {
            Vec::new()
        }

        fn prepare<'a>(&'a self, _context: &mut Vec<usize>, _state: &'a [usize]) -> impl Iterator<Item = usize> + 'a {
            0..self.summands.len()
        }

        fn state_info(&self, _state: &[usize], _context: &Vec<usize>) {}
    }

    impl Summand for GridSummand {
        type Value = usize;
        type Label = ();
        type Context = Vec<usize>;

        fn enumerate<F>(&self, context: &mut Vec<usize>, state: &[usize], mut report: F) -> Result<(), MercError>
        where
            F: FnMut(&(), &[usize]) -> Result<(), MercError>,
        {
            if self.positions.iter().any(|&position| state[position] >= self.bound) {
                return Ok(());
            }

            context.clear();
            context.extend_from_slice(state);
            for &position in &self.positions {
                context[position] += 1;
            }
            report(&(), context)
        }

        fn read_positions(&self) -> &[usize] {
            &self.positions
        }

        fn effect(&self) -> StateEffect<'_> {
            StateEffect::Positions(&self.positions)
        }
    }

    /// Explores the grid over `bounds` with the given encoding and strategy,
    /// and returns the reachable state count.
    fn explored_count(bounds: &[usize], options: &SymbolicLpsOptions, strategy: ExplorationStrategy) -> usize {
        explored_count_with(
            bounds,
            options,
            &ReachabilityOptions {
                strategy,
                ..ReachabilityOptions::default()
            },
            1,
        )
    }

    /// Explores the grid over `bounds` on a manager with `threads` workers, and returns the
    /// reachable state count.
    fn explored_count_with(
        bounds: &[usize],
        options: &SymbolicLpsOptions,
        reachability_options: &ReachabilityOptions,
        threads: u32,
    ) -> usize {
        let storage = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, threads);

        let symbolic =
            SymbolicLps::with_options(&storage, GridLps::new(bounds), options).expect("the encoding is valid");

        let states = reachability_with_options(&storage, &symbolic, reachability_options, &Timing::new())
            .expect("reachability succeeds")
            .states;
        ldd_len(&states, &mut LddLenCache::new())
            .exact()
            .expect("the grids are small") as usize
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Miri is too slow.
    fn test_variable_order_preserves_the_reachable_states() {
        let bounds = [2, 3, 4];

        // Every combination of coordinates below its bound is reachable.
        let expected = bounds.iter().map(|bound| bound + 1).product::<usize>();

        let strategies = [
            ExplorationStrategy::BreadthFirst,
            ExplorationStrategy::Chaining,
            ExplorationStrategy::Fixpoint,
            ExplorationStrategy::FixpointChaining,
            ExplorationStrategy::Saturation,
        ];

        for strategy in strategies {
            assert_eq!(
                explored_count(&bounds, &SymbolicLpsOptions::default(), strategy),
                expected,
                "strategy {strategy:?} changed the reachable states"
            );
        }

        // The order only decides how the state vector is stored, never which states are reachable.
        for order in [vec![0, 1, 2], vec![2, 1, 0], vec![1, 2, 0], vec![0, 2, 1]] {
            for grouping in [SummandGrouping::None, SummandGrouping::Used, SummandGrouping::Simple] {
                let options = SymbolicLpsOptions {
                    grouping,
                    order: VariableOrder::Explicit(order.clone()),
                };
                for strategy in strategies {
                    assert_eq!(
                        explored_count(&bounds, &options, strategy),
                        expected,
                        "variable order {order:?}, strategy {strategy:?} changed the reachable states"
                    );
                }
            }
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // miri does not work with oxidd.
    fn test_variable_order_permutes_the_transition_groups() {
        let storage = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);

        let options = SymbolicLpsOptions {
            grouping: SummandGrouping::None,
            order: VariableOrder::Explicit(vec![2, 1, 0]),
        };
        let symbolic =
            SymbolicLps::with_options(&storage, GridLps::new(&[2, 3, 4]), &options).expect("the encoding is valid");

        // Summand `i` reads and writes position `i`, which the reversed order stores at level
        // `2 - i`; the groups are given as read/write levels alone and stay in summand order.
        let levels: Vec<Vec<Value>> = symbolic
            .transition_groups()
            .iter()
            .map(|group| group.read_indices().to_vec())
            .collect();
        assert_eq!(levels, vec![vec![2], vec![1], vec![0], vec![1, 2]]);
    }

    /// Explores the grid over `bounds` with the given encoding, strategy and domain caching, and
    /// returns the reachable state count.
    #[test]
    #[cfg_attr(miri, ignore)] // Miri is too slow.
    fn test_domain_caching_preserves_the_reachable_states() {
        let bounds = [4, 3, 5];
        // Every combination of coordinates below its bound is reachable.
        let expected = bounds.iter().map(|bound| bound + 1).product::<usize>();

        let strategies = [
            ExplorationStrategy::BreadthFirst,
            ExplorationStrategy::Chaining,
            ExplorationStrategy::Fixpoint,
            ExplorationStrategy::FixpointChaining,
            ExplorationStrategy::Saturation,
        ];

        for strategy in strategies {
            for cached in [false, true] {
                let reachability_options = ReachabilityOptions {
                    strategy,
                    cached,
                    ..ReachabilityOptions::default()
                };
                assert_eq!(
                    explored_count_with(&bounds, &SymbolicLpsOptions::default(), &reachability_options, 1),
                    expected,
                    "strategy {strategy:?} with cached = {cached} changed the reachable states"
                );
            }
        }
    }

    /// Parallel transition learning must not change the reachable set, whatever the strategy and
    /// whether the learned domains are cached.
    #[test]
    #[cfg_attr(miri, ignore)] // Miri is too slow.
    fn test_parallel_learning_preserves_the_reachable_states() {
        let bounds = [4, 3, 5];
        // Every combination of coordinates below its bound is reachable.
        let expected = bounds.iter().map(|bound| bound + 1).product::<usize>();

        let strategies = [
            ExplorationStrategy::BreadthFirst,
            ExplorationStrategy::Chaining,
            ExplorationStrategy::Fixpoint,
            ExplorationStrategy::FixpointChaining,
            ExplorationStrategy::Saturation,
        ];

        for strategy in strategies {
            for cached in [false, true] {
                let reachability_options = ReachabilityOptions {
                    strategy,
                    cached,
                    ..ReachabilityOptions::default()
                };
                assert_eq!(
                    explored_count_with(&bounds, &SymbolicLpsOptions::default(), &reachability_options, 4),
                    expected,
                    "parallel learning under strategy {strategy:?} with cached = {cached} changed the reachable states"
                );
            }
        }
    }

    /// A grid large enough that parallel learning splits the projected states over several workers,
    /// and that node-wise saturation learns from several workers at once, must still reach exactly
    /// the serial learner's set.
    #[test]
    #[cfg_attr(miri, ignore)] // Miri is too slow.
    fn test_parallel_learning_matches_serial_across_batches() {
        let bounds = [64, 56, 48];
        let expected = bounds.iter().map(|bound| bound + 1).product::<usize>();

        for strategy in [
            ExplorationStrategy::BreadthFirst,
            ExplorationStrategy::Chaining,
            ExplorationStrategy::Saturation,
        ] {
            for cached in [false, true] {
                let reachability_options = ReachabilityOptions {
                    strategy,
                    cached,
                    ..ReachabilityOptions::default()
                };
                let serial = explored_count_with(&bounds, &SymbolicLpsOptions::default(), &reachability_options, 1);
                let parallel = explored_count_with(&bounds, &SymbolicLpsOptions::default(), &reachability_options, 4);
                assert_eq!(
                    serial, expected,
                    "serial run under {strategy:?}/cached={cached} is wrong"
                );
                assert_eq!(
                    parallel, serial,
                    "parallel learning under {strategy:?}/cached={cached} changed the reachable states"
                );
            }
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // miri does not work with oxidd.
    fn test_invalid_variable_order_is_rejected() {
        let storage = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);

        let options = SymbolicLpsOptions {
            grouping: SummandGrouping::None,
            order: VariableOrder::Explicit(vec![0, 1]),
        };
        assert!(SymbolicLps::with_options(&storage, GridLps::new(&[2, 3, 4]), &options).is_err());
    }

    /// Every strategy, with and without caching, learns the same relations when it completes, also
    /// for a group that reads and writes nothing, and finds no deadlocks when every state has a
    /// self-loop.
    ///
    /// The relations of separate runs can be compared because they share a manager, and the grid
    /// interns the values of every position in increasing order whatever the strategy.
    #[test]
    #[cfg_attr(miri, ignore)] // miri does not work with oxidd.
    fn test_learned_relations_agree_across_strategies() {
        let bounds = [3, 2, 4];
        let expected_states = bounds.iter().map(|bound| bound + 1).product::<usize>();
        let storage = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);

        let mut expected_relations = None;
        for strategy in ALL_STRATEGIES {
            for cached in [false, true] {
                let symbolic = SymbolicLps::with_options(
                    &storage,
                    GridLps::with_self_loop(&bounds),
                    &SymbolicLpsOptions::default(),
                )
                .expect("the encoding is valid");
                let options = ReachabilityOptions {
                    strategy,
                    detect_deadlocks: true,
                    cached,
                };
                let result = reachability_with_options(&storage, &symbolic, &options, &Timing::new())
                    .expect("reachability succeeds");

                assert!(result.complete);
                assert_eq!(count(&result.states), expected_states, "{strategy:?}/cached={cached}");
                let deadlocks = result.deadlocks.expect("detect_deadlocks was requested");
                assert!(
                    deadlocks.is_empty(),
                    "{strategy:?}/cached={cached} reports {} deadlock(s), but every state has a self-loop",
                    count(&deadlocks)
                );

                let relations: Vec<_> = result.groups.iter().map(|group| group.relation().clone()).collect();
                assert!(
                    relations.iter().all(|relation| !relation.is_empty()),
                    "{strategy:?}/cached={cached} left a relation empty"
                );
                match &expected_relations {
                    None => expected_relations = Some(relations),
                    Some(expected) => assert!(
                        *expected == relations,
                        "{strategy:?}/cached={cached} learned other relations than {:?}",
                        ALL_STRATEGIES[0]
                    ),
                }
            }
        }
    }

    /// Exploring a [`SymbolicLps`] twice finds the same states and relations, and the values keep
    /// their LDD values.
    #[test]
    #[cfg_attr(miri, ignore)] // miri does not work with oxidd.
    fn test_exploring_twice_gives_the_same_result() {
        let storage = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 4);
        let symbolic = SymbolicLps::with_options(
            &storage,
            GridLps::with_self_loop(&[4, 3, 5]),
            &SymbolicLpsOptions::default(),
        )
        .expect("the encoding is valid");

        for strategy in [ExplorationStrategy::BreadthFirst, ExplorationStrategy::Saturation] {
            let options = ReachabilityOptions {
                strategy,
                ..ReachabilityOptions::default()
            };
            let first = reachability_with_options(&storage, &symbolic, &options, &Timing::new())
                .expect("reachability succeeds");
            let values: Vec<Vec<usize>> = (0..symbolic.interning().num_positions())
                .map(|position| symbolic.interning().values(position))
                .collect();

            let second = reachability_with_options(&storage, &symbolic, &options, &Timing::new())
                .expect("reachability succeeds");
            assert!(first.states == second.states, "{strategy:?}");
            for (a, b) in first.groups.iter().zip(&second.groups) {
                assert!(a.relation() == b.relation(), "{strategy:?}");
            }
            for (position, values) in values.iter().enumerate() {
                assert_eq!(*values, symbolic.interning().values(position), "{strategy:?}");
            }
        }
    }

    const ALL_STRATEGIES: [ExplorationStrategy; 5] = [
        ExplorationStrategy::BreadthFirst,
        ExplorationStrategy::Chaining,
        ExplorationStrategy::Fixpoint,
        ExplorationStrategy::FixpointChaining,
        ExplorationStrategy::Saturation,
    ];

    /// The number of vectors in `ldd`.
    fn count(ldd: &oxidd::ldd::LDDFunction) -> usize {
        ldd_len(ldd, &mut LddLenCache::new())
            .exact()
            .expect("the grids are small") as usize
    }

    /// A [`GridLps`] whose `prepare` panics, like a backend that hits an internal error.
    struct PanickingLps(GridLps);

    impl LPS for PanickingLps {
        type Value = usize;
        type Label = ();
        type StateInfo = ();
        type Summand = GridSummand;

        fn initial_state(&self) -> Vec<usize> {
            self.0.initial_state()
        }

        fn summands(&self) -> &[GridSummand] {
            self.0.summands()
        }

        fn create_context(&self) -> Vec<usize> {
            Vec::new()
        }

        fn prepare<'a>(&'a self, _context: &mut Vec<usize>, _state: &'a [usize]) -> impl Iterator<Item = usize> + 'a {
            panic!("the backend panics while learning");
            #[allow(unreachable_code)]
            (0..0)
        }

        fn state_info(&self, _state: &[usize], _context: &Vec<usize>) {}
    }

    /// A panic of the backend while learning is propagated to the caller as a panic; it does not
    /// abort the process (e.g. by panicking again while the learner is dropped during unwinding).
    #[test]
    #[cfg_attr(miri, ignore)] // miri does not work with oxidd.
    fn test_learner_panic_propagates_without_abort() {
        let storage = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 2);
        let symbolic = SymbolicLps::with_options(
            &storage,
            PanickingLps(GridLps::new(&[2, 3])),
            &SymbolicLpsOptions::default(),
        )
        .expect("the encoding is valid");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = reachability_with_options(&storage, &symbolic, &ReachabilityOptions::default(), &Timing::new());
        }));
        assert!(result.is_err(), "the panic of the backend is propagated");
    }
}
