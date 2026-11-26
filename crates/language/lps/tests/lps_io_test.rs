use merc_aterm::ATermString;
use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_data::Mcrl2DataSpecification;
use merc_data::SortExpression;
use merc_lps::Action;
use merc_lps::ActionLabel;
use merc_lps::ActionSummand;
use merc_lps::DeadlockSummand;
use merc_lps::LinearProcessSpecification;
use merc_lps::MultiAction;
use merc_lps::read_lps;
use merc_lps::write_lps;
use merc_syntax::UntypedProcessSpecification;
use merc_utilities::random_test;

/// `act a: Bool; proc P(b: Bool, c: Bool) = sum d: Bool . b -> a(d) . P(b = d) + c -> delta; init P(true, false);`
fn example() -> LinearProcessSpecification {
    let bool_sort = SortExpression::bool();
    let variable = |name: &str| DataVariable::with_sort(name, bool_sort.copy());
    let (b, c, d) = (variable("b"), variable("c"), variable("d"));
    let label = ActionLabel {
        name: ATermString::new("a"),
        sorts: vec![bool_sort.clone()],
    };

    LinearProcessSpecification {
        data_specification: Mcrl2DataSpecification::default(),
        action_labels: vec![label.clone()],
        global_variables: Vec::new(),
        process_parameters: vec![b.clone(), c.clone()],
        action_summands: vec![ActionSummand {
            summation_variables: vec![d.clone()],
            condition: b.clone().into(),
            action: MultiAction {
                actions: vec![Action {
                    label,
                    arguments: vec![d.clone().into()],
                }],
                time: None,
            },
            // `c` is left unchanged, so it is not written as an assignment.
            next_state: vec![Some(d.into()), None],
        }],
        deadlock_summands: vec![DeadlockSummand {
            summation_variables: Vec::new(),
            condition: c.into(),
            time: Some(merc_data::undefined_real()),
        }],
        initial_state: vec![DataExpression::bool_literal(true), DataExpression::bool_literal(false)],
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_lps_round_trip() {
    let lps = example();
    let mut bytes = Vec::new();
    write_lps(&lps, &mut bytes).unwrap();
    let reread = read_lps(&bytes[..]).unwrap();

    assert_eq!(lps.action_labels, reread.action_labels);
    assert_eq!(lps.global_variables, reread.global_variables);
    assert_eq!(lps.process_parameters, reread.process_parameters);
    assert_eq!(lps.action_summands, reread.action_summands);
    assert_eq!(lps.initial_state, reread.initial_state);
    // The undefined real is how mCRL2 encodes "no time", so it is read back as `None`.
    assert_eq!(reread.deadlock_summands[0].time, None);
    assert_eq!(
        lps.deadlock_summands[0].condition,
        reread.deadlock_summands[0].condition
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_lps_garbage_is_rejected() {
    assert!(read_lps(&b"not an lps"[..]).is_err());
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_lps_display() {
    let mut lps = example();
    lps.deadlock_summands[0].time = None;
    assert_eq!(
        lps.to_string(),
        "act a: Bool;\nproc P(b: Bool, c: Bool) =\n    sum d: Bool. b -> a(d) . P(b = d)\n  + c -> delta;\ninit P(true, false);\n"
    );
}

/// Property: for every generated untyped LPS AST, the printed form parses,
/// and printing the reparsed AST yields exactly the same string (a fixpoint).
///
/// This catches Display/grammar mismatches without depending on `PartialEq`
/// surviving parenthesization. Uses `random_test` to generate 100 random LPS
/// specs automatically with a reproducible seed (set via MERC_SEED env var).
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn random_lps_print_parse_fixpoint() {
    random_test(100, |rng| {
        let spec = merc_syntax::random_lps(rng, 4, 3, 0.6);
        let printed = format!("{spec}");
        let reparsed = UntypedProcessSpecification::parse(&printed)
            .unwrap_or_else(|e| panic!("failed to reparse generated LPS:\n{printed}\nerror: {e}"));
        assert_eq!(printed, format!("{reparsed}"), "LPS print/parse not a fixpoint");
    });
}
