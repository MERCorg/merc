//! Type checking of mCRL2 process specifications, see [ProcessSpecification].

mod check;
mod disambiguation;
mod error;
mod process_specification;
mod variable_resolution;

pub use disambiguation::disambiguate_process_specification;
pub use error::ProcessError;
pub use process_specification::ProcessSpecification;
