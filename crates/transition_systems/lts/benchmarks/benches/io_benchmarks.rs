use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use std::hint::black_box;

use merc_aterm::ATerm;
use merc_aterm::ATermInt;
use merc_aterm::ATermList;
use merc_aterm::ATermStreamable;
use merc_aterm::ATermWrite;
use merc_aterm::BinaryATermWriter;
use merc_aterm::Symbol;
use merc_aterm::Term;
use merc_collections::VecBag;
use merc_data::DataFunctionSymbol;
use merc_data::Mcrl2DataSpecification;
use merc_lts::LTS;
use merc_lts::LtsAction;
use merc_lts::LtsMultiAction;
use merc_lts::read_lts;
use merc_lts::write_lts;

/// The number of transitions of every generated LTS, roughly.
const TRANSITIONS: usize = 300_000;

/// The number of extra state parameters of the `wide` shape.
const WIDE_EXTRA_PARAMETERS: usize = 16;

/// The number of distinct values in the action labels of the `data` shape.
const DATA_LABEL_VALUES: usize = 1000;

/// The input shapes of mCRL2's `ltsiobench` (`tools/developer/ltsiobench/generate_inputs.py`,
/// mCRL2 pull request 1976), with terms of the same structure but simpler names. Every shape is a torus of N x N states, in which state (i, j) has
/// transitions labelled a, b and c to states (i + 1, j), (i, j + 1) and (i + 1, j + 1) modulo N.
#[derive(Clone, Copy)]
enum Shape {
    /// Plain actions and no state labels, which isolates decoding the transitions.
    Grid,
    /// Plain actions and the state labels (i, j).
    Info,
    /// Actions a(n), b(n) and c(n) with n = (i + j) mod 1000, so many distinct labels.
    Data,
    /// Plain actions and state labels with 18 parameters: i, j and (i + k * j) mod 1000.
    Wide,
}

impl Shape {
    const ALL: [Shape; 4] = [Shape::Grid, Shape::Info, Shape::Data, Shape::Wide];

    fn name(self) -> &'static str {
        match self {
            Shape::Grid => "grid",
            Shape::Info => "info",
            Shape::Data => "data",
            Shape::Wide => "wide",
        }
    }

    /// Returns the state label of state (i, j), if this shape has state labels.
    fn state_label(self, i: usize, j: usize) -> Option<Vec<usize>> {
        match self {
            Shape::Grid | Shape::Data => None,
            Shape::Info => Some(vec![i, j]),
            Shape::Wide => Some(
                [i, j]
                    .into_iter()
                    .chain((1..=WIDE_EXTRA_PARAMETERS).map(|k| (i + k * j) % 1000))
                    .collect(),
            ),
        }
    }
}

/// Returns the label term of the action `name`, with a data argument for `value` if given.
fn label(name: &str, value: Option<usize>) -> ATerm {
    let arguments = value
        .map(|value| DataFunctionSymbol::new(format!("n{value}").as_str()).into())
        .into_iter()
        .collect();
    LtsMultiAction::new(VecBag::singleton(LtsAction::new(name.to_string(), arguments)))
        .to_mcrl2_aterm()
        .expect("A valid multi-action")
}

/// Returns a state label: a list with one balanced tree of the values, the way mCRL2 stores state
/// vectors.
fn state_label(node: &Symbol, values: &[usize]) -> ATerm {
    ATermList::from_double_iter([balanced_tree(node, values)].into_iter()).into()
}

/// Returns the balanced tree of `values`, with the larger half on the left.
fn balanced_tree(node: &Symbol, values: &[usize]) -> ATerm {
    if let [value] = values {
        return ATermInt::new(*value).into();
    }

    let (left, right) = values.split_at(values.len().div_ceil(2));
    ATerm::with_args(node, &[balanced_tree(node, left), balanced_tree(node, right)]).protect()
}

/// A constant term with the given name.
fn constant(name: &str) -> ATerm {
    ATerm::constant(&Symbol::new(name, 0))
}

/// Writes an `.lts` file of the given shape, with its parts in the order in which mCRL2 writes them.
fn lts_bytes(shape: Shape) -> Vec<u8> {
    let n = (TRANSITIONS / 3).isqrt();
    let transition = constant("transition");
    let node = Symbol::new("node", 2);

    let mut bytes = Vec::new();
    {
        let mut writer = BinaryATermWriter::new(&mut bytes).expect("Writing to memory succeeds");
        writer.write_aterm(&constant("labelled_transition_system")).unwrap();
        Mcrl2DataSpecification::default().write(&mut writer).unwrap();
        writer.write_aterm(&ATermList::<ATerm>::empty().into()).unwrap(); // Parameters
        writer.write_aterm(&ATermList::<ATerm>::empty().into()).unwrap(); // Action labels

        let plain_labels: Vec<ATerm> = ["a", "b", "c"].iter().map(|name| label(name, None)).collect();
        for i in 0..n {
            for j in 0..n {
                let successors = [((i + 1) % n, j), (i, (j + 1) % n), ((i + 1) % n, (j + 1) % n)];
                for (k, (name, (ni, nj))) in ["a", "b", "c"].iter().zip(successors).enumerate() {
                    let label = match shape {
                        Shape::Data => &label(name, Some((i + j) % DATA_LABEL_VALUES)),
                        _ => &plain_labels[k],
                    };

                    writer.write_aterm(&transition).unwrap();
                    writer.write_aterm_int(i * n + j).unwrap();
                    writer.write_aterm(label).unwrap();
                    writer.write_aterm_int(ni * n + nj).unwrap();
                }
            }
        }

        for i in 0..n {
            for j in 0..n {
                if let Some(values) = shape.state_label(i, j) {
                    writer.write_aterm(&state_label(&node, &values)).unwrap();
                }
            }
        }

        writer.write_aterm(&constant("initial_state")).unwrap();
        writer.write_aterm_int(1).unwrap();
        writer.write_aterm_int(0).unwrap();
        ATermWrite::flush(&mut writer).unwrap();
    }
    bytes
}

/// Benchmarks reading an `.lts` file from memory.
pub fn bench_read_lts(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_lts");
    for shape in Shape::ALL {
        let bytes = lts_bytes(shape);
        let (lts, _) = read_lts(&bytes[..], false).expect("A valid LTS");

        group.throughput(Throughput::Elements(lts.num_of_transitions() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(shape.name()), &bytes, |b, bytes| {
            b.iter(|| black_box(read_lts(&bytes[..], false).expect("A valid LTS")));
        });
    }
    group.finish();
}

/// Benchmarks writing the transitions of an `.lts` file to memory, without state labels.
pub fn bench_write_lts(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_lts");
    for shape in [Shape::Grid, Shape::Data] {
        let (lts, data_spec) = read_lts(&lts_bytes(shape)[..], false).expect("A valid LTS");

        group.throughput(Throughput::Elements(lts.num_of_transitions() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(shape.name()), &lts, |b, lts| {
            b.iter(|| {
                let mut bytes = Vec::new();
                write_lts(&mut bytes, lts, &data_spec).expect("Writing to memory succeeds");
                black_box(bytes)
            });
        });
    }
    group.finish();
}
