use std::collections::HashSet;

use merc_syntax::SortExpressionKind;
use merc_syntax::SortId;
use merc_syntax::UntypedDataSpecification;

use crate::argument_sorts;
use crate::target_sort;

/// Returns the set of syntactically non-empty sorts.
///
/// A sort that is the target of at least one constructor can be empty; every
/// other declared sort (an abstract sort or an alias) is unconstrained and
/// assumed non-empty. The set is therefore seeded with those non-constructor
/// sorts and then grown as the least fixpoint of the non-emptiness rule: a
/// constructor sort becomes non-empty as soon as it has a constructor all of
/// whose argument sorts are non-empty. Built-in, container and function argument
/// sorts are treated as non-empty.
pub(crate) fn nonempty_sorts(spec: &UntypedDataSpecification) -> HashSet<SortId> {
    let constructor_sorts: HashSet<SortId> = spec
        .constructor_declarations
        .iter()
        .filter_map(|constructor| match &target_sort(&constructor.sort).node {
            SortExpressionKind::Resolved(_, id) => Some(*id),
            _ => None,
        })
        .collect();

    let mut nonempty: HashSet<SortId> = spec
        .sort_declarations
        .iter()
        .map(|declaration| declaration.id.expect("Name must have been resolved"))
        .filter(|id| !constructor_sorts.contains(id))
        .collect();

    let mut changed = true;
    while changed {
        changed = false;
        for constructor in &spec.constructor_declarations {
            let SortExpressionKind::Resolved(_, target) = &target_sort(&constructor.sort).node else {
                unreachable!("The target sort of a constructor should be a resolved sort");
            };

            if nonempty.contains(target) {
                continue;
            }

            let all_arguments_nonempty =
                argument_sorts(&constructor.sort)
                    .iter()
                    .all(|argument| match &argument.node {
                        SortExpressionKind::Resolved(_, id) => nonempty.contains(id),
                        _ => true,
                    });

            if all_arguments_nonempty {
                nonempty.insert(*target);
                changed = true;
            }
        }
    }

    nonempty
}

#[cfg(test)]
mod tests {
    use merc_syntax::UntypedDataSpecification;

    use crate::DataSpecification;
    use crate::WellTypedError;

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_empty_data_spec() {
        let spec = UntypedDataSpecification::parse(
            "
            sort D;
            cons f: D -> D;
        ",
        )
        .unwrap();

        match DataSpecification::from_untyped(spec) {
            Err(WellTypedError::EmptySort { sort, .. }) if sort == "D" => {}
            Err(other) => panic!("Unexpected {:?}", other),
            _ => panic!("Unexpected from_untyped to fail"),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_constant_constructor_is_nonempty() {
        // Regression: a constant constructor `c: S` has no argument sorts and
        // makes `S` non-empty; the non-emptiness check must not treat it as a
        // function sort (which panicked in `argument_sorts`).
        let spec = UntypedDataSpecification::parse("sort S;\ncons c: S;").unwrap();
        DataSpecification::from_untyped(spec).expect("a sort with a constant constructor is non-empty");
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_abstract_argument_sort_is_nonempty() {
        // Regression: an abstract sort `D` used as a constructor argument is
        // unconstrained and assumed non-empty, so `E` is non-empty here.
        let spec = UntypedDataSpecification::parse("sort D;\n     E;\ncons c: D -> E;").unwrap();
        DataSpecification::from_untyped(spec).expect("a constructor over an abstract argument sort is non-empty");
    }
}
