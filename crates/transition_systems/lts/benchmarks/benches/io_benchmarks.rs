use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use std::hint::black_box;

use merc_aterm::ATerm;
use merc_aterm::ATermInt;
use merc_aterm::ATermList;
use merc_aterm::ATermRef;
use merc_aterm::Symbol;
use merc_aterm::Term;
use merc_collections::VecBag;
use merc_data::DataFunctionSymbol;
use merc_data::Mcrl2DataSpecification;
use merc_lts::BuilderIndex;
use merc_lts::LTS;
use merc_lts::LabelledTransitionSystem;
use merc_lts::LtsAction;
use merc_lts::LtsBuilderMem;
use merc_lts::LtsMultiAction;
use merc_lts::StateIndex;
use merc_lts::StateLabels;
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

/// Returns the multi-action `name`, with a data argument for `value` if given.
fn label(name: &str, value: Option<usize>) -> LtsMultiAction<LtsAction> {
    let arguments = value
        .map(|value| DataFunctionSymbol::new(format!("n{value}").as_str()).into())
        .into_iter()
        .collect();
    LtsMultiAction::new(VecBag::singleton(LtsAction::new(name.to_string(), arguments)))
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

/// The state labels of a generated LTS, one term for every state.
struct StateVectors(Vec<ATerm>);

impl StateLabels for StateVectors {
    type Label<'a> = ATermRef<'a>;

    fn get(&self, state_index: StateIndex) -> Option<ATermRef<'_>> {
        self.0.get(state_index.value()).map(|label| label.copy())
    }

    fn len(&self) -> usize {
        self.0.len()
    }
}

/// Generates the LTS of the given shape, together with its (empty) data specification.
fn generate_lts(
    shape: Shape,
) -> (
    LabelledTransitionSystem<LtsMultiAction<LtsAction>, StateVectors>,
    Mcrl2DataSpecification,
) {
    let n = (TRANSITIONS / 3).isqrt();
    let node = Symbol::new("node", 2);

    // The labels a, b and c for every data value, of which plain shapes have only one.
    let mut builder = LtsBuilderMem::new(Vec::new(), Vec::new());
    let num_of_values = match shape {
        Shape::Data => DATA_LABEL_VALUES,
        _ => 1,
    };
    let labels: Vec<[BuilderIndex; 3]> = (0..num_of_values)
        .map(|value| {
            let value = matches!(shape, Shape::Data).then_some(value);
            ["a", "b", "c"].map(|name| builder.label_index(&label(name, value)))
        })
        .collect();

    for i in 0..n {
        for j in 0..n {
            let successors = [((i + 1) % n, j), (i, (j + 1) % n), ((i + 1) % n, (j + 1) % n)];
            for (label, (ni, nj)) in labels[(i + j) % num_of_values].into_iter().zip(successors) {
                builder.add_transition_index(StateIndex::new(i * n + j), label, StateIndex::new(ni * n + nj));
            }
        }
    }

    let state_labels = StateVectors(
        (0..n)
            .flat_map(|i| (0..n).map(move |j| (i, j)))
            .filter_map(|(i, j)| shape.state_label(i, j))
            .map(|values| state_label(&node, &values))
            .collect(),
    );
    let lts = builder
        .finish(StateIndex::new(0), false)
        .with_state_labels(state_labels);

    (lts, Mcrl2DataSpecification::default())
}

/// Benchmarks reading an `.lts` file from memory.
pub fn bench_read_lts(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_lts");
    for shape in Shape::ALL {
        let (lts, data_spec) = generate_lts(shape);
        let mut bytes = Vec::new();
        write_lts(&mut bytes, &lts, &data_spec).expect("Writing to memory succeeds");

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
        let (lts, data_spec) = generate_lts(shape);

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
