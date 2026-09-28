use merc_syntax::DataExprKind;
use merc_syntax::EqnDecl;
use merc_syntax::EqnSpecData;
use merc_syntax::IdDecl;
use merc_syntax::MapId;
use merc_syntax::SortExpressionKind;
use merc_syntax::random_data_specification;
use merc_syntax::random_pbes;
use merc_syntax::random_pres;
use merc_syntax::random_process_specification;
use merc_syntax::random_value_expression;
use merc_typecheck::DataSpecification;
use merc_typecheck::PbesSpecification;
use merc_typecheck::PresSpecification;
use merc_typecheck::ProcessSpecification;
use merc_utilities::Span;
use merc_utilities::random_test;
use rand::RngExt;

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn random_data_specification_type_checks() {
    random_test(200, |rng| {
        let spec = random_data_specification(rng, 2, 1);
        if let Err(error) = DataSpecification::from_untyped(spec.clone()) {
            panic!("expected the random data specification to type check:\n{spec}\nerror: {error}");
        }
    });
}

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

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn random_value_expression_type_checks_for_every_generated_sort() {
    random_test(200, |rng| {
        let mut spec = random_data_specification(rng, 2, 1);

        for i in 0..spec.sort_declarations.len() {
            let sort = SortExpressionKind::Reference(spec.sort_declarations[i].identifier.clone()).into();
            let value = random_value_expression(rng, &spec.sort_declarations, &sort, &[], 2);

            let name = format!("g{i}");
            spec.map_declarations
                .push(IdDecl::<MapId>::new(name.clone(), sort, Span::default()));
            spec.equation_declarations.push(
                EqnSpecData {
                    variables: Vec::new(),
                    equations: vec![EqnDecl {
                        condition: None,
                        lhs: DataExprKind::Id(name).into(),
                        rhs: value,
                        span: Span::default(),
                        id: None,
                    }],
                    id: None,
                }
                .spanned(Span::default()),
            );
        }

        if let Err(error) = DataSpecification::from_untyped(spec.clone()) {
            panic!("expected the random values to type check:\n{spec}\nerror: {error}");
        }
    });
}

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
