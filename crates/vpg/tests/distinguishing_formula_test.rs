//! These tests check whether the distinguishing formulas generated when two
//! LTSs are found not to be strongly bisimilar are indeed distinguishing. They
//! are located here to avoid circular dependencies.
use std::io::Write;

use merc_syntax::UntypedDataSpecification;
use merc_syntax::UntypedStateFrmSpec;
use merc_syntax::generate_distinguishing_formula;
use merc_vpg::PG;
use rand::rngs::StdRng;

use merc_io::DumpFiles;
use merc_lts::mutate_lts;
use merc_lts::random_lts;
use merc_lts::write_aut;
use merc_utilities::Timing;
use merc_utilities::random_test;
use merc_vpg::solve_zielonka;
use merc_vpg::translate;

use merc_reduction::Equivalence;
use merc_reduction::compare_lts;

#[test]
#[cfg_attr(miri, ignore)] // Tests are too slow under miri.
fn test_random_strong_bisim_distinguishing_formula() {
    random_test(100, |rng| {
        is_distinguishing_test("test_random_strong_bisim_distinguishing_formula", rng);
    });
}

/// Helper function that generates two related (but likely non-bisimilar)
/// random LTSs, and checks that the distinguishing formula produced by
/// `compare_lts` is indeed a valid witness of their non-bisimilarity.
fn is_distinguishing_test(dump_name: &str, rng: &mut StdRng) {
    let files = DumpFiles::new(dump_name);

    let lts1 = random_lts::<String, _>(rng, 100, 3);
    let lts2 = mutate_lts(&lts1, rng, 100).unwrap();

    files.dump("lts1.aut", |w| write_aut(w, &lts1)).unwrap();
    files.dump("lts2.aut", |w| write_aut(w, &lts2)).unwrap();

    let timing = Timing::new();
    let (equal, counter_example) = compare_lts(
        Equivalence::StrongBisimNaive,
        lts1.clone(),
        lts2.clone(),
        false,
        true,
        &timing,
    );

    if !equal {
        if let Some(ce) = counter_example {
            let formula = generate_distinguishing_formula(&ce);
            println!("Distinguishing formula: {}", formula);

            files
                .dump("distinguishing_formula.mcf", |f| Ok(writeln!(f, "{}", formula)?))
                .unwrap();

            // No `act` declarations: `formula` is built directly over the two LTSs' own string
            // labels, with a randomly-generated alphabet per run — there's nothing to declare it
            // against, so it type-checks as a "simple action" formula instead. `translate` desugars
            // and type checks its own copy, so each LTS needs one.
            let spec = UntypedStateFrmSpec {
                formula,
                action_declarations: Vec::new(),
                data_specification: UntypedDataSpecification::default(),
            };

            let lts1_pg = translate(&lts1, spec.clone()).unwrap();
            let lts2_pg = translate(&lts2, spec).unwrap();

            let (lts1_solution, _) = solve_zielonka(&lts1_pg, false);
            let (lts2_solution, _) = solve_zielonka(&lts2_pg, false);

            // The formula distinguishes the two systems iff their initial
            // vertices end up in different players' winning sets. Index the Even
            // winning set (player 0) by the initial vertex of each game.
            let lts1_init = lts1_pg.initial_vertex();
            let lts2_init = lts2_pg.initial_vertex();
            assert!(
                lts1_solution[0][*lts1_init] != lts2_solution[0][*lts2_init],
                "compare_lts returned false, but the counter example is not distinguishing."
            );
        } else {
            panic!("Expected a distinguishing formula.");
        }
    }
}
