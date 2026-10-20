#![forbid(unsafe_code)]

use std::fmt;

/// A distinguishing strong-bisimulation Hennessy–Milner formula.
///
/// The formula is generic over the transition label type `L`, storing the
/// actual labels so that consumers can render it without the LTS.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DistinguishingFormula<L> {
    /// `<label>( /\ conjuncts )`.
    Diamond {
        label: L,
        conjuncts: Vec<DistinguishingFormula<L>>,
    },
    /// The negation of the inner formula.
    Negate(Box<DistinguishingFormula<L>>),
}

impl<L> DistinguishingFormula<L> {
    /// Returns the nesting depth of diamond modalities of the formula.
    pub fn depth(&self) -> usize {
        match self {
            DistinguishingFormula::Diamond { conjuncts, .. } => {
                1 + conjuncts.iter().map(DistinguishingFormula::depth).max().unwrap_or(0)
            }
            DistinguishingFormula::Negate(inner) => inner.depth(),
        }
    }
}

impl<L: fmt::Display> fmt::Display for DistinguishingFormula<L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DistinguishingFormula::Diamond { label, conjuncts } => {
                write!(f, "<{label}>")?;
                if conjuncts.is_empty() {
                    write!(f, "true")
                } else {
                    write!(f, "(")?;
                    for (i, conjunct) in conjuncts.iter().enumerate() {
                        if i > 0 {
                            write!(f, " && ")?;
                        }
                        write!(f, "{conjunct}")?;
                    }
                    write!(f, ")")
                }
            }
            DistinguishingFormula::Negate(inner) => write!(f, "!({inner})"),
        }
    }
}
