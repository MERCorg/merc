#![forbid(unsafe_code)]

use criterion::BatchSize;
use criterion::Bencher;
use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use merc_data::DataExpression;
use merc_sabre::InnermostRewriter;
use merc_sabre::JittyRewriter;
use merc_sabre::RewriteEngine;
use merc_sabre::RewriteSpecification;
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
/// - `isort` mostly compares numbers below 1009, inside list manipulation;
/// - `compare` and `exp` depend on rewriting only the selected branch of an `if` in the
///   specification of the numbers: binary comparison takes time linear in the value under the
///   innermost rewriter, and its `exp` does not terminate.
///
/// The flag tells whether the innermost rewriter is timed as well.
const GOALS: &[(&str, &str, bool)] = &[
    ("fib", "fib(18) == 2584", true),
    ("sum", "total(2000) == 2001000", true),
    ("count", "upto(0, 5000) == 5000", true),
    ("sqrt", "sqrt(4095) * sqrt(4095) <= 4095", true),
    (
        "sort",
        "head(isort(numbers(300))) == 0 && rhead(isort(numbers(300))) == 1006",
        true,
    ),
    ("compare", "1048575 < 1048576", true),
    ("exp", "exp(3, 400) mod 1000003 == 313558", false),
];

/// Returns the rewrite specification of [SPEC] with `goal` as the right-hand side of a constant
/// `b`, and the term `b`, lowered under `encoding`.
fn lower(goal: &str, encoding: NumberEncoding) -> (RewriteSpecification, DataExpression) {
    let untyped = UntypedDataSpecification::parse(&format!("{SPEC}\nmap b: Bool;\neqn b = {goal};")).unwrap();
    let spec = DataSpecification::from_untyped_with(untyped, encoding, &mut SourceMap::new())
        .unwrap()
        .lower_data_specification();
    let b = spec
        .equations()
        .iter()
        .find(|equation| equation.lhs().to_string() == "b")
        .expect("no equation for b")
        .lhs()
        .protect();
    (RewriteSpecification::from_data_specification(&spec), b)
}

/// Rewrites each goal under both number encodings, with the innermost and jitty rewriters.
/// Every iteration uses a fresh rewriter, whose construction and destruction are not timed,
/// so that the condition cache does not carry results over from earlier iterations.
fn criterion_benchmark_number_encoding(c: &mut Criterion) {
    let mut group = c.benchmark_group("number encoding");
    for &(name, goal, innermost) in GOALS {
        for (encoding_name, encoding) in [
            ("binary", NumberEncoding::Binary),
            ("machine word", NumberEncoding::MachineWord),
        ] {
            if innermost {
                group.bench_function(format!("innermost {name} {encoding_name}"), |bencher| {
                    let (spec, b) = lower(goal, encoding);
                    bench_rewriter(bencher, &b, || InnermostRewriter::new(&spec));
                });
            }

            group.bench_function(format!("jitty {name} {encoding_name}"), |bencher| {
                let (spec, b) = lower(goal, encoding);
                bench_rewriter(bencher, &b, || JittyRewriter::new(&spec));
            });
        }
    }
    group.finish();
}

/// Times rewriting `b` to `true` with a fresh rewriter made by `new_rewriter` per iteration.
fn bench_rewriter<R: RewriteEngine>(bencher: &mut Bencher<'_>, b: &DataExpression, new_rewriter: impl Fn() -> R) {
    assert_eq!(new_rewriter().rewrite(b), DataExpression::bool_literal(true), "{b}");
    bencher.iter_batched(
        new_rewriter,
        |mut rewriter| {
            let result = rewriter.rewrite(b);
            // Dropped after timing, like the rewriter.
            (rewriter, result)
        },
        BatchSize::LargeInput,
    );
}

criterion_group!(benches, criterion_benchmark_number_encoding);
criterion_main!(benches);
