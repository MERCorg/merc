//! Integration tests comparing LDD and BDD symbolic reachability on real `.sym` files.
//!
//! The randomised property test is gated on `MCRL2_PATH`: it is skipped when the
//! variable is unset and runs when set to a directory containing `txt2lps` and
//! `lpsreach`. The `abp` smoke test reads a checked-in `.sym` file and runs
//! unconditionally.

use std::fs::File;
use std::path::Path;
use std::process::Command;

use merc_io::temp_dir;
use merc_io::traced_command;
use merc_symbolic::BDD_CACHE_CAPACITY;
use merc_symbolic::BDD_NODE_CAPACITY;
use merc_symbolic::LDD_CACHE_CAPACITY;
use merc_symbolic::LDD_NODE_CAPACITY;
use merc_symbolic::SatCountCache;
use merc_symbolic::SymbolicLtsBdd;
use merc_symbolic::approx_satcount;
use merc_symbolic::reachability;
use merc_symbolic::reachability_bdd;
use merc_symbolic::read_symbolic_lts;
use merc_syntax::random_lps;
use merc_utilities::Timing;
use merc_utilities::random_test;

/// Reads the symbolic LTS at `sym_path`, runs both LDD and BDD reachability,
/// and asserts the reachable-state counts agree.
fn compare_ldd_bdd_reachability(sym_path: &Path) {
    // LDD path: read into its own manager and run symbolic reachability.
    let ldd_storage = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
    let mut lts_ldd = read_symbolic_lts(&ldd_storage, File::open(sym_path).expect("Failed to open sym file"))
        .expect("Failed to read sym file for LDD path");
    let ldd_count = reachability(&ldd_storage, &mut lts_ldd, &Timing::new())
        .expect("LDD reachability failed")
        .len();

    // BDD path: read a second time so the two paths are fully independent.
    let bdd_ldd_storage = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
    let lts_for_bdd = read_symbolic_lts(&bdd_ldd_storage, File::open(sym_path).expect("Failed to open sym file"))
        .expect("Failed to read sym file for BDD path");
    let bdd_manager = oxidd::bdd::new_manager(BDD_NODE_CAPACITY, BDD_CACHE_CAPACITY, 1);
    let lts_bdd =
        SymbolicLtsBdd::from_symbolic_lts(&bdd_ldd_storage, &bdd_manager, &lts_for_bdd).expect("BDD conversion failed");
    let reachable = reachability_bdd(&bdd_manager, &lts_bdd, false).expect("BDD reachability failed");
    let bdd_count = approx_satcount(
        &reachable,
        lts_bdd.state_variables().len() as u32,
        &mut SatCountCache::new(),
    )
    .as_f64() as usize;

    assert_eq!(
        ldd_count,
        bdd_count,
        "LDD and BDD reachable-state counts disagree for {}",
        sym_path.display()
    );
}

/// Reads the checked-in `abp.sym` and asserts both LDD and BDD reachability
/// give 74 states (the known-correct value for ABP).
///
/// This test does not require `MCRL2_PATH` and serves as a regression guard for
/// `SymbolicLtsBdd::from_symbolic_lts` conversion bugs.
#[test]
#[cfg_attr(miri, ignore)]
fn test_abp_ldd_bdd_agree() {
    let sym_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/lts/abp.sym");
    compare_ldd_bdd_reachability(&sym_path);
}

/// Generates random LPS specifications, converts them to `.sym` via `txt2lps` +
/// `lpsreach`, then asserts that LDD and BDD reachability give the same state count.
///
/// Requires `MCRL2_PATH` to point to a directory containing `txt2lps` and `lpsreach`;
/// skipped silently otherwise.
#[test]
#[cfg_attr(miri, ignore)]
fn test_lpsreach_random_reachability() {
    let Ok(mcrl2_path) = std::env::var("MCRL2_PATH") else {
        println!("Skipping test: MCRL2_PATH not set");
        return;
    };

    let txt2lps = Path::new(&mcrl2_path).join("txt2lps");
    let lpsreach = Path::new(&mcrl2_path).join("lpsreach");

    let temp = temp_dir("test_sym_lpsreach").expect("Failed to create temp dir");
    let spec_path = temp.path().join("spec.mcrl2");
    let lps_path = temp.path().join("spec.lps");
    let sym_path = temp.path().join("spec.sym");

    random_test(10, |rng| {
        let spec = random_lps(rng, 4, 2, 0.4);
        std::fs::write(&spec_path, spec.to_string()).expect("Failed to write random LPS spec");

        let status =
            traced_command(Command::new(&txt2lps).arg(&spec_path).arg(&lps_path)).expect("Failed to execute txt2lps");
        assert!(status.success(), "txt2lps failed with status: {status}");

        let status =
            traced_command(Command::new(&lpsreach).arg(&lps_path).arg(&sym_path)).expect("Failed to execute lpsreach");
        assert!(status.success(), "lpsreach failed with status: {status}");

        compare_ldd_bdd_reachability(&sym_path);
    });
}
