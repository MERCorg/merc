//! Whole-PBES-specification type checking: [`PbesSpecification`] extends
//! [`crate::DataSpecification`] to also check global variables, propositional-variable equations,
//! and `init`. See [`PbesSpecification`]'s doc comment for what's in and out of scope.

mod check;
mod error;
mod pbes_specification;

pub use error::PbesError;
pub use pbes_specification::PbesSpecification;
