// At most one `#[global_allocator]` may exist, so the backends below are
// mutually exclusive: `metrics` takes precedence and suppresses `jemalloc`
// and `mimalloc`, and enabling both `jemalloc` and `mimalloc` at once will
// fail to compile (two `#[global_allocator]` statics).
#[cfg(feature = "metrics")]
use log::info;

#[cfg(feature = "metrics")]
#[global_allocator]
static GLOBAL_ALLOCATOR: crate::AllocCounter = crate::AllocCounter::new();

#[cfg(not(target_env = "msvc"))]
#[cfg(not(feature = "metrics"))]
#[cfg(feature = "jemalloc")]
#[global_allocator]
static GLOBAL_ALLOCATOR: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[cfg(not(feature = "metrics"))]
#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL_ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Prints information from the [AllocCounter].
#[cfg(feature = "metrics")]
pub fn print_allocator_metrics() {
    info!("{}", GLOBAL_ALLOCATOR.get_metrics());
}

#[cfg(not(feature = "metrics"))]
pub fn print_allocator_metrics() {}
