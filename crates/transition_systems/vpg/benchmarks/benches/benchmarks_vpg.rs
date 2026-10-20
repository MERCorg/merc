use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;

mod attractor_benchmarks;

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = attractor_benchmarks::bench_attractor_vs_num_vertices,
        attractor_benchmarks::bench_attractor_vs_out_degree,
        attractor_benchmarks::bench_attractor_vs_num_priorities,
        attractor_benchmarks::bench_attractor_vs_num_groups,
        attractor_benchmarks::bench_attractor_vs_threads,
        attractor_benchmarks::bench_attractor_vs_capacity,
);
criterion_main!(benches);
