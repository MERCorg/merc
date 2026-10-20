#![forbid(unsafe_code)]

//! Shared helpers for benchmarking [`merc_lts::LtsBuilderMem`]'s duplicate-transition
//! removal (`remove_duplicates`, invoked via `finish(initial_state, true)`).
//!
//! There used to be a second, memory-hungry builder (`LtsBuilderFast`) that these
//! benchmarks compared against directly; it has since been deleted now that
//! `LtsBuilderMem` handles deduplication itself without needing a global sort (see
//! `crates/lts/src/lts_builder.rs`). These benchmarks now track `LtsBuilderMem`'s own
//! dedup performance and memory usage over time - compare `--save-baseline` runs
//! across revisions (see the `benchmark` skill) rather than against another builder.

use merc_lts::LabelIndex;
use merc_lts::LtsBuilder;
use merc_lts::LtsBuilderMem;
use merc_lts::StateIndex;
use rand::RngExt;
use rand::rngs::StdRng;

/// A synthetic, pre-generated stream of `(from, label, to)` transitions, along
/// with the label pool and state count used to generate it.
pub struct SyntheticTransitions {
    pub transitions: Vec<(StateIndex, LabelIndex, StateIndex)>,
    pub labels: Vec<String>,
    pub num_states: usize,
}

/// Generates a synthetic transition stream with a controlled duplicate ratio.
///
/// `out_degree` transitions are generated for every one of `num_states` states
/// (before dedup). For each, with probability `duplicate_ratio` it is an exact
/// repeat of a transition already generated for that same state (so the
/// intended amount of dedup work is known up front); otherwise it is a fresh
/// `(label, to)` pair, guaranteed distinct from every other fresh pair
/// generated so far for that state.
pub fn generate_uniform(
    rng: &mut StdRng,
    num_states: usize,
    out_degree: usize,
    duplicate_ratio: f64,
) -> SyntheticTransitions {
    generate(rng, num_states, |_state| out_degree, duplicate_ratio)
}

/// Like [`generate_uniform`], but a small number of "hub" states receive a much
/// higher out-degree than the rest, to stress the dedup path with a
/// pathologically large single bucket.
pub fn generate_with_hub_states(
    rng: &mut StdRng,
    num_states: usize,
    background_out_degree: usize,
    num_hub_states: usize,
    hub_out_degree: usize,
    duplicate_ratio: f64,
) -> SyntheticTransitions {
    generate(
        rng,
        num_states,
        |state| {
            if state < num_hub_states {
                hub_out_degree
            } else {
                background_out_degree
            }
        },
        duplicate_ratio,
    )
}

fn generate(
    rng: &mut StdRng,
    num_states: usize,
    out_degree_of: impl Fn(usize) -> usize,
    duplicate_ratio: f64,
) -> SyntheticTransitions {
    let max_out_degree = (0..num_states).map(&out_degree_of).max().unwrap_or(0);

    // Big enough that a state's fresh `(label, to)` pairs, derived from a
    // per-state counter below, can never collide with each other.
    let num_labels = max_out_degree.div_ceil(num_states.max(1)).max(1);
    let labels: Vec<String> = (0..num_labels).map(|i| format!("a{i}")).collect();

    let mut transitions = Vec::new();
    for state in 0..num_states {
        let out_degree = out_degree_of(state);
        let mut seen: Vec<(LabelIndex, StateIndex)> = Vec::with_capacity(out_degree);
        let mut fresh_counter = 0usize;

        for _ in 0..out_degree {
            let (label, to) = if !seen.is_empty() && rng.random::<f64>() < duplicate_ratio {
                seen[rng.random_range(0..seen.len())]
            } else {
                let label = LabelIndex::new(fresh_counter % num_labels);
                let to = StateIndex::new((fresh_counter / num_labels) % num_states);
                fresh_counter += 1;
                (label, to)
            };

            seen.push((label, to));
            transitions.push((StateIndex::new(state), label, to));
        }
    }

    SyntheticTransitions {
        transitions,
        labels,
        num_states,
    }
}

/// Builds a fresh, populated [`LtsBuilderMem`] from a [`SyntheticTransitions`].
pub fn fill_mem_builder(input: &SyntheticTransitions) -> LtsBuilderMem<String> {
    let mut builder = LtsBuilderMem::with_capacity(
        input.labels.clone(),
        Vec::new(),
        input.labels.len(),
        input.num_states,
        input.transitions.len(),
    );
    for (from, label, to) in &input.transitions {
        builder
            .add_transition(*from, &input.labels[label.value()], *to)
            .unwrap();
    }
    builder
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::*;

    /// Not a timed benchmark: reports how many bytes `LtsBuilderMem` holds on to
    /// while *accumulating* transitions and while deduplicating them, next to the
    /// naive `Vec<(usize, usize, usize)>` baseline it avoids paying for. Compare
    /// this output across revisions to catch memory regressions the timed
    /// benchmarks below wouldn't show.
    ///
    /// Run with `cargo test -p benchmarks_lts -- --nocapture memory_comparison`.
    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn memory_comparison() {
        let mut rng = StdRng::seed_from_u64(0x6c74735f6d656d63);

        for (name, input) in [
            (
                "uniform, 100k states x 8, 20% duplicates",
                generate_uniform(&mut rng, 100_000, 8, 0.2),
            ),
            (
                "hub, 1k states x 4 + 1 hub x 100k, 50% duplicates",
                generate_with_hub_states(&mut rng, 1_000, 4, 1, 100_000, 0.5),
            ),
        ] {
            let uncompressed = input.transitions.len() * std::mem::size_of::<(StateIndex, LabelIndex, StateIndex)>();

            let mut mem = fill_mem_builder(&input);
            let mem_before = mem.memory_usage();
            mem.finish(StateIndex::new(0), true);
            let mem_after = mem.memory_usage();

            println!("--- {name} ({} transitions before dedup) ---", input.transitions.len());
            println!("Vec<(usize, usize, usize)> baseline: {uncompressed} bytes");
            println!("LtsBuilderMem: {mem_before} bytes before dedup, {mem_after} bytes after");
        }
    }
}
