use std::time::Duration;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;

mod counter_benchmarks;

criterion_group!(
    name = benches;
    config = Criterion::default().measurement_time(Duration::new(10, 0)).sample_size(100);
    targets = counter_benchmarks::benchmark_sharded_counter,
        counter_benchmarks::benchmark_atomic_usize,
);
criterion_main!(benches);
