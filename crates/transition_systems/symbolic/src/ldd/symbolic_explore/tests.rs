use std::ops::ControlFlow;

use crate::LDD_CACHE_CAPACITY;
use crate::LDD_NODE_CAPACITY;
use merc_aterm::ATermString;
use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_data::Mcrl2DataSpecification;
use merc_lts::LTS;
use merc_lts::LtsAction;
use merc_lts::LtsBuilderMem;
use merc_lts::LtsMultiAction;
use merc_lts::TransitionLabel;
use merc_utilities::Timing;
use merc_utilities::random_test;
use oxidd::LevelNo;
use oxidd::Manager;
use oxidd::ManagerRef;
use oxidd::ldd::LDDFunction;
use oxidd::ldd::LDDManagerRef;
use oxidd::ldd::RelationMeta;
use oxidd::ldd::Value;
use rand::RngExt;

use crate::ExplorationStrategy;
use crate::LddLenCache;
use crate::ReachabilityOptions;
use crate::ReachabilityResult;
use crate::SummandGroup;
use crate::SylvanLts;
use crate::SylvanTransitionGroup;
use crate::SymbolicLPS;
use crate::SymbolicLTS;
use crate::SymbolicLts;
use crate::convert_symbolic_lts;
use crate::from_iter;
use crate::ldd_len;
use crate::random_symbolic_lts;
use crate::reachability_with_callback;
use crate::reachability_with_options;
use crate::read_sylvan;

/// Capacities of the managers for the tiny models below; allocating the default capacities for every
/// scenario would dominate the running time.
const SMALL_NODE_CAPACITY: usize = 1 << 16;
const SMALL_CACHE_CAPACITY: usize = 1 << 16;

/// The number of vectors in `ldd`, which is small enough for every test in this module to be exact.
fn count(ldd: &LDDFunction) -> usize {
    ldd_len(ldd, &mut LddLenCache::new())
        .exact()
        .expect("the count of a test set is exact") as usize
}

/// All exploration strategies; every one of them must compute the same reachable set.
const ALL_STRATEGIES: [ExplorationStrategy; 5] = [
    ExplorationStrategy::BreadthFirst,
    ExplorationStrategy::Chaining,
    ExplorationStrategy::Fixpoint,
    ExplorationStrategy::FixpointChaining,
    ExplorationStrategy::Saturation,
];

/// Whether the strategy alternates learning and applying the relations in rounds, so that it only
/// knows it is done after a round that changes nothing.
fn learns_in_rounds(strategy: ExplorationStrategy) -> bool {
    strategy == ExplorationStrategy::Saturation
}

/// Runs [reachability_with_options] on `lts` with a fresh context.
fn reach<L: SymbolicLPS>(manager: &LDDManagerRef, lts: &mut L, options: &ReachabilityOptions) -> ReachabilityResult {
    let mut context = lts.create_context();
    reachability_with_options(manager, lts, &mut context, options, &Timing::new())
        .expect("Reachability should work correctly")
}

/// Explores the Sylvan fixture in `bytes` with the given strategy in a fresh manager, and returns the
/// manager, the LTS and its reachable states.
fn explore_fixture(bytes: &[u8], strategy: ExplorationStrategy) -> (LDDManagerRef, SylvanLts, LDDFunction) {
    let ldd_manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
    let mut lts = read_sylvan(&ldd_manager, &mut &bytes[..]).expect("Loading should work correctly");

    let options = ReachabilityOptions {
        strategy,
        detect_deadlocks: false,
        // The groups of a Sylvan fixture are fully explored, so there is nothing to cache.
        cached: false,
    };
    let states = reach(&ldd_manager, &mut lts, &options).states;

    (ldd_manager, lts, states)
}

/// A [SylvanLts] together with its state space, so that it can be converted to an explicit LTS. The groups
/// of a Sylvan file have no action label position, so all transitions get the single default label.
struct SylvanLtsWithStates {
    lts: SylvanLts,
    states: LDDFunction,
    default_label: Vec<LtsMultiAction<LtsAction>>,
}

impl SymbolicLPS for SylvanLtsWithStates {
    type Group = SylvanTransitionGroup;
    type Learner<'a>
        = crate::NoLearner
    where
        Self: 'a;

    fn initial_state(&self) -> &LDDFunction {
        self.lts.initial_state()
    }

    fn transition_groups(&self) -> &[Self::Group] {
        self.lts.transition_groups()
    }

    fn transition_groups_mut(&mut self) -> &mut [Self::Group] {
        self.lts.transition_groups_mut()
    }

    fn create_context(&self) {}
}

impl SymbolicLTS for SylvanLtsWithStates {
    type Label = LtsMultiAction<LtsAction>;

    fn states(&self) -> &LDDFunction {
        &self.states
    }

    fn action_labels(&self) -> &[Self::Label] {
        &self.default_label
    }

    fn parameter_values(&self) -> &[Vec<DataExpression>] {
        &[]
    }
}

/// Asserts that every strategy finds as many reachable states as `BreadthFirst` on the Sylvan fixture in
/// `bytes`.
fn assert_strategies_agree(name: &str, bytes: &[u8]) {
    let expected = count(&explore_fixture(bytes, ExplorationStrategy::BreadthFirst).2);
    for strategy in ALL_STRATEGIES {
        assert_eq!(
            expected,
            count(&explore_fixture(bytes, strategy).2),
            "{name}: {strategy:?} disagrees with BreadthFirst"
        );
    }
}

/// Asserts that the states found by `BreadthFirst` on the Sylvan fixture in `bytes` are exactly the
/// reachable ones according to [assert_is_reachable_set]. Converting is far slower than exploring.
fn assert_fixture_is_reachable_set(name: &str, bytes: &[u8]) {
    let (manager, lts, states) = explore_fixture(bytes, ExplorationStrategy::BreadthFirst);
    let lts = SylvanLtsWithStates {
        lts,
        states,
        default_label: vec![LtsMultiAction::from_index(0)],
    };
    assert_is_reachable_set(name, &manager, &lts, None);
}

/// The callback is called once after every iteration, and can stop the exploration: what is found
/// up to that iteration is returned, and the result says that it is not complete.
#[test]
#[cfg_attr(miri, ignore)] // Miri is too slow
fn test_reachability_callback() {
    let bytes = include_bytes!("../../../../../../examples/ldd/anderson.4.ldd");

    for strategy in ALL_STRATEGIES {
        let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let options = ReachabilityOptions {
            strategy,
            detect_deadlocks: true,
            cached: false,
        };

        // Without stopping, every iteration is reported once, in order, and the sets only grow.
        let mut lts = read_sylvan(&manager, &mut &bytes[..]).expect("Loading should work correctly");
        lts.create_context();
        let mut seen: Vec<(usize, usize)> = Vec::new();
        let full = reachability_with_callback(
            &manager,
            &mut lts,
            &mut (),
            &options,
            &Timing::new(),
            |iteration, states| {
                seen.push((iteration, count(states)));
                ControlFlow::Continue(())
            },
        )
        .expect("Reachability should work correctly");

        assert!(
            full.complete,
            "{strategy:?}: an exploration that is not stopped is complete"
        );
        assert!(!seen.is_empty(), "{strategy:?}: the callback is never called");
        assert!(
            seen.iter()
                .enumerate()
                .all(|(index, (iteration, _))| *iteration == index + 1),
            "{strategy:?}: the iterations are not numbered consecutively from one: {seen:?}"
        );
        assert!(
            seen.windows(2).all(|pair| pair[0].1 <= pair[1].1),
            "{strategy:?}: the states shrink: {seen:?}"
        );
        assert_eq!(seen.last().unwrap().1, count(&full.states), "{strategy:?}");
        let iterations = seen.len();

        // Stopping after the first iteration returns exactly what the callback saw.
        let mut lts = read_sylvan(&manager, &mut &bytes[..]).expect("Loading should work correctly");
        lts.create_context();
        let mut calls = 0;
        let partial =
            reachability_with_callback(&manager, &mut lts, &mut (), &options, &Timing::new(), |iteration, _| {
                calls += 1;
                assert_eq!(iteration, 1, "{strategy:?}: called again after breaking");
                ControlFlow::Break(())
            })
            .expect("Reachability should work correctly");

        assert_eq!(calls, 1, "{strategy:?}");
        assert_eq!(count(&partial.states), seen[0].1, "{strategy:?}");
        assert!(
            partial.states.diff(&full.states).expect("diff works").is_empty(),
            "{strategy:?}: the states found so far are not reachable"
        );
        // The round-based strategies only know that they are done after a round that changes nothing, the
        // other strategies know when the iteration that is stopped was the last one.
        assert_eq!(
            partial.complete,
            iterations == 1 && !learns_in_rounds(strategy),
            "{strategy:?}"
        );
        if learns_in_rounds(strategy) {
            // The relations only cover what was learned from so far, so nothing is reported.
            assert_eq!(count(partial.deadlocks.as_ref().expect("requested")), 0);
        }

        // Stopping in the last iteration still gives the complete result, except for the round-based ones.
        if !learns_in_rounds(strategy) {
            let mut lts = read_sylvan(&manager, &mut &bytes[..]).expect("Loading should work correctly");
            lts.create_context();
            let last =
                reachability_with_callback(&manager, &mut lts, &mut (), &options, &Timing::new(), |iteration, _| {
                    if iteration == iterations {
                        ControlFlow::Break(())
                    } else {
                        ControlFlow::Continue(())
                    }
                })
                .expect("Reachability should work correctly");

            assert!(last.complete, "{strategy:?}");
            assert!(last.states == full.states, "{strategy:?}");
        }
    }
}

#[test]
#[cfg_attr(miri, ignore)] // Miri is too slow
fn test_reachability_strategies_agree() {
    // All strategies must compute the same reachable set, only the convergence speed differs.
    //
    // Only fixtures that `BreadthFirst` explores within the default manager capacity in reasonable
    // time can be used as an oracle: e.g. `anderson.6` and `anderson.8` run out of nodes.
    assert_strategies_agree(
        "anderson.4",
        include_bytes!("../../../../../../examples/ldd/anderson.4.ldd"),
    );
    assert_strategies_agree(
        "blocks.2",
        include_bytes!("../../../../../../examples/ldd/blocks.2.ldd"),
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_reachability_fixture_is_reachable_set() {
    assert_fixture_is_reachable_set(
        "blocks.2",
        include_bytes!("../../../../../../examples/ldd/blocks.2.ldd"),
    );
}

/// The larger fixtures take minutes in a debug build, and CI runs the debug tests with `--include-ignored`,
/// so instead of being ignored this test only exists in release builds, which the nightly CI runs.
#[test]
#[cfg(not(debug_assertions))]
#[cfg_attr(miri, ignore)] // Miri is too slow
fn test_reachability_fixtures_slow() {
    let anderson = include_bytes!("../../../../../../examples/ldd/anderson.4.ldd");
    let bakery = include_bytes!("../../../../../../examples/ldd/bakery.4.ldd");

    assert_fixture_is_reachable_set("anderson.4", anderson);
    assert_strategies_agree("bakery.4", bakery);
    assert_fixture_is_reachable_set("bakery.4", bakery);
}

#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_random_reachability_strategies_agree() {
    // Differential test: random groups with random read/write patterns (including write-only and
    // read-only groups, and read and write sets that only partially overlap) exercise every branch
    // of the `meta` dispatch.
    random_test(100, |rng| {
        let manager = oxidd::ldd::new_manager(1 << 20, 1 << 18, 1);
        let num_state_variables = rng.random_range(2..=8);
        let mut lts = random_symbolic_lts(rng, &manager, num_state_variables, 3).unwrap();

        assert_strategies_correct(&format!("{num_state_variables} variables"), &manager, &mut lts);
    });
}

/// Minimal hand-built LTS over a single parameter with transitions `0 -> 1 -> 2`, so the only
/// reachable deadlock is state `2`.
struct LineLts {
    initial: LDDFunction,
    groups: Vec<SylvanTransitionGroup>,
}

impl SymbolicLPS for LineLts {
    type Group = SylvanTransitionGroup;
    type Learner<'a>
        = crate::NoLearner
    where
        Self: 'a;

    fn initial_state(&self) -> &LDDFunction {
        &self.initial
    }

    fn transition_groups(&self) -> &[Self::Group] {
        &self.groups
    }

    fn transition_groups_mut(&mut self) -> &mut [Self::Group] {
        &mut self.groups
    }

    fn create_context(&self) {}
}

fn line_lts(manager: &oxidd::ldd::LDDManagerRef) -> LineLts {
    manager.with_manager_exclusive(|m| {
        m.add_vars(3);
    });

    // One read+write of parameter 0; relation short vectors place the read/write values at the
    // positions reported by `relation_meta`.
    let RelationMeta {
        meta,
        read_positions,
        write_positions,
    } = manager
        .with_manager_shared(|m| LDDFunction::relation_meta(m, &[0], &[0]))
        .expect("meta");
    let transition = |from: Value, to: Value| {
        let mut vector: Vec<Value> = vec![0; read_positions.len() + write_positions.len()];
        vector[read_positions[0] as usize] = from;
        vector[write_positions[0] as usize] = to;
        vector
    };

    let relation = from_iter(manager, [transition(0, 1), transition(1, 2)].iter());
    let group = SylvanTransitionGroup::new(relation, meta, vec![0], vec![0]);

    LineLts {
        initial: from_iter(manager, std::iter::once(&vec![0])),
        groups: vec![group],
    }
}

/// Explores `lts` with the given strategy, detecting deadlocks.
fn explore(manager: &LDDManagerRef, lts: &mut TestLts, strategy: ExplorationStrategy) -> ReachabilityResult {
    let options = ReachabilityOptions {
        strategy,
        detect_deadlocks: true,
        cached: false,
    };
    reach(manager, lts, &options)
}

/// Checks that the states of `lts` are exactly the states reachable from its initial state and, when given,
/// that `deadlocks` are exactly the ones among them without outgoing transitions.
///
/// The oracle does not use any exploration strategy, nor `relational_product` or `saturate`: the LTS is
/// converted to an explicit one by [convert_symbolic_lts], which applies the relation vectors to every
/// individual state.
fn assert_is_reachable_set<L: SymbolicLTS>(
    name: &str,
    manager: &LDDManagerRef,
    lts: &L,
    deadlocks: Option<&LDDFunction>,
) {
    // The conversion fails when a state has a successor outside of the states, i.e., when they are not
    // closed under the transitions, or when the initial state is missing.
    let mut builder = LtsBuilderMem::new(Vec::new(), Vec::new());
    let explicit = convert_symbolic_lts(manager, &mut builder, lts)
        .unwrap_or_else(|error| panic!("{name}: not a valid state space: {error}"));

    // Every state must be reachable from the initial state, otherwise the states contain spurious ones.
    let initial = explicit.initial_state_index();
    let mut visited = vec![false; explicit.num_of_states().max(initial.value() + 1)];
    visited[initial.value()] = true;
    let mut todo = vec![initial];
    let mut num_reachable = 0;
    let mut num_deadlocks = 0;
    while let Some(state) = todo.pop() {
        num_reachable += 1;

        let mut has_successor = false;
        for transition in explicit.outgoing_transitions(state) {
            has_successor = true;
            if !std::mem::replace(&mut visited[transition.to.value()], true) {
                todo.push(transition.to);
            }
        }

        if !has_successor {
            num_deadlocks += 1;
        }
    }

    // A state that does not occur in any transition is not part of the explicit LTS, so it is counted
    // as well by comparing against the size of the states.
    assert_eq!(num_reachable, count(lts.states()), "{name}: reachable states");

    if let Some(deadlocks) = deadlocks {
        assert_eq!(num_deadlocks, count(deadlocks), "{name}: deadlocks");
        assert!(
            deadlocks.diff(lts.states()).unwrap().is_empty(),
            "{name}: deadlocks outside of the reachable states"
        );
    }
}

/// Checks all strategies on `lts`: they must all agree with `BreadthFirst`, whose result must be correct
/// according to [assert_is_reachable_set].
///
/// All strategies share the manager, which also checks that the saturation cache is properly reset
/// between explorations.
fn assert_strategies_correct(name: &str, manager: &LDDManagerRef, lts: &mut TestLts) {
    let expected = explore(manager, lts, ExplorationStrategy::BreadthFirst);
    let expected_deadlocks = expected.deadlocks.expect("detect_deadlocks was requested");

    // All strategies find the same states, so it suffices to check the oracle once.
    lts.set_states(expected.states.clone());
    assert_is_reachable_set(name, manager, lts, Some(&expected_deadlocks));

    for strategy in ALL_STRATEGIES {
        let result = explore(manager, lts, strategy);
        let deadlocks = result.deadlocks.expect("detect_deadlocks was requested");

        assert!(
            result.states == expected.states,
            "{name}: {strategy:?} found {} reachable state(s), BreadthFirst found {}",
            count(&result.states),
            count(&expected.states),
        );
        assert!(
            deadlocks == expected_deadlocks,
            "{name}: {strategy:?} found {} deadlock(s), BreadthFirst found {}",
            count(&deadlocks),
            count(&expected_deadlocks),
        );
    }
}

/// A transition of a summand group.
struct TransitionSpec {
    /// The values of the read parameters, in the order of [GroupSpec::read].
    read: Vec<Value>,
    /// The values written to the write parameters, in the order of [GroupSpec::write].
    write: Vec<Value>,
    /// The action label, which is stored in a trailing position that `meta` does not cover.
    label: Value,
}

/// A summand group, described explicitly by its read and write parameters and its transitions.
struct GroupSpec {
    read: Vec<u32>,
    write: Vec<u32>,
    transitions: Vec<TransitionSpec>,
}

/// Shorthand for a [TransitionSpec].
fn transition(read: &[Value], write: &[Value], label: Value) -> TransitionSpec {
    TransitionSpec {
        read: read.to_vec(),
        write: write.to_vec(),
        label,
    }
}

/// Builds a [SymbolicLts] over `num_parameters` parameters with the given `initial` state and `groups`,
/// laid out the way `SymbolicLpsGroup` does: relation vectors hold the read/write positions given by the
/// meta, followed by a single action label position that is not covered by the meta.
fn summand_lts(manager: &LDDManagerRef, num_parameters: usize, initial: &[Value], groups: &[GroupSpec]) -> TestLts {
    manager.with_manager_exclusive(|m| {
        m.add_vars(1 + 2 * num_parameters as LevelNo);
    });

    let parameters = (0..num_parameters)
        .map(|i| DataVariable::new(ATermString::new(format!("p{i}"))))
        .collect::<Vec<_>>();

    let groups = groups
        .iter()
        .map(|group| {
            let RelationMeta {
                read_positions,
                write_positions,
                ..
            } = manager
                .with_manager_shared(|m| LDDFunction::relation_meta(m, &group.read, &group.write))
                .expect("meta");

            let vectors: Vec<Vec<Value>> = group
                .transitions
                .iter()
                .map(|t| {
                    let mut vector = vec![0; read_positions.len() + write_positions.len() + 1];
                    for (&position, &value) in read_positions.iter().zip(&t.read) {
                        vector[position as usize] = value;
                    }
                    for (&position, &value) in write_positions.iter().zip(&t.write) {
                        vector[position as usize] = value;
                    }
                    *vector.last_mut().unwrap() = t.label;
                    vector
                })
                .collect();

            SummandGroup::new(
                manager,
                &parameters,
                group.read.iter().map(|&i| parameters[i as usize].clone()).collect(),
                group.write.iter().map(|&i| parameters[i as usize].clone()).collect(),
                from_iter(manager, vectors.iter()),
            )
            .expect("parameters exist")
        })
        .collect();

    let initial = from_iter(manager, std::iter::once(&initial.to_vec()));
    let value = DataExpression::from_string("1").unwrap();

    SymbolicLts::new(
        Mcrl2DataSpecification::default(),
        parameters,
        initial.clone(),
        initial,
        groups,
        (0..NUM_LABELS).map(LtsMultiAction::from_index).collect(),
        vec![vec![value; NUM_VALUES]; num_parameters],
    )
}

/// The type of LTS that the random generator produces.
type TestLts = SymbolicLts<LtsMultiAction<LtsAction>>;

/// The number of action labels and parameter values used by the specified groups below.
const NUM_LABELS: usize = 3;
const NUM_VALUES: usize = 3;

/// Runs [assert_is_reachable_set] on the line `0 -> 1 -> 2` with the given state set.
fn check_line_states(states: &[Vec<Value>]) {
    let manager = oxidd::ldd::new_manager(SMALL_NODE_CAPACITY, SMALL_CACHE_CAPACITY, 1);
    let group = GroupSpec {
        read: vec![0],
        write: vec![0],
        transitions: vec![transition(&[0], &[1], 0), transition(&[1], &[2], 0)],
    };

    let mut lts = summand_lts(&manager, 1, &[0], &[group]);
    lts.set_states(from_iter(&manager, states.iter()));
    assert_is_reachable_set("line", &manager, &lts, None);
}

#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_oracle_accepts_reachable_states() {
    check_line_states(&[vec![0], vec![1], vec![2]]);
}

#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
#[should_panic(expected = "not a valid state space")]
fn test_oracle_rejects_missing_states() {
    // State 1 has a successor, state 2, that is not in the state set.
    check_line_states(&[vec![0], vec![1]]);
}

#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
#[should_panic(expected = "reachable states")]
fn test_oracle_rejects_spurious_states() {
    // State 3 is closed under the transitions, but not reachable from the initial state.
    check_line_states(&[vec![0], vec![1], vec![2], vec![3]]);
}

#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_reachability_degenerate_groups() {
    // A group without read and write parameters: its relation only holds the trailing action label, so
    // it is a self-loop on every state that must neither add states nor be reported as a deadlock.
    let unconditional = || GroupSpec {
        read: vec![],
        write: vec![],
        transitions: vec![transition(&[], &[], 0)],
    };
    // Write-only: fires on every state. The transitions differ only in the value written and the label.
    let write_only = || GroupSpec {
        read: vec![],
        write: vec![0],
        transitions: vec![transition(&[], &[1], 1), transition(&[], &[2], 2)],
    };
    // Read-only: a guard that never changes the state.
    let read_only = || GroupSpec {
        read: vec![0],
        write: vec![],
        transitions: vec![transition(&[2], &[], 1)],
    };
    // Reads and writes disjoint parameters.
    let disjoint = || GroupSpec {
        read: vec![0],
        write: vec![2],
        transitions: vec![transition(&[1], &[1], 0), transition(&[2], &[2], 1)],
    };
    // Reads and writes the same parameter. The same read/write pair occurs with several labels, so that
    // the trailing position must be ignored by the product and only deduplicated by the relation.
    let read_write = || GroupSpec {
        read: vec![1],
        write: vec![1],
        transitions: (0..2)
            .flat_map(|v| (0..3).map(move |label| transition(&[v], &[v + 1], label)))
            .collect(),
    };
    // Reads two parameters and writes one of them, the other being read-only.
    let overlapping = || GroupSpec {
        read: vec![1, 2],
        write: vec![2],
        transitions: vec![transition(&[2, 0], &[1], 0), transition(&[1, 1], &[2], 1)],
    };
    // A group that never fires.
    let empty = || GroupSpec {
        read: vec![0],
        write: vec![1],
        transitions: vec![],
    };

    // The models are tiny, so a small manager is enough.
    let check = |name: &str, groups: &[GroupSpec]| {
        let manager = oxidd::ldd::new_manager(SMALL_NODE_CAPACITY, SMALL_CACHE_CAPACITY, 1);
        let mut lts = summand_lts(&manager, 3, &[0, 0, 0], groups);
        assert_strategies_correct(name, &manager, &mut lts);
    };

    check(
        "all",
        &[
            unconditional(),
            write_only(),
            read_only(),
            disjoint(),
            read_write(),
            overlapping(),
            empty(),
        ],
    );

    // Without the groups that fire in every state there are deadlocks, and every subset of the
    // remaining groups is a different mix of the `meta` branches.
    let optional: [fn() -> GroupSpec; 6] = [read_only, disjoint, read_write, overlapping, empty, unconditional];
    for mask in 1..(1u32 << optional.len()) {
        let groups = optional
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, group)| group())
            .collect::<Vec<_>>();
        check(&format!("mask {mask:#b}"), &groups);
    }
}

#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_reachability_detect_deadlocks() {
    for strategy in ALL_STRATEGIES {
        let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let mut lts = line_lts(&manager);

        let options = ReachabilityOptions {
            strategy,
            detect_deadlocks: true,
            cached: false,
        };
        lts.create_context();
        let result = reachability_with_options(&manager, &mut lts, &mut (), &options, &Timing::new())
            .expect("Reachability should work correctly");

        // States 0, 1, 2 are reachable and only state 2 has no outgoing transition.
        assert_eq!(count(&result.states), 3, "{strategy:?}");
        let deadlocks = result.deadlocks.expect("detect_deadlocks was requested");
        assert_eq!(count(&deadlocks), 1, "{strategy:?}");
    }
}
