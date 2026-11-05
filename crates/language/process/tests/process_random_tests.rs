use merc_process::ProcessSpecification;
use merc_syntax::random_process_specification;
use merc_utilities::random_test;
use rand::RngExt;

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn random_process_specification_with_data_specification_type_checks() {
    random_test(100, |rng| {
        let equation_count = rng.random_range(1..=3usize);
        let depth = rng.random_range(1..=3usize);
        let use_integers = rng.random_bool(0.5);

        let spec = random_process_specification(rng, 2, 1, equation_count, depth, use_integers);
        if let Err(error) = ProcessSpecification::from_untyped(spec.clone()) {
            panic!("expected the random process specification to type check:\n{spec}\nerror: {error}");
        }
    });
}
