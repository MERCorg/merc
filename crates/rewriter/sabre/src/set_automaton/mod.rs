// This module contains the code to construct a set automaton. It does not use
// unsafe code.
#![forbid(unsafe_code)]

mod automaton;
mod display;
mod match_goal;

pub use automaton::*;
pub(crate) use match_goal::*;

#[allow(unused)]
pub use display::*;
