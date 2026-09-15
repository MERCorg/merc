use std::fmt;

use merc_utilities::Span;

use crate::Bound;
use crate::IdDecl;
use crate::Quantifier;

mod actfrm;
mod dataexpr;
mod pbesexpr;
mod presexpr;
mod procexpr;
mod regfrm;
mod sortexpr;
mod specs;
mod statefrm;

/// Returns the 1-based `(line, column)` of the byte offset `span.start` within `input`.
///
/// Counts the bytes consumed by each preceding line (including its newline) until
/// the offset falls inside the current line. Offsets past the end of the input
/// resolve to the position just after the last character.
pub fn line_column(input: &str, span: &Span) -> (usize, usize) {
    let mut consumed = 0;
    for (number, line) in input.lines().enumerate() {
        // `+ 1` accounts for the newline that `lines()` strips.
        let line_bytes = line.len() + 1;
        if span.start < consumed + line_bytes {
            return (number + 1, span.start - consumed + 1);
        }
        consumed += line_bytes;
    }

    // The offset is at (or past) the end of the input.
    let last_line = input.lines().count().max(1);
    (last_line, span.start.saturating_sub(consumed) + 1)
}

impl fmt::Display for Quantifier {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Quantifier::Exists => write!(f, "exists"),
            Quantifier::Forall => write!(f, "forall"),
        }
    }
}

impl fmt::Display for Bound {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Bound::Inf => write!(f, "inf"),
            Bound::Sum => write!(f, "sum"),
            Bound::Sup => write!(f, "sup"),
        }
    }
}

impl<Id> fmt::Display for IdDecl<Id> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: {}", self.identifier.node, self.sort)
    }
}
