#![forbid(unsafe_code)]

use std::hint::black_box;
use std::ops::ControlFlow;
use std::rc::Rc;

use criterion::BatchSize;
use criterion::Bencher;
use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use merc_data::BasicSort;
use merc_data::DataApplication;
use merc_data::DataExpression;
use merc_data::DataFunctionSymbol;
use merc_data::DataVariable;
use merc_data::Mcrl2DataSpecification;
use merc_data::SortArrow;
use merc_data::SortExpression;
use merc_enumerate::EnumerationLimits;
use merc_enumerate::EnumerationPlans;
use merc_enumerate::Enumerator;
use merc_enumerate::EnumeratorFrame;
use merc_enumerate::FreshVariableGenerator;
use merc_sabre::InnermostRewriter;
use merc_sabre::JittyRewriter;
use merc_sabre::RewriteEngine;
use merc_sabre::RewriteSpecification;
use merc_syntax::SourceMap;
use merc_syntax::UntypedDataSpecification;
use merc_typecheck::DataSpecification;
use merc_typecheck::NumberEncoding;

/// A small recursive sort mirroring `Nat`'s shape (`dzero`/`dsucc`), avoiding
/// the real `Nat`/`Pos` prelude.
const PRELUDE: &str = "
    sort D;
    cons dzero: D;
         dsucc: D -> D;
    map lt: D # D -> Bool;
    var x, y: D;
    eqn lt(x, dzero) = false;
        lt(dzero, dsucc(y)) = true;
        lt(dsucc(x), dsucc(y)) = lt(x, y);
";

fn lower(source: &str) -> Mcrl2DataSpecification {
    lower_with(source, NumberEncoding::default())
}

fn lower_with(source: &str, encoding: NumberEncoding) -> Mcrl2DataSpecification {
    let untyped = UntypedDataSpecification::parse(source).unwrap();
    let data_spec = DataSpecification::from_untyped_with(untyped, encoding, &mut SourceMap::new()).unwrap();
    data_spec.lower_data_specification()
}

fn d_sort() -> SortExpression {
    SortExpression::from(BasicSort::new("D"))
}

fn numeral(value: u32) -> DataExpression {
    let dzero = DataFunctionSymbol::with_sort("dzero", d_sort().copy());
    let succ_sort: SortExpression = SortArrow::new(&[d_sort()], &d_sort()).into();
    let dsucc = DataFunctionSymbol::with_sort("dsucc", succ_sort.copy());
    let mut term: DataExpression = dzero.into();
    for _ in 0..value {
        term = DataApplication::with_args(&dsucc, &[term]).into();
    }
    term
}

fn lt_goal(bound: u32) -> (Vec<DataVariable>, DataExpression) {
    let n = DataVariable::with_sort("n", d_sort().copy());
    let lt_sort: SortExpression = SortArrow::new(&[d_sort(), d_sort()], &SortExpression::bool()).into();
    let lt = DataFunctionSymbol::with_sort("lt", lt_sort.copy());
    let body: DataExpression =
        DataApplication::with_args(&lt, &[DataExpression::from(n.clone()), numeral(bound)]).into();
    (vec![n], body)
}

/// Constructor-expansion throughput for `sum n:D . n < bound`, across a range
/// of bounds, with one rewriter and enumerator shared across all iterations as
/// a state space exploration would.
fn criterion_benchmark_bounded_enumeration(c: &mut Criterion) {
    let spec = lower(PRELUDE);
    let rewrite_spec = RewriteSpecification::from_data_specification(&spec);
    let plans = Rc::new(EnumerationPlans::build(&spec));
    let mut rewriter = InnermostRewriter::new(&rewrite_spec);
    let enumerator = Enumerator::new(plans, FreshVariableGenerator::new("v", std::iter::empty()));
    let mut frame = EnumeratorFrame::default();

    let mut group = c.benchmark_group("bounded sum enumeration");
    for bound in [10, 50, 200] {
        let (vars, body) = lt_goal(bound);
        group.bench_function(format!("n < {bound}"), |bencher| {
            bencher.iter(|| {
                let mut count = 0usize;
                enumerator.enumerate(
                    &mut frame,
                    &mut rewriter,
                    &vars,
                    &body,
                    |_rewriter, _solution| -> ControlFlow<()> {
                        count += 1;
                        ControlFlow::Continue(())
                    },
                );
                black_box(count);
            });
        });
    }
    group.finish();
}

/// Quantified goals, with their expected normal form, that are also timed with mCRL2's
/// `mcrl2i -Q0` to compare both enumerators.
///
/// Neither rewriter applies the one-point rule, so `one_point` measures searching `n` up to 500.
const QUANTIFIER_GOALS: &[(&str, bool, &str)] = &[
    ("sqrt", true, "exists n: Nat . n < 200 && n * n == 39601"),
    ("parity", true, "forall n: Nat . n < 100 => (n * (n + 1)) mod 2 == 0"),
    ("one_point", true, "exists n: Nat . n == 500 && n mod 7 == 3"),
    (
        "nested",
        true,
        "forall x: Nat . x < 30 => exists y: Nat . y < 30 && x + y == 29",
    ),
    (
        "deep_nested",
        true,
        "forall x: Nat . x < 12 => exists y: Nat . y < 12 && (forall z: Nat . z < 12 => x + y + z < 36)",
    ),
    (
        "siblings",
        true,
        "forall x: Nat . x < 30 => ((exists y: Nat . y < 30 && x + y == 29) && (exists z: Nat . z < 30 && x == z))",
    ),
    (
        "sum_squares",
        false,
        "exists x, y: Nat . x < 40 && y < 40 && x * x + y * y == 3",
    ),
    (
        "xor_assoc",
        true,
        "forall b1, b2, b3, b4, b5, b6, b7, b8, b9, b10, b11, b12: Bool . \
         (((((((((((b1 != b2) != b3) != b4) != b5) != b6) != b7) != b8) != b9) != b10) != b11) != b12) == \
         (b1 != (b2 != (b3 != (b4 != (b5 != (b6 != (b7 != (b8 != (b9 != (b10 != (b11 != b12)))))))))))",
    ),
    (
        "pigeonhole",
        false,
        "exists a, b, c, d, e: Colour . a != b && a != c && a != d && a != e && b != c && b != d && b != e \
         && c != d && c != e && d != e",
    ),
];

/// Rewriting a quantified goal to `true` or `false` with the enumerator wired into the
/// rewriter, without limits as `-Q0` does for mCRL2. Every iteration uses a fresh rewriter,
/// whose construction and destruction are not timed, so that the condition cache does not
/// carry results over from earlier iterations.
///
/// Each goal is timed with the innermost and jitty rewriters, under both number encodings.
/// mCRL2 uses machine words, so compare with it under `machine word`.
///
/// The timing of a goal depends on the state of the global term pool that earlier goals
/// leave behind, by more than a factor two, so compare goals measured in separate runs.
fn criterion_benchmark_quantifier_goals(c: &mut Criterion) {
    let mut group = c.benchmark_group("quantifier goals");
    for &(name, expected, goal) in QUANTIFIER_GOALS {
        for (encoding_name, encoding) in [
            ("binary", NumberEncoding::Binary),
            ("machine word", NumberEncoding::MachineWord),
        ] {
            // Set up in the closures, which criterion skips for a goal that is filtered out.
            let setup = || {
                let spec = lower_with(
                    &format!("sort Colour = struct red | green | blue | yellow;\nmap b: Bool;\neqn b = {goal};"),
                    encoding,
                );
                let body = spec
                    .equations()
                    .iter()
                    .find(|eq| eq.lhs().data_function_symbol().name().value() == "b")
                    .expect("no equation for b")
                    .rhs()
                    .protect();
                let rewrite_spec = RewriteSpecification::from_data_specification(&spec);
                let plans = Rc::new(EnumerationPlans::build(&spec));
                (body, rewrite_spec, plans)
            };
            let enumerator = |plans: &Rc<EnumerationPlans>| {
                let limits = EnumerationLimits {
                    max_items: usize::MAX,
                    max_depth: u32::MAX,
                };
                let generator = FreshVariableGenerator::new("v", std::iter::empty());
                Enumerator::new(plans.clone(), generator).with_limits(limits)
            };

            group.bench_function(format!("innermost {name} {encoding_name}"), |bencher| {
                let (body, rewrite_spec, plans) = setup();
                let new_rewriter = || InnermostRewriter::with_enumerator(&rewrite_spec, enumerator(&plans));
                bench_goal(bencher, &body, expected, new_rewriter);
            });

            group.bench_function(format!("jitty {name} {encoding_name}"), |bencher| {
                let (body, rewrite_spec, plans) = setup();
                let new_rewriter = || JittyRewriter::with_enumerator(&rewrite_spec, enumerator(&plans));
                bench_goal(bencher, &body, expected, new_rewriter);
            });
        }
    }
    group.finish();
}

/// Times rewriting `body` to `expected`, with a fresh rewriter made by `new_rewriter` per
/// iteration.
fn bench_goal<R: RewriteEngine>(
    bencher: &mut Bencher<'_>,
    body: &DataExpression,
    expected: bool,
    new_rewriter: impl Fn() -> R,
) {
    assert_eq!(
        new_rewriter().rewrite(body),
        DataExpression::bool_literal(expected),
        "{body}"
    );
    bencher.iter_batched(
        new_rewriter,
        |mut rewriter| {
            let result = rewriter.rewrite(body);
            // Dropped after timing, since dropping a rewriter takes about a millisecond.
            (rewriter, result)
        },
        BatchSize::LargeInput,
    );
}

/// Cost of constructing the enumeration plans for a specification.
fn criterion_benchmark_enumeration_plans_build(c: &mut Criterion) {
    let spec = lower(PRELUDE);

    c.bench_function("EnumerationPlans::build (D + Bool prelude)", |bencher| {
        bencher.iter(|| {
            black_box(EnumerationPlans::build(&spec));
        });
    });
}

criterion_group!(
    benches,
    criterion_benchmark_bounded_enumeration,
    criterion_benchmark_quantifier_goals,
    criterion_benchmark_enumeration_plans_build,
);
criterion_main!(benches);
