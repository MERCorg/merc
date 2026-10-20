use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;

mod dedup_benchmarks;
mod io_benchmarks;

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = dedup_benchmarks::bench_dedup_vs_transition_count,
        dedup_benchmarks::bench_dedup_vs_duplicate_ratio,
        dedup_benchmarks::bench_dedup_vs_num_states,
        dedup_benchmarks::bench_dedup_hub_state,
        io_benchmarks::bench_read_lts,
        io_benchmarks::bench_write_lts,
);
criterion_main!(benches);
