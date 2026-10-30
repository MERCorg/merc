use merc_pres::PresSpecification;
use merc_syntax::random_pres;
use merc_utilities::random_test;
use rand::RngExt;

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn random_pres_with_data_specification_type_checks() {
    random_test(100, |rng| {
        let equation_count = rng.random_range(1..=3usize);
        let atom_count = rng.random_range(1..=3usize);
        let propvar_count = rng.random_range(0..=2usize);
        let use_bounds = rng.random_bool(0.5);

        let pres = random_pres(rng, 2, 1, equation_count, atom_count, propvar_count, use_bounds);
        if let Err(error) = PresSpecification::from_untyped(pres.clone()) {
            panic!("expected the random PRES to type check:\n{pres}\nerror: {error}");
        }
    });
}
