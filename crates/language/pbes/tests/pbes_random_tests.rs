use merc_pbes::PbesSpecification;
use merc_syntax::random_pbes;
use merc_utilities::random_test;
use rand::RngExt;

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn random_pbes_with_data_specification_type_checks() {
    random_test(100, |rng| {
        let equation_count = rng.random_range(1..=3usize);
        let atom_count = rng.random_range(1..=3usize);
        let propvar_count = rng.random_range(0..=2usize);
        let use_quantifiers = rng.random_bool(0.5);
        let use_integers = rng.random_bool(0.5);

        let pbes = random_pbes(
            rng,
            2,
            1,
            equation_count,
            atom_count,
            propvar_count,
            use_quantifiers,
            use_integers,
        );
        if let Err(error) = PbesSpecification::from_untyped(pbes.clone()) {
            panic!("expected the random PBES to type check:\n{pbes}\nerror: {error}");
        }
    });
}
