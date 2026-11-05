#![forbid(unsafe_code)]

use criterion::BatchSize;
use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use merc_data::DataExpression;
use merc_lowering::lower_data_specification;
use merc_sabre::InnermostRewriter;
use merc_sabre::RewriteEngine;
use merc_sabre::RewriteSpecification;
use merc_sabre::SabreRewriter;
use merc_syntax::SourceMap;
use merc_syntax::UntypedDataSpecification;
use merc_typecheck::DataSpecification;
use merc_typecheck::NumberEncoding;

/// Recursive functions over `Nat` and `List(Nat)`, defined by conditional equations so that
/// the innermost rewriters do not evaluate both branches of an `if`.
///
/// The same equations, with `init delta;` appended, are used to time the goals with mCRL2's
/// `mcrl2i`, built with and without `MCRL2_ENABLE_MACHINENUMBERS`.
const SPEC: &str = "
map fib, total: Nat -> Nat;
    upto: Nat # Nat -> Nat;
    numbers: Nat -> List(Nat);
    ins: Nat # List(Nat) -> List(Nat);
    isort: List(Nat) -> List(Nat);
var n, k, m: Nat;
    l: List(Nat);
eqn n < 2 -> fib(n) = n;
    n >= 2 -> fib(n) = fib(Int2Nat(n - 1)) + fib(Int2Nat(n - 2));
    n == 0 -> total(n) = 0;
    n > 0 -> total(n) = n + total(Int2Nat(n - 1));
    n < k -> upto(n, k) = upto(n + 1, k);
    n >= k -> upto(n, k) = n;
    n == 0 -> numbers(n) = [];
    n > 0 -> numbers(n) = ((Int2Nat(n - 1) * 7919) mod 1009) |> numbers(Int2Nat(n - 1));
    ins(m, []) = [m];
    m <= n -> ins(m, n |> l) = m |> n |> l;
    m > n -> ins(m, n |> l) = n |> ins(m, l);
    isort([]) = [];
    isort(m |> l) = ins(m, isort(l));
";

/// Goals that rewrite to `true`, chosen to separate where each encoding pays:
///
/// - `fib`, `total` and `upto` do many operations on small numbers, the case that the binary
///   encoding handles with a few bits and the machine-word encoding with a single digit;
/// - `sqrt` computes with a single number, through multiplication and comparison;
/// - `isort` mostly compares numbers below 1009, inside list manipulation.
const GOALS: &[(&str, &str)] = &[
    ("fib", "fib(18) == 2584"),
    ("sum", "total(2000) == 2001000"),
    ("count", "upto(0, 5000) == 5000"),
    ("sqrt", "sqrt(4095) * sqrt(4095) <= 4095"),
    (
        "sort",
        "head(isort(numbers(300))) == 0 && rhead(isort(numbers(300))) == 1006",
    ),
];

/// Returns the rewrite specification of [SPEC] with `goal` as the right-hand side of a constant
/// `b`, and the term `b`, lowered under `encoding`.
fn lower(goal: &str, encoding: NumberEncoding) -> (RewriteSpecification, DataExpression) {
    let untyped = UntypedDataSpecification::parse(&format!("{SPEC}\nmap b: Bool;\neqn b = {goal};")).unwrap();
    let spec = lower_data_specification(
        &DataSpecification::from_untyped_with(untyped, encoding, &mut SourceMap::new()).unwrap(),
    );
    let b = spec
        .equations()
        .iter()
        .find(|equation| equation.lhs().to_string() == "b")
        .expect("no equation for b")
        .lhs()
        .protect();
    (RewriteSpecification::from_data_specification(&spec), b)
}

/// Rewrites each goal under both number encodings, with the innermost and Sabre rewriters.
/// Every iteration uses a fresh rewriter, whose construction and destruction are not timed,
/// so that the condition cache does not carry results over from earlier iterations.
fn criterion_benchmark_number_encoding(c: &mut Criterion) {
    let mut group = c.benchmark_group("number encoding");
    for &(name, goal) in GOALS {
        for (encoding_name, encoding) in [
            ("binary", NumberEncoding::Binary),
            ("machine word", NumberEncoding::MachineWord),
        ] {
            group.bench_function(format!("innermost {name} {encoding_name}"), |bencher| {
                let (spec, b) = lower(goal, encoding);
                assert_eq!(
                    InnermostRewriter::new(&spec).rewrite(&b),
                    DataExpression::bool_literal(true),
                    "{goal}"
                );
                bencher.iter_batched(
                    || InnermostRewriter::new(&spec),
                    |mut rewriter| {
                        let result = rewriter.rewrite(&b);
                        // Dropped after timing, like the rewriter.
                        (rewriter, result)
                    },
                    BatchSize::LargeInput,
                );
            });

            group.bench_function(format!("sabre {name} {encoding_name}"), |bencher| {
                let (spec, b) = lower(goal, encoding);
                bencher.iter_batched(
                    || SabreRewriter::new(&spec),
                    |mut rewriter| {
                        let result = rewriter.rewrite(&b);
                        (rewriter, result)
                    },
                    BatchSize::LargeInput,
                );
            });
        }
    }
    group.finish();
}

criterion_group!(benches, criterion_benchmark_number_encoding);
criterion_main!(benches);
