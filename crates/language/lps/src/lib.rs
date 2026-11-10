#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod io_lps;
mod linear_process;

pub use io_lps::read_lps;
pub use io_lps::write_lps;
pub use linear_process::Action;
pub use linear_process::ActionLabel;
pub use linear_process::ActionSummand;
pub use linear_process::DeadlockSummand;
pub use linear_process::LinearProcessSpecification;
pub use linear_process::MultiAction;
