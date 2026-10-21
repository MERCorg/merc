use std::fs::File;
use std::io::Cursor;
use std::path::Path;
use std::process::Command;

use mcrl2::LinearProcessSpecification;
use mcrl2::PreprocessOptions;
use mcrl2::preprocess;
use mcrl2::read_lps;
use merc_explore::CachingStrategy;
use merc_explore::ExplorationStrategy;
use merc_io::temp_dir;
use merc_io::traced_command;
use merc_lts::AutStream;
use merc_lts::LTS;
use merc_lts::LtsBuilderMem;
use merc_lts::LtsMultiAction;
use merc_lts::MutexLtsBuilder;
use merc_lts::SimpleAction;
use merc_lts::StateIndex;
use merc_lts::read_lts;
use merc_lts::read_mcrl2_aut;
use merc_lts::write_lts;
use merc_lts::write_mcrl2_aut;
use merc_reduction::Equivalence;
use merc_reduction::compare_lts;
use merc_syntax::random_lps;
use merc_utilities::Timing;
use merc_utilities::random_test;

use merc_lps::Mcrl2MultiActionLabel;
use merc_lps::convert_data_specification;
use merc_lps::explore_lps_explicit;
use merc_lps::explore_lps_explicit_parallel;

/// Reads the LPS at `lps_path` and preprocesses it exactly as the `merc-lps`
/// tool does, since the explorers themselves take the LPS as given. Each
/// exploration consumes its own LPS, so this is called once per exploration.
fn read_preprocessed_lps(lps_path: &Path) -> LinearProcessSpecification {
    let lps = read_lps(lps_path.to_str().unwrap()).expect("Failed to read LPS");
    preprocess(&lps, &PreprocessOptions::default()).expect("Failed to preprocess LPS")
}

/// Runs `mcrl22lps` and `lps2lts` on a `.mcrl2` specification, explores the
/// LPS with `explore_lps_explicit`, and asserts strong bisimilarity between
/// the two resulting LTSs.
fn compare_with_lps2lts(spec_relative_path: &str) {
    compare_with_lps2lts_caching(spec_relative_path, CachingStrategy::None);
}

/// Like [`compare_with_lps2lts`] but explores the LPS with the given
/// [`CachingStrategy`].
fn compare_with_lps2lts_caching(spec_relative_path: &str, strategy: CachingStrategy) {
    let Ok(mcrl2_path) = std::env::var("MCRL2_PATH") else {
        println!("Skipping test: MCRL2_PATH not set");
        return;
    };

    let mcrl22lps = Path::new(&mcrl2_path).join("mcrl22lps");
    let lps2lts = Path::new(&mcrl2_path).join("lps2lts");

    let spec_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(spec_relative_path);
    assert!(spec_path.exists(), "Spec file not found: {}", spec_path.display());

    let temp_dir = temp_dir("test_explore_lps").unwrap();
    let lps_path = temp_dir.path().join("spec.lps");
    let aut_path = temp_dir.path().join("reference.aut");

    let status =
        traced_command(Command::new(&mcrl22lps).arg(&spec_path).arg(&lps_path)).expect("Failed to execute mcrl22lps");
    assert!(status.success(), "mcrl22lps failed with status: {status}");

    let status =
        traced_command(Command::new(&lps2lts).arg(&lps_path).arg(&aut_path)).expect("Failed to execute lps2lts");
    assert!(status.success(), "lps2lts failed with status: {status}");

    // Parse the labels as multi-actions (a multiset of actions) rather than
    // comparing the pretty-printed strings verbatim: mCRL2 does not
    // guarantee a canonical order for the `|`-separated actions of a
    // multi-action, so two equivalent multi-actions (e.g. reached via
    // different summands, or printed by a different tool) can be printed
    // with their actions in a different order.
    let reference_lts = read_mcrl2_aut(File::open(&aut_path).unwrap())
        .expect("Failed to read reference .aut")
        .relabel(|label| LtsMultiAction::<SimpleAction>::from_string(&label))
        .unwrap();

    let mut builder: LtsBuilderMem<Mcrl2MultiActionLabel> = LtsBuilderMem::new(Vec::new(), Vec::new());
    explore_lps_explicit(
        &mut builder,
        read_preprocessed_lps(&lps_path),
        strategy,
        ExplorationStrategy::Dfs,
        false,
        &Timing::new(),
    )
    .expect("Failed to explore LPS");
    let result_lts = builder.finish(StateIndex::new(0), false);

    write_mcrl2_aut(
        &mut File::create(temp_dir.path().join("result.aut")).unwrap(),
        &result_lts,
    )
    .expect("Failed to write result .aut");

    assert_eq!(
        reference_lts.num_of_states(),
        result_lts.num_of_states(),
        "State count mismatch for {spec_relative_path} with {strategy:?}"
    );
    assert_eq!(
        reference_lts.num_of_transitions(),
        result_lts.num_of_transitions(),
        "Transition count mismatch for {spec_relative_path} with {strategy:?}"
    );
    assert!(
        compare_lts(
            Equivalence::StrongBisim,
            reference_lts,
            result_lts
                .relabel(|label| LtsMultiAction::<SimpleAction>::from_string(&label.to_string()))
                .unwrap(),
            false,
            false,
            &Timing::new(),
        )
        .0,
        "LTSs are not strongly bisimilar for {spec_relative_path} with {strategy:?}"
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_mcrl2_explore_abp() {
    compare_with_lps2lts("../../../../examples/mCRL2/academic/abp/abp.mcrl2");
}

/// Explores ABP explicitly, writes the result through the typed
/// `to_lts_multi_action`/[`convert_data_specification`] path into the binary mCRL2 `.lts`
/// format, reads it back, and checks it is strongly bisimilar to the `lps2lts` reference —
/// exercising the same round trip `merc-lps explore-explicit --out-format lts` performs.
#[test]
#[cfg_attr(miri, ignore)]
fn test_mcrl2_explore_abp_lts_format() {
    let Ok(mcrl2_path) = std::env::var("MCRL2_PATH") else {
        println!("Skipping test: MCRL2_PATH not set");
        return;
    };

    let mcrl22lps = Path::new(&mcrl2_path).join("mcrl22lps");
    let lps2lts = Path::new(&mcrl2_path).join("lps2lts");

    let spec_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../examples/mCRL2/academic/abp/abp.mcrl2");
    let temp_dir = temp_dir("test_explore_lps_lts_format").unwrap();
    let lps_path = temp_dir.path().join("spec.lps");
    let aut_path = temp_dir.path().join("reference.aut");

    let status =
        traced_command(Command::new(&mcrl22lps).arg(&spec_path).arg(&lps_path)).expect("Failed to execute mcrl22lps");
    assert!(status.success(), "mcrl22lps failed with status: {status}");

    let status =
        traced_command(Command::new(&lps2lts).arg(&lps_path).arg(&aut_path)).expect("Failed to execute lps2lts");
    assert!(status.success(), "lps2lts failed with status: {status}");

    let reference_lts = read_mcrl2_aut(File::open(&aut_path).unwrap())
        .expect("Failed to read reference .aut")
        .relabel(|label| LtsMultiAction::<SimpleAction>::from_string(&label))
        .unwrap();

    // The data specification never changes during exploration, so convert it from a
    // separate read of the LPS, before the other read is consumed by the explorer.
    let data_spec = convert_data_specification(&read_preprocessed_lps(&lps_path));

    let mut builder: LtsBuilderMem<Mcrl2MultiActionLabel> = LtsBuilderMem::new(Vec::new(), Vec::new());
    let explored_lts = explore_lps_explicit(
        &mut builder,
        read_preprocessed_lps(&lps_path),
        CachingStrategy::None,
        ExplorationStrategy::Dfs,
        false,
        &Timing::new(),
    )
    .expect("Failed to explore LPS")
    .relabel(|label| label.to_lts_multi_action())
    .expect("Failed to convert mCRL2 multi-action labels to typed LTS multi-actions");

    let mut buffer: Vec<u8> = Vec::new();
    write_lts(&mut buffer, &explored_lts, &data_spec).expect("Failed to write .lts");
    let (result_lts, _data_spec) = read_lts(&buffer[..], false).expect("Failed to read back .lts");

    assert_eq!(
        reference_lts.num_of_states(),
        result_lts.num_of_states(),
        "State count mismatch after .lts round trip"
    );
    assert_eq!(
        reference_lts.num_of_transitions(),
        result_lts.num_of_transitions(),
        "Transition count mismatch after .lts round trip"
    );
    assert!(
        compare_lts(
            Equivalence::StrongBisim,
            reference_lts,
            result_lts
                .with_state_labels(())
                .relabel(|label| LtsMultiAction::<SimpleAction>::from_string(&label.to_string()))
                .unwrap(),
            false,
            false,
            &Timing::new(),
        )
        .0,
        "LTSs are not strongly bisimilar after .lts round trip"
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_mcrl2_explore_cabp() {
    compare_with_lps2lts("../../../../examples/mCRL2/academic/cabp/cabp.mcrl2");
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_mcrl2_explore_allow() {
    compare_with_lps2lts("../../../../examples/mCRL2/academic/allow/allow.mcrl2");
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_mcrl2_explore_block() {
    compare_with_lps2lts("../../../../examples/mCRL2/academic/block/block.mcrl2");
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_mcrl2_explore_dining3() {
    compare_with_lps2lts("../../../../examples/mCRL2/academic/dining/dining3.mcrl2");
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_mcrl2_explore_par() {
    compare_with_lps2lts("../../../../examples/mCRL2/academic/par/par.mcrl2");
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_mcrl2_explore_abp_local_cache() {
    compare_with_lps2lts_caching(
        "../../../../examples/mCRL2/academic/abp/abp.mcrl2",
        CachingStrategy::Local,
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_mcrl2_explore_cabp_local_cache() {
    compare_with_lps2lts_caching(
        "../../../../examples/mCRL2/academic/cabp/cabp.mcrl2",
        CachingStrategy::Local,
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_mcrl2_explore_dining3_local_cache() {
    compare_with_lps2lts_caching(
        "../../../../examples/mCRL2/academic/dining/dining3.mcrl2",
        CachingStrategy::Local,
    );
}

/// Generates random LPS specs using [`random_lps`], explores them with
/// [`explore_lps_explicit`], and asserts strong bisimilarity against `lps2lts`.
///
/// Random specs are written to a temp file and converted with `txt2lps` (no
/// linearisation step required for the FSM-shaped output of [`random_lps`]).
fn compare_random_lps_with_lps2lts(strategy: CachingStrategy) {
    let Ok(mcrl2_path) = std::env::var("MCRL2_PATH") else {
        println!("Skipping test: MCRL2_PATH not set");
        return;
    };

    let txt2lps = Path::new(&mcrl2_path).join("txt2lps");
    let lps2lts = Path::new(&mcrl2_path).join("lps2lts");

    let temp_dir = temp_dir("test_explore_random_lps").unwrap();
    let spec_path = temp_dir.path().join("spec.mcrl2");
    let lps_path = temp_dir.path().join("spec.lps");
    let aut_path = temp_dir.path().join("reference.aut");

    random_test(20, |rng| {
        let spec = random_lps(rng, 5, 3, 0.4);
        std::fs::write(&spec_path, spec.to_string()).expect("Failed to write random LPS spec");

        let status =
            traced_command(Command::new(&txt2lps).arg(&spec_path).arg(&lps_path)).expect("Failed to execute txt2lps");
        assert!(status.success(), "txt2lps failed with status: {status}");

        let status =
            traced_command(Command::new(&lps2lts).arg(&lps_path).arg(&aut_path)).expect("Failed to execute lps2lts");
        assert!(status.success(), "lps2lts failed with status: {status}");

        let reference_lts = read_mcrl2_aut(File::open(&aut_path).unwrap()).expect("Failed to read reference .aut");
        let mut builder: LtsBuilderMem<Mcrl2MultiActionLabel> = LtsBuilderMem::new(Vec::new(), Vec::new());
        let result_lts = explore_lps_explicit(
            &mut builder,
            read_preprocessed_lps(&lps_path),
            strategy,
            ExplorationStrategy::Dfs,
            false,
            &Timing::new(),
        )
        .expect("Failed to explore LPS");

        assert_eq!(
            reference_lts.num_of_states(),
            result_lts.num_of_states(),
            "State count mismatch with {strategy:?}"
        );
        assert_eq!(
            reference_lts.num_of_transitions(),
            result_lts.num_of_transitions(),
            "Transition count mismatch with {strategy:?}"
        );
        assert!(
            compare_lts(
                Equivalence::StrongBisim,
                reference_lts,
                result_lts.relabel(|label| { Ok(label.to_string()) }).unwrap(),
                false,
                false,
                &Timing::new(),
            )
            .0,
            "LTSs are not strongly bisimilar with {strategy:?}"
        );
    });
}

/// Explores `lps_path` both sequentially and with the parallel work-stealing
/// search on several threads, and asserts the two LTSs are strongly
/// bisimilar. The parallel explorer numbers states sparsely (see the
/// assertion below), so its LTS carries extra unreachable deadlock states and
/// the comparison is only up to bisimulation rather than on exact counts.
fn assert_parallel_matches_sequential(lps_path: &Path) {
    // Sequential reference, relabelled to strings for comparison.
    let mut builder: LtsBuilderMem<Mcrl2MultiActionLabel> = LtsBuilderMem::new(Vec::new(), Vec::new());
    explore_lps_explicit(
        &mut builder,
        read_preprocessed_lps(lps_path),
        CachingStrategy::None,
        ExplorationStrategy::Bfs,
        false,
        &Timing::new(),
    )
    .expect("Sequential exploration failed");
    let sequential = builder
        .finish(StateIndex::new(0), false)
        .relabel(|label| Ok(label.to_string()))
        .unwrap();

    // Parallel exploration across several threads, streamed into an
    // in-memory AUT buffer (guarded by a `MutexLtsBuilder`) and read back as
    // a string-labelled LTS.
    let mut buffer = Cursor::new(Vec::new());
    {
        let mut builder = MutexLtsBuilder::new(AutStream::new_mcrl2(&mut buffer).unwrap());
        explore_lps_explicit_parallel(
            &mut builder,
            read_preprocessed_lps(lps_path),
            CachingStrategy::None,
            4,
            false,
            false,
            &Timing::new(),
        )
        .expect("Parallel exploration failed");
    }
    buffer.set_position(0);
    let parallel = read_mcrl2_aut(&mut buffer).expect("Failed to read parallel AUT output");

    // The parallel explorer numbers states sparsely (see `explore_parallel`),
    // so its LTS has extra unreachable deadlock states: compare up to
    // bisimulation rather than on exact state counts.
    assert!(
        compare_lts(
            Equivalence::StrongBisim,
            parallel,
            sequential,
            false,
            false,
            &Timing::new()
        )
        .0,
        "Parallel and sequential LTSs are not strongly bisimilar for {}",
        lps_path.display()
    );
}

/// Runs `mcrl22lps` on a `.mcrl2` specification and asserts that parallel and
/// sequential explicit exploration agree.
fn compare_parallel_with_sequential(spec_relative_path: &str) {
    let Ok(mcrl2_path) = std::env::var("MCRL2_PATH") else {
        println!("Skipping test: MCRL2_PATH not set");
        return;
    };

    let mcrl22lps = Path::new(&mcrl2_path).join("mcrl22lps");
    let spec_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(spec_relative_path);
    assert!(spec_path.exists(), "Spec file not found: {}", spec_path.display());

    let temp_dir = temp_dir("test_explore_lps_parallel").unwrap();
    let lps_path = temp_dir.path().join("spec.lps");

    let status =
        traced_command(Command::new(&mcrl22lps).arg(&spec_path).arg(&lps_path)).expect("Failed to execute mcrl22lps");
    assert!(status.success(), "mcrl22lps failed with status: {status}");

    assert_parallel_matches_sequential(&lps_path);
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_mcrl2_explore_parallel_abp() {
    compare_parallel_with_sequential("../../../../examples/mCRL2/academic/abp/abp.mcrl2");
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_mcrl2_explore_parallel_cabp() {
    compare_parallel_with_sequential("../../../../examples/mCRL2/academic/cabp/cabp.mcrl2");
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_mcrl2_explore_parallel_dining3() {
    compare_parallel_with_sequential("../../../../examples/mCRL2/academic/dining/dining3.mcrl2");
}

/// Generates random LPS specs with [`random_lps`] and asserts that parallel
/// and sequential exploration agree on each.
#[test]
#[cfg_attr(miri, ignore)]
fn test_mcrl2_explore_parallel_random_lps() {
    let Ok(mcrl2_path) = std::env::var("MCRL2_PATH") else {
        println!("Skipping test: MCRL2_PATH not set");
        return;
    };

    let txt2lps = Path::new(&mcrl2_path).join("txt2lps");

    let temp_dir = temp_dir("test_explore_random_lps_parallel").unwrap();
    let spec_path = temp_dir.path().join("spec.mcrl2");
    let lps_path = temp_dir.path().join("spec.lps");

    random_test(20, |rng| {
        let spec = random_lps(rng, 5, 3, 0.4);
        std::fs::write(&spec_path, spec.to_string()).expect("Failed to write random LPS spec");

        let status =
            traced_command(Command::new(&txt2lps).arg(&spec_path).arg(&lps_path)).expect("Failed to execute txt2lps");
        assert!(status.success(), "txt2lps failed with status: {status}");

        assert_parallel_matches_sequential(&lps_path);
    });
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_mcrl2_random_lps() {
    compare_random_lps_with_lps2lts(CachingStrategy::None);
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_mcrl2_random_lps_local_cache() {
    compare_random_lps_with_lps2lts(CachingStrategy::Local);
}
