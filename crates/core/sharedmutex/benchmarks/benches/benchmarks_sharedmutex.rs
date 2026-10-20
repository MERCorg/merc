use std::time::Duration;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;

mod mutex_benchmarks;
mod vec_benchmarks;

criterion_group!(
    name = benches;
    config = Criterion::default().measurement_time(Duration::new(10, 0)).sample_size(100);
    targets = mutex_benchmarks::benchmark_bfsharedmutex,
        vec_benchmarks::benchmark_vector,
);
criterion_main!(benches);
