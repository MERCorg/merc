use log::debug;
use oxidd::ldd::LDDFunction;
use oxidd::ldd::LDDManagerRef;

use mcrl2::ATerm as Mcrl2ATerm;
use mcrl2::LinearProcessSpecification;
use mcrl2::mcrl2_aterm_to_merc;
use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_lts::LtsAction;
use merc_lts::LtsMultiAction;
use merc_symbolic::ReachabilityOptions;
use merc_symbolic::ReachabilityResult;
use merc_symbolic::SummandGroup;
use merc_symbolic::SymbolicLPS;
use merc_symbolic::SymbolicLps;
use merc_symbolic::SymbolicLpsOptions;
use merc_symbolic::SymbolicLts;
use merc_symbolic::TransitionGroup;
use merc_symbolic::reachability_with_options;
use merc_utilities::MercError;
use merc_utilities::Timing;

use crate::explore_explicit::ExplicitLinearProcessSpecification;

/// Explore the linear process specification using symbolic reachability.
///
/// The summand machinery (read/write positions, condition enumeration) is reused
/// from the explicit [`ExplicitLinearProcessSpecification`] via the generic
/// [`SymbolicLps`] adapter, so LPS and PBES symbolic exploration share one
/// implementation. The `encoding` decides how the summands are distributed over
/// the transition groups and in which order their parameters are stored,
/// mirroring the `--groups` and `--reorder` options of mCRL2's `lpsreach`.
///
/// The LPS is explored as given: any preprocessing (see [`mcrl2::preprocess`])
/// is the caller's responsibility.
///
/// This only reports the reachable state (and, when requested, deadlock)
/// count; it does not retain the transition relation or action labels
/// discovered along the way. Use [`explore_lps_symbolic_to_sym`] to keep those
/// and write a real symbolic LTS.
pub fn explore_lps_symbolic(
    storage: &LDDManagerRef,
    lps: LinearProcessSpecification,
    encoding: &SymbolicLpsOptions,
    options: &ReachabilityOptions,
    timing: &Timing,
) -> Result<ReachabilityResult, MercError> {
    let lps = ExplicitLinearProcessSpecification::new(lps)?;
    let mut symbolic = SymbolicLps::with_options(storage, lps, encoding)?;

    debug!("{symbolic:?}");

    let mut context = symbolic.create_context();
    reachability_with_options(storage, &mut symbolic, &mut context, options, timing)
}

/// Explore the linear process specification using symbolic reachability and
/// assemble the result into a [`SymbolicLts`] carrying the real data
/// specification, process parameters, parameter values, action labels and
/// transition relation — suitable for [`merc_symbolic::write_symbolic_lts`].
///
/// Unlike [`explore_lps_symbolic`], this converts every mCRL2 FFI term it
/// touches (the data specification's declarations, the process parameters,
/// the observed parameter values, the action labels) into the pure-Rust
/// `merc_aterm`/`merc_data` representation via [`mcrl2::mcrl2_aterm_to_merc`],
/// so the result no longer depends on the mCRL2 C++ term pool.
pub fn explore_lps_symbolic_to_sym(
    storage: &LDDManagerRef,
    lps: LinearProcessSpecification,
    encoding: &SymbolicLpsOptions,
    options: &ReachabilityOptions,
    timing: &Timing,
) -> Result<(SymbolicLts<LtsMultiAction<LtsAction>>, Option<LDDFunction>), MercError> {
    let lps = ExplicitLinearProcessSpecification::new(lps)?;
    let mut symbolic = SymbolicLps::with_options(storage, lps, encoding)?;

    debug!("{symbolic:?}");

    let mut context = symbolic.create_context();
    let result = reachability_with_options(storage, &mut symbolic, &mut context, options, timing)?;

    // Converts a single FFI term into its `merc_aterm` counterpart.
    let convert = |term: Mcrl2ATerm| mcrl2_aterm_to_merc(&term.copy());

    // Same, but for terms that came out of the rewriter/enumerator during exploration.
    let convert_learned = |term: Mcrl2ATerm| mcrl2_aterm_to_merc(&mcrl2::remove_index(&term.copy()).copy());

    let permuted = symbolic.lps();
    let inner = permuted.inner();
    let order = permuted.order();

    // The data specification never changes during exploration, so it is only
    // converted once here rather than tracked incrementally.
    let data_specification = crate::convert_data_specification(inner.lps());

    // `order[i]` is the position of `inner`'s (unpermuted) state vector stored
    // at position `i` of the diagrams `symbolic` builds, so the process
    // parameters in diagram order are `inner.parameters()[order[i]]`.
    let inner_parameters = inner.parameters();
    let process_parameters: Vec<DataVariable> = order
        .iter()
        .map(|&position| DataVariable::from(convert(Mcrl2ATerm::from(inner_parameters[position].clone()))))
        .collect();

    // The values observed for each process parameter, in the dense order the
    // diagrams use, i.e. matching the LDD values stored at that position.
    let mut parameter_values: Vec<Vec<DataExpression>> = Vec::with_capacity(process_parameters.len());
    for column in context.columns() {
        let mut values = Vec::with_capacity(column.len());
        for i in 0..column.len() {
            let index = *column.get_by_index(i).expect("dense index must be present");
            values.push(DataExpression::from(convert_learned(Mcrl2ATerm::from(
                inner.value(index),
            ))));
        }
        parameter_values.push(values);
    }

    // The action labels observed during exploration, again in dense order.
    let labels = context.labels();
    let mut action_labels: Vec<LtsMultiAction<LtsAction>> = Vec::with_capacity(labels.len());
    for i in 0..labels.len() {
        let label = labels.get_by_index(i).expect("dense index must be present");
        action_labels.push(LtsMultiAction::from_mcrl2_aterm(convert_learned(label.as_aterm()))?);
    }

    // Every transition group, translated from diagram positions to the
    // process parameters they read/write.
    let mut summand_groups = Vec::with_capacity(symbolic.transition_groups().len());
    for group in symbolic.transition_groups() {
        let read_parameters: Vec<DataVariable> = group
            .read_indices()
            .iter()
            .map(|&position| process_parameters[position as usize].clone())
            .collect();
        let write_parameters: Vec<DataVariable> = group
            .write_indices()
            .iter()
            .map(|&position| process_parameters[position as usize].clone())
            .collect();

        summand_groups.push(SummandGroup::new(
            storage,
            &process_parameters,
            read_parameters,
            write_parameters,
            group.relation().clone(),
        )?);
    }

    let lts = SymbolicLts::new(
        data_specification,
        process_parameters,
        result.states,
        symbolic.initial_state().clone(),
        summand_groups,
        action_labels,
        parameter_values,
    );

    Ok((lts, result.deadlocks))
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::path::Path;
    use std::process::Command;

    use mcrl2::PreprocessOptions;
    use mcrl2::preprocess;
    use mcrl2::read_lps;
    use merc_io::traced_command;
    use merc_symbolic::BDD_CACHE_CAPACITY;
    use merc_symbolic::BDD_NODE_CAPACITY;
    use merc_symbolic::LDD_CACHE_CAPACITY;
    use merc_symbolic::LDD_NODE_CAPACITY;
    use merc_symbolic::ReachabilityOptions;
    use merc_symbolic::SatCountCache;
    use merc_symbolic::SymbolicLPS;
    use merc_symbolic::SymbolicLpsOptions;
    use merc_symbolic::SymbolicLtsBdd;
    use merc_symbolic::approx_satcount;
    use merc_symbolic::reachability_bdd;
    use merc_symbolic::reachability_with_options;
    use merc_symbolic::read_symbolic_lts;
    use merc_symbolic::write_symbolic_lts;
    use merc_utilities::Timing;
    use merc_utilities::random_test;
    use oxidd::BooleanFunction;

    use mcrl2::DataSpecification;
    use mcrl2::mcrl2_aterm_to_merc;
    use merc_data::BasicSort;
    use merc_data::DataEquation;
    use merc_data::DataFunctionSymbol;
    use merc_data::SortAlias;

    use super::explore_lps_symbolic;
    use super::explore_lps_symbolic_to_sym;

    /// Converts every declaration of a data specification parsed from mCRL2 text into its
    /// `merc_data` counterpart, exercising the same `mcrl2_aterm_to_merc` conversion
    /// [`explore_lps_symbolic_to_sym`] applies to an LPS's data specification — but without needing
    /// an LPS (or `MCRL2_PATH`) at all, since [`DataSpecification::from_string`] uses the mCRL2
    /// library's parser directly.
    ///
    /// Every `merc_data` type produced here is built via a `debug_assert!`-checked `From<ATerm>`
    /// (see the `#[merc_term(..)]` macro in `merc_data`), so a structurally wrong conversion panics
    /// immediately in this (debug) test build.
    #[test]
    fn test_convert_data_specification_from_mcrl2() {
        let spec = DataSpecification::from_string(
            "sort Colour = struct red | green | blue;\n\
             map is_red: Colour -> Bool;\n\
             var c: Colour;\n\
             eqn is_red(c) = c == red;\n",
        );

        let sorts: Vec<BasicSort> = spec
            .user_defined_sorts()
            .to_vec()
            .into_iter()
            .map(|t| BasicSort::from(mcrl2_aterm_to_merc(&t.copy())))
            .collect();
        let aliases: Vec<SortAlias> = spec
            .user_defined_aliases()
            .to_vec()
            .into_iter()
            .map(|t| SortAlias::from(mcrl2_aterm_to_merc(&t.copy())))
            .collect();
        let constructors: Vec<DataFunctionSymbol> = spec
            .user_defined_constructors()
            .to_vec()
            .into_iter()
            .map(|t| DataFunctionSymbol::from(mcrl2_aterm_to_merc(&t.copy())))
            .collect();
        let mappings: Vec<DataFunctionSymbol> = spec
            .user_defined_mappings()
            .to_vec()
            .into_iter()
            .map(|t| DataFunctionSymbol::from(mcrl2_aterm_to_merc(&t.copy())))
            .collect();
        let equations: Vec<DataEquation> = spec
            .user_defined_equations()
            .to_vec()
            .into_iter()
            .map(|t| DataEquation::from(mcrl2_aterm_to_merc(&t.copy())))
            .collect();

        println!("sorts: {sorts:?}");
        for a in &aliases {
            println!("alias: {a}");
        }
        for c in &constructors {
            println!("constructor: {c}");
        }
        for m in &mappings {
            println!("mapping: {m}");
        }
        for e in &equations {
            println!("equation: {e}");
        }

        // `sort Colour = struct ...;` is internally a named alias for an anonymous structured
        // sort, not a `BasicSort` declaration (those are bare `sort Foo;`), so it shows up among
        // the aliases; its constructors are synthesized from that alias rather than separately
        // "user-defined", so only the explicitly declared mapping and equation join it.
        assert!(sorts.is_empty(), "expected no bare sort declarations: {sorts:?}");
        assert_eq!(aliases.len(), 1, "expected the Colour alias: {aliases:?}");
        assert!(
            constructors.is_empty(),
            "expected no separately-declared constructors: {constructors:?}"
        );
        assert!(
            mappings.iter().any(|m| m.to_string().contains("is_red")),
            "expected is_red among the mappings: {mappings:?}"
        );
        assert_eq!(equations.len(), 1, "expected the one declared equation: {equations:?}");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_mcrl2_explore_symbolic_abp() {
        let Ok(mcrl2_path) = std::env::var("MCRL2_PATH") else {
            println!("Skipping test: MCRL2_PATH not set");
            return;
        };

        let mcrl22lps = Path::new(&mcrl2_path).join("mcrl22lps");

        let temp_dir = tempfile::tempdir().unwrap();
        let lps_path = temp_dir.path().join("abp.lps");

        let spec_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../examples/mCRL2/academic/abp/abp.mcrl2");

        // Run mcrl22lps on the ABP example to get an LPS file.
        let status = Command::new(&mcrl22lps)
            .arg(&spec_path)
            .arg(&lps_path)
            .status()
            .expect("Failed to execute mcrl22lps");
        assert!(status.success(), "mcrl22lps failed with status: {status}");

        let lps = read_lps(lps_path.to_str().expect("LPS path is valid UTF-8")).expect("Failed to read LPS");

        // The explorer takes the LPS as given, so preprocess it here exactly as
        // the `merc-lps` tool does.
        let lps = preprocess(&lps, &PreprocessOptions::default()).expect("Failed to preprocess LPS");

        let storage = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let timing = Timing::new();

        let states = explore_lps_symbolic(
            &storage,
            lps,
            &SymbolicLpsOptions::default(),
            &ReachabilityOptions::default(),
            &timing,
        )
        .expect("Failed to explore LPS")
        .states;
        let num_of_states = states.len();

        assert_eq!(
            num_of_states, 74,
            "ABP should have 74 reachable states (see examples/lts/abp.aut)"
        );
    }

    /// Generates random LPS specs, runs `lpsreach` to produce `.sym` files, then compares three
    /// reachability counts against each other:
    ///
    /// 1. merc-lps LDD path: `explore_lps_symbolic` on the `.lps` directly.
    /// 2. lpsreach `.sym` read by merc, LDD reachability via `reachability_with_options`.
    /// 3. lpsreach `.sym` read by merc, BDD reachability via `reachability_bdd`.
    ///
    /// All three must agree on the number of reachable states.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_mcrl2_random_lps_lpsreach() {
        let Ok(mcrl2_path) = std::env::var("MCRL2_PATH") else {
            println!("Skipping test: MCRL2_PATH not set");
            return;
        };

        let txt2lps = Path::new(&mcrl2_path).join("txt2lps");
        let lpsreach = Path::new(&mcrl2_path).join("lpsreach");

        let temp_dir = tempfile::tempdir().unwrap();
        let spec_path = temp_dir.path().join("spec.mcrl2");
        let lps_path = temp_dir.path().join("spec.lps");
        let sym_path = temp_dir.path().join("spec.sym");

        let storage = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let timing = Timing::new();

        random_test(10, |rng| {
            let spec = merc_syntax::random_lps(rng, 4, 2, 0.4);
            std::fs::write(&spec_path, spec.to_string()).expect("Failed to write spec");

            // Compile the random spec to an LPS (no linearisation needed for the FSM-shaped output).
            let status = traced_command(Command::new(&txt2lps).arg(&spec_path).arg(&lps_path))
                .expect("Failed to execute txt2lps");
            assert!(status.success(), "txt2lps failed: {status}");

            // Run lpsreach to produce a .sym file.
            let status = traced_command(Command::new(&lpsreach).arg(&lps_path).arg(&sym_path))
                .expect("Failed to execute lpsreach");
            assert!(status.success(), "lpsreach failed: {status}");

            // 1. merc-lps LDD path: explore the .lps directly via merc's symbolic engine.
            let lps = read_lps(lps_path.to_str().unwrap()).expect("Failed to read LPS");
            let lps = preprocess(&lps, &PreprocessOptions::default()).expect("Failed to preprocess LPS");
            let lps_ldd_count = explore_lps_symbolic(
                &storage,
                lps,
                &SymbolicLpsOptions::default(),
                &ReachabilityOptions::default(),
                &timing,
            )
            .expect("Failed to explore LPS symbolically")
            .states
            .len();

            // 2. lpsreach .sym, LDD reachability.
            let mut sym_lts_ldd = read_symbolic_lts(&storage, File::open(&sym_path).expect("Failed to open .sym"))
                .expect("Failed to read .sym (LDD path)");
            let mut sym_context = sym_lts_ldd.create_context();
            let sym_ldd_count = reachability_with_options(
                &storage,
                &mut sym_lts_ldd,
                &mut sym_context,
                &ReachabilityOptions::default(),
                &timing,
            )
            .expect("Failed to run LDD reachability on .sym")
            .states
            .len();

            // 3. lpsreach .sym, BDD reachability (fresh read since reachability_with_options mutates the LTS).
            let sym_lts_bdd = read_symbolic_lts(&storage, File::open(&sym_path).expect("Failed to open .sym"))
                .expect("Failed to read .sym (BDD path)");
            let bdd_manager = oxidd::bdd::new_manager(BDD_NODE_CAPACITY, BDD_CACHE_CAPACITY, 1);
            let lts_bdd = SymbolicLtsBdd::from_symbolic_lts(&storage, &bdd_manager, &sym_lts_bdd)
                .expect("Failed to convert .sym to BDD");
            assert!(
                lts_bdd.initial_state().satisfiable(),
                "BDD conversion produced an unsatisfiable initial state"
            );
            let reach_bdd = reachability_bdd(&bdd_manager, &lts_bdd, false).expect("Failed to run BDD reachability");
            let sym_bdd_count = approx_satcount(
                &reach_bdd,
                lts_bdd.state_variables().len() as u32,
                &mut SatCountCache::new(),
            )
            .as_f64() as usize;

            // 4. merc-lps LDD path via explore_lps_symbolic_to_sym: assemble a full SymbolicLts
            //    (data specification, process parameters, parameter values, action labels and the
            //    transition relation, all converted from the mCRL2 FFI term pool), write it to a
            //    `.sym` file and read the state count back, to check the assembled `.sym` file is
            //    valid and round-trips to the same reachable set.
            let lps = read_lps(lps_path.to_str().unwrap()).expect("Failed to read LPS");
            let lps = preprocess(&lps, &PreprocessOptions::default()).expect("Failed to preprocess LPS");
            let (written_lts, _deadlocks) = explore_lps_symbolic_to_sym(
                &storage,
                lps,
                &SymbolicLpsOptions::default(),
                &ReachabilityOptions::default(),
                &timing,
            )
            .expect("Failed to explore LPS into a symbolic LTS");

            let written_sym_path = temp_dir.path().join("written.sym");
            write_symbolic_lts(
                &storage,
                File::create(&written_sym_path).expect("Failed to create written.sym"),
                &written_lts,
            )
            .expect("Failed to write the assembled symbolic LTS");

            let mut reread_lts = read_symbolic_lts(
                &storage,
                File::open(&written_sym_path).expect("Failed to open written.sym"),
            )
            .expect("Failed to read the written symbolic LTS back");
            let mut reread_context = reread_lts.create_context();
            let written_sym_count = reachability_with_options(
                &storage,
                &mut reread_lts,
                &mut reread_context,
                &ReachabilityOptions::default(),
                &timing,
            )
            .expect("Failed to run LDD reachability on the written symbolic LTS")
            .states
            .len();

            assert_eq!(
                lps_ldd_count, sym_ldd_count,
                "merc-lps LDD ({lps_ldd_count}) and lpsreach .sym LDD ({sym_ldd_count}) counts disagree"
            );
            assert_eq!(
                sym_ldd_count, sym_bdd_count,
                "lpsreach .sym LDD ({sym_ldd_count}) and BDD ({sym_bdd_count}) counts disagree"
            );
            assert_eq!(
                lps_ldd_count, written_sym_count,
                "merc-lps LDD ({lps_ldd_count}) and the assembled+written .sym ({written_sym_count}) counts disagree"
            );
        });
    }
}
