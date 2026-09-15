//! Whole-state-formula type checking: [`ModalSpecification`] extends [`crate::DataSpecification`]
//! to also check `act` declarations, action instances, fixpoint (`mu`/`nu`) variables, and every
//! `forall`/`exists`/`inf`/`sup`/`sum` binder in a modal (mu-calculus) formula.

mod check;
mod error;
mod modal_specification;

pub use error::ModalError;
pub use modal_specification::ModalSpecification;
pub use modal_specification::ValSort;
