//! Whole-PRES-specification type checking: [`PresSpecification`] extends
//! [`crate::DataSpecification`] to also check global variables, propositional-variable equations
//! (each a real-valued PRES formula, unlike a PBES's boolean-valued one — see [`check`]), and
//! `init`. See [`PresSpecification`]'s doc comment for what's in and out of scope.

mod check;
mod error;
mod pres_specification;

pub use error::PresError;
pub use pres_specification::PresSpecification;
