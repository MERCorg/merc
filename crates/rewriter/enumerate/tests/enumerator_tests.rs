use std::ops::ControlFlow;
use std::rc::Rc;

use ahash::AHashSet;
use merc_aterm::Term;
use merc_data::DataExpression;
use merc_data::DataExpressionRef;
use merc_data::DataVariable;
use merc_data::DataVariableRef;
use merc_data::Mcrl2DataSpecification;
use merc_data::is_data_variable;
use merc_data::visit_data_expr;
use merc_enumerate::EnumerationLimits;
use merc_enumerate::EnumerationPlans;
use merc_enumerate::Enumerator;
use merc_enumerate::FreshVariableGenerator;
use merc_enumerate::Outcome;
use merc_enumerate::QuantifierKind;
use merc_enumerate::WitnessOutcome;
use merc_enumerate::simplify_one_point;
use merc_sabre::InnermostRewriter;
use merc_sabre::RewriteEngine;
use merc_sabre::RewriteSpecification;
use merc_syntax::UntypedDataSpecification;
use merc_typecheck::DataSpecification;
use merc_utilities::Step;

/// Every variable occurring free in `term`.
fn free_variables(term: &DataExpression) -> AHashSet<DataVariable> {
    let mut free = AHashSet::new();
    let _: Option<()> = visit_data_expr(&term.copy(), (), |expr: &DataExpressionRef<'_>, context| {
        if is_data_variable(expr) {
            free.insert(DataVariableRef::from(Term::copy(expr)).protect());
        }
        ControlFlow::Continue(Step::Into(context))
    });
    free
}

/// `D`'s shape mirrors `Nat`'s (`dzero`/`dsucc`, `lt`/`eq` defined by
/// recursion on both constructors), so it exercises the same "infinite
/// enumerable sort with a bounded guard" pattern the real `Nat` prelude would,
/// without pulling that prelude in.
const PRELUDE: &str = "
    sort D;
    cons dzero: D;
         dsucc: D -> D;
    map lt: D # D -> Bool;
        eq: D # D -> Bool;
    var x, y: D;
    eqn lt(x, dzero) = false;
        lt(dzero, dsucc(y)) = true;
        lt(dsucc(x), dsucc(y)) = lt(x, y);
        eq(dzero, dzero) = true;
        eq(dsucc(x), dzero) = false;
        eq(dzero, dsucc(y)) = false;
        eq(dsucc(x), dsucc(y)) = eq(x, y);
";

/// Builds the numeral `D` term for `value` (`dsucc(dsucc(...dzero...))`), for
/// asserting on solutions found by an [`Enumerator`] built over [`PRELUDE`].
fn numeral(value: usize) -> DataExpression {
    let dzero = merc_data::DataFunctionSymbol::with_sort("dzero", d_sort().copy());
    let succ_sort: merc_data::SortExpression = merc_data::SortArrow::new(&[d_sort()], d_sort()).into();
    let dsucc = merc_data::DataFunctionSymbol::with_sort("dsucc", succ_sort.copy());
    let mut term: DataExpression = dzero.into();
    for _ in 0..value {
        term = merc_data::DataApplication::with_args(&dsucc, &[term]).into();
    }
    term
}

fn d_sort() -> merc_data::SortExpression {
    merc_data::SortExpression::from(merc_data::BasicSort::new("D"))
}

/// Parses and type-checks `PRELUDE` followed by `source`.
fn lower(source: &str) -> Mcrl2DataSpecification {
    let untyped = UntypedDataSpecification::parse(&format!("{PRELUDE}\n{source}")).unwrap();
    let data_spec = DataSpecification::from_untyped(untyped).unwrap();
    data_spec.lower_data_specification()
}

/// Returns the `(variables, rhs)` of the single equation defining mapping
/// `name`, i.e. treats `map name: ... -> Bool; eqn name(vars...) = body;` as a
/// `(vars, body)` enumeration goal.
fn goal(spec: &Mcrl2DataSpecification, name: &str) -> (Vec<DataVariable>, DataExpression) {
    let equation = spec
        .equations()
        .iter()
        .find(|eq| eq.lhs().data_function_symbol().name().value() == name)
        .unwrap_or_else(|| panic!("no equation for {name}"));
    (equation.variables().to_vec(), equation.rhs().protect())
}

/// A generator seeded from exactly the names in scope for one goal — see
/// [`FreshVariableGenerator`]'s doc comment on why this is the caller's job.
fn generator_for(vars: &[DataVariable]) -> FreshVariableGenerator {
    FreshVariableGenerator::new("v", vars.iter().map(|v| v.name().to_string()))
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_enumerate_finds_every_bounded_solution() {
    let spec = lower(
        "map goal: D -> Bool;
         var n: D;
         eqn goal(n) = lt(n, dsucc(dsucc(dsucc(dsucc(dsucc(dzero))))));", // n < 5
    );
    let rewrite_spec = RewriteSpecification::from_data_specification(&spec);
    let mut rewriter = InnermostRewriter::new(&rewrite_spec);
    let plans = Rc::new(EnumerationPlans::build(&spec));

    let (vars, body) = goal(&spec, "goal");

    let mut enumerator = Enumerator::new(plans);
    let mut results: Vec<DataExpression> = Vec::new();
    let outcome = enumerator.enumerate(
        &mut rewriter,
        &mut generator_for(&vars),
        &vars,
        &body,
        |_rewriter, solution| -> ControlFlow<()> {
            results.push(solution.values()[0].clone());
            ControlFlow::Continue(())
        },
    );

    assert!(matches!(outcome, Outcome::Exhausted), "{outcome:?}");

    let expected: AHashSet<DataExpression> = (0..5).map(numeral).collect();
    let actual: AHashSet<DataExpression> = results.iter().cloned().collect();
    assert_eq!(actual, expected, "{results:?}");
    assert_eq!(results.len(), 5, "solutions must be pairwise distinct: {results:?}");
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_enumerate_never_truncates_past_the_default_limits() {
    // `Enumerator::enumerate` (the `sum`-exploration path) must ignore
    // `EnumerationLimits` entirely: a truncated result is an unsound LTS.
    let target = 20;
    let tiny_limits = EnumerationLimits {
        max_items: 3,
        max_depth: 2,
    };

    let bound: String = numeral_source(target);
    let spec = lower(&format!(
        "map goal: D -> Bool;
         var n: D;
         eqn goal(n) = lt(n, {bound});"
    ));
    let rewrite_spec = RewriteSpecification::from_data_specification(&spec);
    let mut rewriter = InnermostRewriter::new(&rewrite_spec);
    let plans = Rc::new(EnumerationPlans::build(&spec));

    let (vars, body) = goal(&spec, "goal");

    let mut enumerator = Enumerator::new(plans).with_limits(tiny_limits);
    let mut count = 0usize;
    let outcome = enumerator.enumerate(
        &mut rewriter,
        &mut generator_for(&vars),
        &vars,
        &body,
        |_rewriter, _solution| -> ControlFlow<()> {
            count += 1;
            ControlFlow::Continue(())
        },
    );

    assert!(matches!(outcome, Outcome::Exhausted), "{outcome:?}");
    assert_eq!(
        count, target,
        "must find every solution despite max_items={}",
        tiny_limits.max_items
    );
}

/// Renders `dsucc(dsucc(...dzero...))` as mCRL2 source text, for embedding a
/// numeral literal directly in a parsed spec.
fn numeral_source(value: usize) -> String {
    let mut text = "dzero".to_string();
    for _ in 0..value {
        text = format!("dsucc({text})");
    }
    text
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_enumerate_is_fair_across_two_infinite_variables() {
    // A depth-first "finish `n` before touching `m`" strategy would never
    // reach any solution with `m > 0`, since `n` alone has infinitely many
    // values. The 3x3 = 9 solutions here can only all be found if both
    // dimensions are explored — see `docs/enumeration-crate-plan.md` §4.5.
    let spec = lower(
        "map goal: D # D -> Bool;
         var n: D;
             m: D;
         eqn goal(n, m) = lt(n, dsucc(dsucc(dsucc(dzero)))) && lt(m, dsucc(dsucc(dsucc(dzero))));", // n<3 && m<3
    );
    let rewrite_spec = RewriteSpecification::from_data_specification(&spec);
    let mut rewriter = InnermostRewriter::new(&rewrite_spec);
    let plans = Rc::new(EnumerationPlans::build(&spec));

    let (vars, body) = goal(&spec, "goal");

    let mut enumerator = Enumerator::new(plans);
    let mut results: AHashSet<(DataExpression, DataExpression)> = AHashSet::new();
    let outcome = enumerator.enumerate(
        &mut rewriter,
        &mut generator_for(&vars),
        &vars,
        &body,
        |_rewriter, solution| -> ControlFlow<()> {
            let values = solution.values();
            results.insert((values[0].clone(), values[1].clone()));
            ControlFlow::Continue(())
        },
    );

    assert!(matches!(outcome, Outcome::Exhausted), "{outcome:?}");
    assert_eq!(results.len(), 9, "{results:?}");
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_compile_one_point_then_enumerate_normalized_matches_direct_enumeration() {
    // `b == true` narrows `b` to a single substitution instead of a search
    // over `Bool`, leaving only the (still genuinely searched) `n < 3` over
    // `D`.
    let spec = lower(
        "map goal: Bool # D -> Bool;
         var b: Bool;
             n: D;
         eqn goal(b, n) = (b == true) && lt(n, dsucc(dsucc(dsucc(dzero))));", // b == true && n < 3
    );
    let rewrite_spec = RewriteSpecification::from_data_specification(&spec);
    let mut rewriter = InnermostRewriter::new(&rewrite_spec);
    let plans = Rc::new(EnumerationPlans::build(&spec));

    let (vars, body) = goal(&spec, "goal");
    let normalized_body = rewriter.rewrite(&body);
    let b = vars[0].clone();

    let mut condition = normalized_body;
    let mut rest = [DataExpression::from(b.clone())];
    simplify_one_point(&mut rewriter, &vars, &mut condition, &mut rest);
    let other = rest[0].clone();

    let true_literal: DataExpression = merc_data::DataFunctionSymbol::with_sort(
        "true",
        merc_data::SortExpression::from(merc_data::BasicSort::new("Bool")).copy(),
    )
    .into();
    assert_eq!(
        other, true_literal,
        "`b`'s eliminated binding must be substituted into every other term, not just `terms[0]`"
    );

    // `compile_one_point` doesn't report which of `vars` it eliminated, so a
    // caller re-derives it by re-scanning the rewritten terms — here, just
    // `condition`, since `other` is not itself an enumeration goal.
    let free = free_variables(&condition);
    let remaining_vars: Vec<DataVariable> = vars.iter().filter(|v| free.contains(*v)).cloned().collect();
    assert_eq!(
        remaining_vars.iter().map(|v| v.name().to_string()).collect::<Vec<_>>(),
        vec!["n"],
        "only `n` should still be free in the residual condition once `b` is compiled away"
    );

    let mut enumerator = Enumerator::new(plans);
    let mut results: Vec<DataExpression> = Vec::new();
    let outcome = enumerator.enumerate_normalized(
        &mut rewriter,
        &mut generator_for(&vars),
        &remaining_vars,
        condition,
        |_rewriter, solution| -> ControlFlow<()> {
            results.push(solution.values()[0].clone());
            ControlFlow::Continue(())
        },
    );

    assert!(matches!(outcome, Outcome::Exhausted), "{outcome:?}");

    let expected: AHashSet<DataExpression> = (0..3).map(numeral).collect();
    let actual: AHashSet<DataExpression> = results.iter().cloned().collect();
    assert_eq!(actual, expected, "{results:?}");
    assert_eq!(results.len(), 3, "solutions must be pairwise distinct: {results:?}");
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_find_witness_exists_finds_a_small_witness() {
    let spec = lower(&format!(
        "map goal: D -> Bool;
         var n: D;
         eqn goal(n) = eq(n, {});",
        numeral_source(7)
    ));
    let rewrite_spec = RewriteSpecification::from_data_specification(&spec);
    let mut rewriter = InnermostRewriter::new(&rewrite_spec);
    let plans = Rc::new(EnumerationPlans::build(&spec));

    let (vars, body) = goal(&spec, "goal");

    let mut enumerator = Enumerator::new(plans);
    match enumerator.find_witness(
        &mut rewriter,
        &mut generator_for(&vars),
        &vars,
        &body,
        QuantifierKind::Exists,
    ) {
        WitnessOutcome::Found(values) => assert_eq!(values, vec![numeral(7)]),
        other => panic!("expected a witness, got {other:?}"),
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_find_witness_gives_up_past_the_bound() {
    // The witness exists (`eq(n, target)`) but lies beyond the configured
    // `max_items`, so the bounded search must give up rather than claim
    // `NoneExists` — for `∃` that distinction is `false` versus "unknown".
    let target = 20;
    let tiny_limits = EnumerationLimits {
        max_items: 3,
        max_depth: 64,
    };

    let spec = lower(&format!(
        "map goal: D -> Bool;
         var n: D;
         eqn goal(n) = eq(n, {});",
        numeral_source(target)
    ));
    let rewrite_spec = RewriteSpecification::from_data_specification(&spec);
    let mut rewriter = InnermostRewriter::new(&rewrite_spec);
    let plans = Rc::new(EnumerationPlans::build(&spec));

    let (vars, body) = goal(&spec, "goal");

    let mut enumerator = Enumerator::new(plans).with_limits(tiny_limits);
    match enumerator.find_witness(
        &mut rewriter,
        &mut generator_for(&vars),
        &vars,
        &body,
        QuantifierKind::Exists,
    ) {
        WitnessOutcome::LimitReached => {}
        other => panic!("expected GaveUp, got {other:?}"),
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_find_witness_forall_holds_over_a_finite_sort() {
    // `b || !b` is a Bool tautology: searching for a *counterexample* over the
    // finite `Bool` sort must exhaust without finding one, which is what
    // proves the quantifier holds.
    let spec = lower(
        "map goal: Bool -> Bool;
         var b: Bool;
         eqn goal(b) = b || !b;",
    );
    let rewrite_spec = RewriteSpecification::from_data_specification(&spec);
    let mut rewriter = InnermostRewriter::new(&rewrite_spec);
    let plans = Rc::new(EnumerationPlans::build(&spec));

    let (vars, body) = goal(&spec, "goal");

    let mut enumerator = Enumerator::new(plans);
    match enumerator.find_witness(
        &mut rewriter,
        &mut generator_for(&vars),
        &vars,
        &body,
        QuantifierKind::Forall,
    ) {
        WitnessOutcome::NoneExists => {}
        other => panic!("expected NoneExists (the quantifier holds), got {other:?}"),
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_find_witness_forall_is_not_broken_by_the_one_point_rule() {
    // `∀ n:D . n == dzero` is false — `dsucc(dzero)` is a counterexample.
    // `==` on `D` is otherwise uninterpreted, so the equations below are what
    // let the ground instances decide; `n == dzero` itself still survives
    // normalisation, which is what offers the one-point rule its bait.
    let spec = lower(
        "map goal: D -> Bool;
         var n: D;
             i, j: D;
         eqn goal(n) = n == dzero;
             dzero == dzero = true;
             dsucc(i) == dzero = false;
             dzero == dsucc(j) = false;
             dsucc(i) == dsucc(j) = i == j;",
    );
    let rewrite_spec = RewriteSpecification::from_data_specification(&spec);
    let mut rewriter = InnermostRewriter::new(&rewrite_spec);
    let plans = Rc::new(EnumerationPlans::build(&spec));

    let (vars, body) = goal(&spec, "goal");

    let mut enumerator = Enumerator::new(plans);
    match enumerator.find_witness(
        &mut rewriter,
        &mut generator_for(&vars),
        &vars,
        &body,
        QuantifierKind::Forall,
    ) {
        WitnessOutcome::Found(values) => {
            assert_ne!(values[0], numeral(0), "dzero is not a counterexample to `n == dzero`");
        }
        other => panic!("expected a counterexample to `forall n. n == dzero`, got {other:?}"),
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_find_witness_forall_does_not_claim_an_undecided_body_holds() {
    // `p` has no equations, so `p(true)`/`p(false)` are stuck: neither is the
    // literal `false` (no counterexample found) nor the literal `true` (no
    // proof either). `NoneExists` means "the quantifier holds", which is not
    // something this search established — `GaveUp` is the honest answer, per
    // `WitnessOutcome::GaveUp`'s own doc comment.
    let spec = lower(
        "map goal: Bool -> Bool;
             p: Bool -> Bool;
         var b: Bool;
         eqn goal(b) = p(b);",
    );
    let rewrite_spec = RewriteSpecification::from_data_specification(&spec);
    let mut rewriter = InnermostRewriter::new(&rewrite_spec);
    let plans = Rc::new(EnumerationPlans::build(&spec));

    let (vars, body) = goal(&spec, "goal");

    let mut enumerator = Enumerator::new(plans);
    match enumerator.find_witness(
        &mut rewriter,
        &mut generator_for(&vars),
        &vars,
        &body,
        QuantifierKind::Forall,
    ) {
        WitnessOutcome::LimitReached => {}
        other => panic!("expected GaveUp for an undecidable body, got {other:?}"),
    }
}

// The tests below are ported from mCRL2's `libraries/data/test/enumerator_test.cpp`.

/// Parses and type-checks `source` without [`PRELUDE`].
fn lower_without_prelude(source: &str) -> Mcrl2DataSpecification {
    let untyped = UntypedDataSpecification::parse(source).unwrap();
    let data_spec = DataSpecification::from_untyped(untyped).unwrap();
    data_spec.lower_data_specification()
}

/// Same as [`goal`], but keeps only the variables occurring in the equation's
/// left-hand side, since an equation carries every variable of its `var` section.
fn goal_on_lhs(spec: &Mcrl2DataSpecification, name: &str) -> (Vec<DataVariable>, DataExpression) {
    let equation = spec
        .equations()
        .iter()
        .find(|eq| eq.lhs().data_function_symbol().name().value() == name)
        .unwrap_or_else(|| panic!("no equation for {name}"));
    let free = free_variables(&equation.lhs().protect());
    let vars = equation.variables().iter().filter(|v| free.contains(v)).collect();
    (vars, equation.rhs().protect())
}

/// Enumerates the solutions of goal `name` in `spec`, stopping after `max` of them.
fn solutions(spec: &Mcrl2DataSpecification, name: &str, max: usize) -> (Outcome, Vec<Vec<DataExpression>>) {
    let rewrite_spec = RewriteSpecification::from_data_specification(spec);
    let mut rewriter = InnermostRewriter::new(&rewrite_spec);
    let (vars, body) = goal_on_lhs(spec, name);

    let mut enumerator = Enumerator::new(Rc::new(EnumerationPlans::build(spec)));
    let mut results = Vec::new();
    let outcome = enumerator.enumerate(
        &mut rewriter,
        &mut generator_for(&vars),
        &vars,
        &body,
        |_rewriter, solution| {
            results.push(solution.values().to_vec());
            if results.len() >= max {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        },
    );
    (outcome, results)
}

/// Searches for a witness of `kind` for goal `name` in `spec`.
fn witness(spec: &Mcrl2DataSpecification, name: &str, kind: QuantifierKind) -> WitnessOutcome {
    let rewrite_spec = RewriteSpecification::from_data_specification(spec);
    let mut rewriter = InnermostRewriter::new(&rewrite_spec);
    let (vars, body) = goal_on_lhs(spec, name);

    let mut enumerator = Enumerator::new(Rc::new(EnumerationPlans::build(spec)));
    enumerator.find_witness(&mut rewriter, &mut generator_for(&vars), &vars, &body, kind)
}

/// Returns the normal form of nullary mapping `name`'s right-hand side.
fn value(spec: &Mcrl2DataSpecification, name: &str) -> DataExpression {
    let rewrite_spec = RewriteSpecification::from_data_specification(spec);
    let (_, body) = goal_on_lhs(spec, name);
    InnermostRewriter::new(&rewrite_spec).rewrite(&body)
}

#[test]
#[ignore = "ported from mCRL2"]
fn test_enumerate_without_variables() {
    let spec = lower_without_prelude(
        "map yes, no: Bool;
             none: Nat -> Bool;
         var y: Nat;
         eqn yes = true;
             no = false;
             none(y) = false;",
    );

    let (outcome, results) = solutions(&spec, "yes", usize::MAX);
    assert!(matches!(outcome, Outcome::Exhausted), "{outcome:?}");
    assert_eq!(results, vec![Vec::new()]);

    let (outcome, results) = solutions(&spec, "no", usize::MAX);
    assert!(matches!(outcome, Outcome::Exhausted), "{outcome:?}");
    assert!(results.is_empty(), "{results:?}");

    let (outcome, results) = solutions(&spec, "none", usize::MAX);
    assert!(matches!(outcome, Outcome::Exhausted), "{outcome:?}");
    assert!(results.is_empty(), "{results:?}");
}

#[test]
#[ignore = "ported from mCRL2"]
fn test_find_witness_in_a_user_defined_list() {
    let spec = lower_without_prelude(
        "sort list_of_booleans;
         cons empty: list_of_booleans;
              lcons: Bool # list_of_booleans -> list_of_booleans;
         map size: list_of_booleans -> Nat;
             goal: list_of_booleans # Nat -> Bool;
         var l: list_of_booleans;
             b: Bool;
             x: list_of_booleans;
             y: Nat;
         eqn size(empty) = 0;
             size(lcons(b, l)) = 1 + size(l);
             goal(x, y) = y == size(x) && 0 < y && y < 2;",
    );

    match witness(&spec, "goal", QuantifierKind::Exists) {
        WitnessOutcome::Found(_) => {}
        other => panic!("expected a witness, got {other:?}"),
    }
}

#[test]
#[ignore = "ported from mCRL2"]
fn test_enumerate_structured_sort() {
    let spec = lower_without_prelude(
        "sort D = struct d1(E) | d2(E);
              E = struct e1 | e2;
         map goal: D -> Bool;
             v1, v2, v3, v4: D;
         var d: D;
         eqn goal(d) = true;
             v1 = d1(e1);
             v2 = d1(e2);
             v3 = d2(e1);
             v4 = d2(e2);",
    );

    let (outcome, results) = solutions(&spec, "goal", usize::MAX);
    assert!(matches!(outcome, Outcome::Exhausted), "{outcome:?}");

    let expected: AHashSet<DataExpression> = ["v1", "v2", "v3", "v4"].iter().map(|v| value(&spec, v)).collect();
    let actual: AHashSet<DataExpression> = results.iter().map(|values| values[0].clone()).collect();
    assert_eq!(actual, expected);
    assert_eq!(results.len(), 4, "solutions must be pairwise distinct: {results:?}");
}

#[test]
#[ignore = "ported from mCRL2"]
fn test_find_witness_with_equality_substitution() {
    let spec = lower_without_prelude(
        "map contradiction, double: Pos -> Bool;
             seventeen: Pos;
         var x: Pos;
         eqn contradiction(x) = x == 17 && x != 17;
             double(x) = x == 17 && 2 * x == 34;
             seventeen = 17;",
    );

    match witness(&spec, "contradiction", QuantifierKind::Exists) {
        WitnessOutcome::NoneExists => {}
        other => panic!("expected no witness, got {other:?}"),
    }

    match witness(&spec, "double", QuantifierKind::Exists) {
        WitnessOutcome::Found(values) => assert_eq!(values, vec![value(&spec, "seventeen")]),
        other => panic!("expected a witness, got {other:?}"),
    }
}

/// mCRL2 ticket #1486: the solution must be found regardless of which side of
/// the equality the variable occurs on, and for both `Nat` and `Pos`.
#[test]
#[ignore = "ported from mCRL2"]
fn test_find_witness_ticket_1486() {
    let spec = lower_without_prelude(
        "map left, right: Nat -> Bool;
             positive: Pos -> Bool;
             five_nat: Nat;
             five_pos: Pos;
         var y: Nat;
             p: Pos;
         eqn left(y) = y + 20 == 25;
             right(y) = 25 == 20 + y;
             positive(p) = 25 == 20 + p;
             five_nat = 5;
             five_pos = 5;",
    );

    for (name, expected) in [("left", "five_nat"), ("right", "five_nat"), ("positive", "five_pos")] {
        match witness(&spec, name, QuantifierKind::Exists) {
            WitnessOutcome::Found(values) => assert_eq!(values, vec![value(&spec, expected)], "{name}"),
            other => panic!("expected a witness for {name}, got {other:?}"),
        }
    }
}

#[test]
#[ignore = "ported from mCRL2"]
fn test_enumerate_tree_yields_distinct_values() {
    let spec = lower_without_prelude(
        "sort tree_with_booleans;
         cons leaf: Bool -> tree_with_booleans;
              node: tree_with_booleans # tree_with_booleans -> tree_with_booleans;
         map goal: tree_with_booleans -> Bool;
         var x: tree_with_booleans;
         eqn goal(x) = true;",
    );

    let (outcome, results) = solutions(&spec, "goal", 32);
    assert!(matches!(outcome, Outcome::Stopped(())), "{outcome:?}");

    let distinct: AHashSet<&Vec<DataExpression>> = results.iter().collect();
    assert_eq!(distinct.len(), 32, "{results:?}");
}

#[test]
#[ignore = "ported from mCRL2"]
fn test_enumerate_mutually_recursive_sorts() {
    let spec = lower_without_prelude(
        "sort this;
              that;
         cons a: this;
              A: that -> this;
              b: that;
              B: this -> that;
         map goal_this: this -> Bool;
             goal_that: that -> Bool;
         var x: this;
             y: that;
         eqn goal_this(x) = true;
             goal_that(y) = true;",
    );

    for name in ["goal_this", "goal_that"] {
        let (outcome, results) = solutions(&spec, name, 32);
        assert!(matches!(outcome, Outcome::Stopped(())), "{name}: {outcome:?}");

        let distinct: AHashSet<&Vec<DataExpression>> = results.iter().collect();
        assert_eq!(distinct.len(), 32, "{name}: {results:?}");
    }
}

#[test]
#[ignore = "ported from mCRL2"]
fn test_enumerate_finite_structured_sorts_without_duplicates() {
    let sorts = [
        (
            "Identifiers",
            "t1 | t2 | t3 | t4 | t5 | p1 | p2 | s1 | r1 | r2 | r3 | IL",
        ),
        ("signal_Messages", "ic_set_proceed_signal | ic_set_stop_signal"),
        ("Enum3", "e2_3 | e1_3 | e0_3"),
        (
            "Enum15",
            "e14_15 | e13_15 | e12_15 | e11_15 | e10_15 | e9_15 | e8_15 | e7_15 | e6_15 | e5_15 | e4_15 | e3_15 | e2_15 | e1_15 | e0_15",
        ),
        (
            "point__working_moving_States",
            "point__working_moving_left_substate | point__working_moving_right_substate | point__working_moving_nop",
        ),
        ("HAL_device_States", "HAL_device__normal_substate"),
    ];

    let mut source = String::new();
    for (sort, constructors) in sorts {
        source.push_str(&format!(
            "sort {sort} = struct {constructors};
             map goal_{sort}: {sort} -> Bool;
             var x_{sort}: {sort};
             eqn goal_{sort}(x_{sort}) = true;\n"
        ));
    }
    let spec = lower_without_prelude(&source);

    for (sort, constructors) in sorts {
        let (outcome, results) = solutions(&spec, &format!("goal_{sort}"), 10);
        let expected = constructors.split('|').count().min(10);
        assert_eq!(results.len(), expected, "{sort}: {outcome:?} {results:?}");

        let distinct: AHashSet<&Vec<DataExpression>> = results.iter().collect();
        assert_eq!(distinct.len(), results.len(), "{sort}: {results:?}");
    }
}

/// `F2` is a constructor, but an equation rewrites it to `fSucc(F1)`.
#[test]
#[ignore = "ported from mCRL2"]
fn test_enumerate_constructors_that_are_not_a_normal_form() {
    let spec = lower_without_prelude(
        "sort FloorID = struct F1 | F2;
         map fSucc: FloorID -> FloorID;
             equal: FloorID # FloorID -> Bool;
             goal: FloorID -> Bool;
         var f, g: FloorID;
         eqn F2 = fSucc(F1);
             equal(fSucc(f), fSucc(g)) = equal(f, g);
             equal(fSucc(f), F1) = false;
             equal(F1, fSucc(g)) = false;
             equal(F1, F1) = true;
             f == g = equal(f, g);
             goal(f) = equal(f, F2);",
    );

    let (outcome, results) = solutions(&spec, "goal", usize::MAX);
    assert!(matches!(outcome, Outcome::Exhausted), "{outcome:?}");
    assert_eq!(results.len(), 1, "{results:?}");
}

#[test]
#[ignore = "ported from mCRL2"]
fn test_enumerate_set_of_bool() {
    let spec = lower_without_prelude(
        "map goal: Set(Bool) -> Bool;
         var f: Set(Bool);
         eqn goal(f) = true in f;",
    );

    let (outcome, results) = solutions(&spec, "goal", usize::MAX);
    assert!(matches!(outcome, Outcome::Exhausted), "{outcome:?}");
    assert_eq!(results.len(), 4, "{results:?}");
}

#[test]
#[ignore = "ported from mCRL2"]
fn test_cannot_enumerate_real() {
    let spec = lower_without_prelude(
        "map goal: Real -> Bool;
         var r: Real;
         eqn goal(r) = r == r;",
    );

    let (outcome, results) = solutions(&spec, "goal", usize::MAX);
    assert!(matches!(outcome, Outcome::NotEnumerable(..)), "{outcome:?}");
    assert!(results.is_empty(), "{results:?}");

    match witness(&spec, "goal", QuantifierKind::Exists) {
        WitnessOutcome::LimitReached => {}
        other => panic!("expected the search to give up, got {other:?}"),
    }
}

#[test]
#[ignore = "ported from mCRL2"]
fn test_find_witness_over_nat_with_a_bounded_guard() {
    let spec = lower_without_prelude(
        "map goal: Nat -> Bool;
         var n: Nat;
         eqn goal(n) = n < 2;",
    );

    // `forall n: Nat. n < 2` has a counterexample, `exists n: Nat. n < 2` a witness.
    for kind in [QuantifierKind::Forall, QuantifierKind::Exists] {
        match witness(&spec, "goal", kind) {
            WitnessOutcome::Found(_) => {}
            other => panic!("expected a witness for {kind:?}, got {other:?}"),
        }
    }
}
