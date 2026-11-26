//! Explores linear processes without using the mCRL2 toolset.
//!
//! The tests on the examples of `examples/mCRL2` need the toolset to linearise them: they read
//! `MCRL2_PATH`, which should point at a directory containing `mcrl22lps` and `lps2lts`, and are
//! skipped when it is not set. The expected sizes are those that `lps2lts` finds.

use std::any::Any;
use std::fs::File;
use std::ops::ControlFlow;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use merc_aterm::ATermString;
use merc_data::BasicSort;
use merc_data::DataEquation;
use merc_data::DataExpression;
use merc_data::DataFunctionSymbol;
use merc_data::DataVariable;
use merc_data::Mcrl2DataSpecification;
use merc_data::SortExpression;
use merc_explore::CachingStrategy;
use merc_explore::ExplorationStrategy;
use merc_io::temp_dir;
use merc_io::traced_command;
use merc_lps::Action;
use merc_lps::ActionLabel;
use merc_lps::ActionSummand;
use merc_lps::CfgLinearProcessSpecification;
use merc_lps::DEFAULT_PREPROCESSING;
use merc_lps::LinearProcessSpecification;
use merc_lps::LpsMultiActionLabel;
use merc_lps::LpsTransformation;
use merc_lps::MultiAction;
use merc_lps::TransformedLps;
use merc_lps::explore_lps_explicit;
use merc_lps::explore_lps_explicit_parallel;
use merc_lps::explore_lps_symbolic;
use merc_lps::preprocess;
use merc_lps::read_lps;
use merc_lps::transformations::AppliedTransformation;
use merc_lps::transformations::resolve_name_clashes;
use merc_lps::write_lps;
use merc_lts::LTS;
use merc_lts::LtsBuilderMem;
use merc_lts::MutexLtsBuilder;
use merc_lts::read_mcrl2_aut;
use merc_symbolic::LDD_CACHE_CAPACITY;
use merc_symbolic::LDD_NODE_CAPACITY;
use merc_symbolic::LddLenCache;
use merc_symbolic::ReachabilityOptions;
use merc_symbolic::SymbolicLpsOptions;
use merc_symbolic::ldd_len;
use merc_utilities::MercError;
use merc_utilities::Timing;
use test_case::test_case;

/// An example of `examples/mCRL2`, linearised by `mcrl22lps`.
struct Mcrl2Example {
    tools: PathBuf,
    lps_path: PathBuf,
    lps: LinearProcessSpecification,
    // Keeps the linearised example until the test is done.
    _directory: tempfile::TempDir,
}

impl Mcrl2Example {
    /// Linearises `example`, a path relative to `examples/mCRL2`, or returns `None` when
    /// `MCRL2_PATH` is not set.
    fn linearise(example: &str) -> Option<Self> {
        let Some(tools) = std::env::var_os("MCRL2_PATH") else {
            println!("Skipping test: MCRL2_PATH not set");
            return None;
        };
        let tools = PathBuf::from(tools);

        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../examples/mCRL2")
            .join(example);
        let directory = temp_dir("merc_lps_explore_test").unwrap();
        let lps_path = directory.path().join("example.lps");
        run(&tools.join("mcrl22lps"), &[&source, &lps_path]);

        let lps = read_lps(File::open(&lps_path).unwrap()).unwrap();
        Some(Mcrl2Example {
            tools,
            lps_path,
            lps,
            _directory: directory,
        })
    }

    /// The number of states and transitions that `lps2lts` finds.
    fn lps2lts(&self) -> (usize, usize) {
        let aut_path = self.lps_path.with_extension("aut");
        run(&self.tools.join("lps2lts"), &[&self.lps_path, &aut_path]);
        let lts = read_mcrl2_aut(File::open(&aut_path).unwrap()).unwrap();
        (lts.num_of_states(), lts.num_of_transitions())
    }

    /// The example with the transformations of [DEFAULT_PREPROCESSING] applied.
    fn preprocessed(&self) -> TransformedLps {
        preprocess(TransformedLps::new(&self.lps).unwrap(), DEFAULT_PREPROCESSING).unwrap()
    }
}

/// Runs `tool` with the given arguments, and fails when it does not succeed.
fn run(tool: &Path, arguments: &[&Path]) {
    let status = traced_command(Command::new(tool).args(arguments))
        .unwrap_or_else(|error| panic!("Failed to execute {}: {error}", tool.display()));
    assert!(status.success(), "{} failed with status: {status}", tool.display());
}

/// The message of a caught panic.
fn panic_message(payload: Box<dyn Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => payload
            .downcast_ref::<&str>()
            .map_or_else(String::new, |message| message.to_string()),
    }
}

fn builder() -> LtsBuilderMem<LpsMultiActionLabel> {
    LtsBuilderMem::new(Vec::new(), Vec::new())
}

#[test_case("academic/abp/abp.mcrl2" ; "alternating bit protocol")]
#[test_case("academic/mutex_models/Petersons/Petersons_spec.mcrl2" ; "numbers on left hand sides")]
#[test_case("academic/mutex_models/Mutex-naive/Mutex-naive_spec.mcrl2" ; "naive mutex")]
#[test_case("academic/mutex_models/Improved-mutex-naive/Improved-mutex-naive_spec.mcrl2" ; "improved naive mutex")]
#[test_case("academic/dining/dining3_seq.mcrl2" ; "dining philosophers")]
#[test_case("academic/hopcroft/hopcroft.mcrl2" ; "hopcroft")]
#[test_case("academic/scheduler/scheduler.mcrl2" ; "scheduler")]
#[test_case("academic/tree/tree.mcrl2" ; "tree")]
#[test_case("academic/producer_consumer/producer_consumer.mcrl2" ; "producer consumer")]
#[test_case("language/divide2_10.mcrl2" ; "divide")]
#[test_case("language/delta.mcrl2" ; "deadlock")]
#[cfg_attr(miri, ignore)] // Too slow under miri.
fn test_mcrl2_explore_explicit(example: &str) {
    let Some(example) = Mcrl2Example::linearise(example) else {
        return;
    };
    let expected = example.lps2lts();

    for (caching, control_flow) in [
        (CachingStrategy::None, false),
        (CachingStrategy::None, true),
        (CachingStrategy::Local, false),
    ] {
        let lts = explore_lps_explicit(
            &mut builder(),
            example.preprocessed(),
            caching,
            ExplorationStrategy::Dfs,
            control_flow,
            &Timing::new(),
        )
        .unwrap();

        assert_eq!(
            (lts.num_of_states(), lts.num_of_transitions()),
            expected,
            "{caching:?}, control flow {control_flow}"
        );
    }
}

#[test_case("academic/abp/abp.mcrl2" ; "alternating bit protocol")]
#[test_case("academic/dining/dining3_seq.mcrl2" ; "dining philosophers")]
#[cfg_attr(miri, ignore)] // Too slow under miri.
fn test_mcrl2_explore_parallel(example: &str) {
    let Some(example) = Mcrl2Example::linearise(example) else {
        return;
    };
    let (_, transitions) = example.lps2lts();

    // The state indices of a parallel exploration are sparse, so only the transitions are counted.
    let lts = explore_lps_explicit_parallel(
        &mut MutexLtsBuilder::new(builder()),
        example.preprocessed(),
        CachingStrategy::None,
        2,
        false,
        false,
        &Timing::new(),
    )
    .unwrap();

    assert_eq!(lts.num_of_transitions(), transitions);
}

#[test_case("academic/abp/abp.mcrl2" ; "alternating bit protocol")]
#[test_case("academic/dining/dining3_seq.mcrl2" ; "dining philosophers")]
#[cfg_attr(miri, ignore)] // oxidd does not work with miri.
fn test_mcrl2_explore_symbolic(example: &str) {
    let Some(example) = Mcrl2Example::linearise(example) else {
        return;
    };
    let (states, _) = example.lps2lts();

    let storage = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
    let result = explore_lps_symbolic(
        &storage,
        example.preprocessed(),
        &SymbolicLpsOptions::default(),
        &ReachabilityOptions::default(),
        &Timing::new(),
        |_, _| ControlFlow::Continue(()),
    )
    .unwrap();

    assert_eq!(
        ldd_len(&result.states, &mut LddLenCache::new()).exact(),
        Some(states as _)
    );
}

#[test]
#[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
fn test_mcrl2_higher_order_data_fails_in_the_rewriter() {
    let Some(example) = Mcrl2Example::linearise("academic/peterson_justness/mutex.mcrl2") else {
        return;
    };

    let lps = TransformedLps::new(&example.lps).unwrap();
    let payload = catch_unwind(AssertUnwindSafe(|| preprocess(lps, DEFAULT_PREPROCESSING)))
        .err()
        .expect("the rewriter should panic on higher order data");
    assert!(panic_message(payload).contains("occurs with different arities"));
}

#[test]
#[cfg_attr(miri, ignore)] // Too slow under miri.
fn test_mcrl2_preprocess_records_transformations() {
    let Some(example) = Mcrl2Example::linearise("academic/abp/abp.mcrl2") else {
        return;
    };
    let expected = example.lps2lts();

    let lps = example.preprocessed();
    let kinds: Vec<LpsTransformation> = lps.transformations().iter().map(AppliedTransformation::kind).collect();
    assert_eq!(kinds, DEFAULT_PREPROCESSING);

    // Applying the transformations again records them again, and keeps the state space the same.
    let again = preprocess(lps, DEFAULT_PREPROCESSING).unwrap();
    assert_eq!(again.transformations().len(), 2 * DEFAULT_PREPROCESSING.len());
    for control_flow in [false, true] {
        assert_eq!(explore(again.clone(), control_flow).unwrap(), expected);
    }
}

/// A variable that is already called like the first constant must not be captured by it.
#[test]
#[cfg_attr(miri, ignore)] // Too slow under miri.
fn test_mcrl2_replaced_constants_do_not_clash() {
    let Some(example) = Mcrl2Example::linearise("academic/abp/abp.mcrl2") else {
        return;
    };

    let mut lps = example.lps;
    let parameter = DataVariable::with_sort("@rewr_var0", lps.process_parameters[0].sort());
    lps.process_parameters.push(parameter.clone());
    lps.initial_state.push(lps.initial_state[0].clone());
    for summand in &mut lps.action_summands {
        summand.next_state.push(None);
    }

    let lps = preprocess(TransformedLps::new(&lps).unwrap(), DEFAULT_PREPROCESSING).unwrap();
    assert!(lps.constant_assignments().next().is_some());
    assert!(
        lps.constant_assignments()
            .all(|(variable, _)| variable.name() != "@rewr_var0"),
        "a constant reuses the name of a process parameter"
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Too slow under miri.
#[cfg_attr(not(debug_assertions), ignore)] // The shape of the LPS is only checked in debug builds.
fn test_mcrl2_one_point_rule_requires_instantiated_global_variables() {
    let Some(example) = Mcrl2Example::linearise("academic/abp/abp.mcrl2") else {
        return;
    };

    let lps = TransformedLps::new(&example.lps).unwrap();
    let payload = catch_unwind(AssertUnwindSafe(|| {
        lps.transform(LpsTransformation::OnePointRuleRewrite)
    }))
    .err()
    .expect("the one point rule should check that there are no global variables");
    assert!(panic_message(payload).contains("requires the global variables to be instantiated"));
}

#[test]
#[cfg_attr(miri, ignore)] // Too slow under miri.
fn test_mcrl2_global_variables_are_not_explored() {
    let Some(example) = Mcrl2Example::linearise("academic/abp/abp.mcrl2") else {
        return;
    };

    assert!(!example.lps.global_variables.is_empty());
    let result = explore(TransformedLps::new(&example.lps).unwrap(), false);
    assert!(result.is_err());
}

/// Explores `lps` as given, returning the number of states and transitions.
fn explore(lps: TransformedLps, control_flow: bool) -> Result<(usize, usize), MercError> {
    let lts = explore_lps_explicit(
        &mut builder(),
        lps,
        CachingStrategy::None,
        ExplorationStrategy::Dfs,
        control_flow,
        &Timing::new(),
    )?;
    Ok((lts.num_of_states(), lts.num_of_transitions()))
}

/// The LPS `act a; proc P(parameter) = summand; init P(initial);` over `data_specification`.
fn single_summand(
    data_specification: Mcrl2DataSpecification,
    parameter: DataVariable,
    summand: (Vec<DataVariable>, DataExpression, Option<DataExpression>),
    initial: DataExpression,
) -> LinearProcessSpecification {
    let label = ActionLabel {
        name: ATermString::new("a"),
        sorts: Vec::new(),
    };
    let (summation_variables, condition, value) = summand;
    LinearProcessSpecification {
        data_specification,
        action_labels: vec![label.clone()],
        global_variables: Vec::new(),
        process_parameters: vec![parameter],
        action_summands: vec![ActionSummand {
            summation_variables,
            condition,
            action: MultiAction {
                actions: vec![Action {
                    label,
                    arguments: Vec::new(),
                }],
                time: None,
            },
            next_state: vec![value],
        }],
        deadlock_summands: Vec::new(),
        initial_state: vec![initial],
    }
}

/// `proc P(d: Bool) = sum d: Bool. d -> a . P(d = d); init P(false);`, where the summation variable
/// is the same term as the parameter and hence shadows it. Without preprocessing nothing renames it.
#[test]
#[cfg_attr(miri, ignore)] // Too slow under miri.
fn test_summation_variable_shadows_parameter() {
    let d = DataVariable::with_sort("d", SortExpression::bool().copy());
    let value = DataExpression::from(d.clone());
    let lps = single_summand(
        Mcrl2DataSpecification::default(),
        d.clone(),
        (vec![d], value.clone(), Some(value)),
        DataExpression::bool_literal(false),
    );

    for control_flow in [false, true] {
        assert_eq!(
            explore(TransformedLps::new(&lps).unwrap(), control_flow).unwrap(),
            (2, 2)
        );
    }
}

/// `proc P(d: Bool) = sum d: Bool. d -> a . P(); init P(false);`, in which the unassigned parameter
/// keeps its value although a summation variable has the same name.
#[test_case(false ; "as given")]
#[test_case(true ; "preprocessed")]
#[cfg_attr(miri, ignore)] // Too slow under miri.
fn test_unassigned_parameter_is_not_captured_by_a_summation_variable(with_preprocessing: bool) {
    let d = DataVariable::with_sort("d", SortExpression::bool().copy());
    let lps = single_summand(
        Mcrl2DataSpecification::default(),
        d.clone(),
        (vec![d.clone()], d.into(), None),
        DataExpression::bool_literal(false),
    );

    let mut bytes = Vec::new();
    write_lps(&lps, &mut bytes).unwrap();
    let mut lps = TransformedLps::new(&read_lps(&bytes[..]).unwrap()).unwrap();
    if with_preprocessing {
        lps = preprocess(lps, DEFAULT_PREPROCESSING).unwrap();
    }
    assert_eq!(explore(lps, false).unwrap(), (1, 1));
}

/// `proc P(d: Bool) = sum d: Bool. (d == true) -> a . P(d = false); init P(false);`, in which the
/// equality is about the summation variable, not about the parameter.
#[test]
#[cfg_attr(miri, ignore)] // Too slow under miri.
fn test_control_flow_pruning_respects_shadowing() {
    let d = DataVariable::with_sort("d", SortExpression::bool().copy());
    let condition = DataExpression::equal(
        &SortExpression::bool(),
        &d.clone().into(),
        &DataExpression::bool_literal(true),
    );
    let lps = single_summand(
        Mcrl2DataSpecification::default(),
        d.clone(),
        (vec![d], condition, Some(DataExpression::bool_literal(false))),
        DataExpression::bool_literal(false),
    );
    assert!(lps.has_name_clashes());
    let (resolved, _) = resolve_name_clashes(lps.clone());
    assert!(!resolved.has_name_clashes());

    for control_flow in [false, true] {
        assert_eq!(
            explore(TransformedLps::new(&lps).unwrap(), control_flow).unwrap(),
            (1, 1)
        );
    }
}

/// `sort D; map a, b: D; proc P(d: D) = (d == a) -> a . P(d = a); init P(b);`, with the equations
/// `a == b = true; b == a = true;` when `equality` holds.
fn lps_over_two_constants(equality: bool) -> LinearProcessSpecification {
    let basic = BasicSort::new("D");
    let sort: SortExpression = basic.clone().into();
    let a = DataFunctionSymbol::with_sort("a", sort.copy());
    let b = DataFunctionSymbol::with_sort("b", sort.copy());
    let (a_value, b_value): (DataExpression, DataExpression) = (a.clone().into(), b.clone().into());
    let equations = if equality {
        vec![
            DataEquation::new(
                &[],
                None,
                DataExpression::equal(&sort, &a_value, &b_value),
                DataExpression::bool_literal(true),
            ),
            DataEquation::new(
                &[],
                None,
                DataExpression::equal(&sort, &b_value, &a_value),
                DataExpression::bool_literal(true),
            ),
        ]
    } else {
        Vec::new()
    };

    let d = DataVariable::with_sort("d", sort.copy());
    single_summand(
        Mcrl2DataSpecification::new(vec![basic], Vec::new(), Vec::new(), vec![a, b], equations),
        d.clone(),
        (
            Vec::new(),
            DataExpression::equal(&sort, &d.into(), &a_value),
            Some(a_value.clone()),
        ),
        b_value,
    )
}

/// With the equations `a == b = true; b == a = true;`, `==` is coarser than syntactic equality: `a`
/// and `b` are the same location of the control flow parameter `d`.
#[test]
#[cfg_attr(miri, ignore)] // Too slow under miri.
fn test_control_flow_pruning_respects_user_defined_equality() {
    let lps = lps_over_two_constants(true);
    let cfg = CfgLinearProcessSpecification::new(TransformedLps::new(&lps).unwrap()).unwrap();
    assert_eq!(cfg.control_flow_parameters(), [0]);

    for control_flow in [false, true] {
        assert_eq!(
            explore(TransformedLps::new(&lps).unwrap(), control_flow).unwrap(),
            (2, 2)
        );
    }
}

/// Without equations, `a == b` does not rewrite to `true` or `false`, so the locations of `d` are
/// unknown and `d` is not a control flow parameter.
#[test]
#[cfg_attr(miri, ignore)] // Too slow under miri.
fn test_undecided_locations_are_not_control_flow() {
    let lps = lps_over_two_constants(false);
    let cfg = CfgLinearProcessSpecification::new(TransformedLps::new(&lps).unwrap()).unwrap();
    assert!(cfg.control_flow_parameters().is_empty());
}
