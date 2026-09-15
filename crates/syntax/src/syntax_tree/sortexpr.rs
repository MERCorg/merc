use merc_utilities::Span;

use crate::spanned::Spanned;

use super::SortId;
use super::TypeVarId;

/// The kind of a [SortExpression] node.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum SortExpressionKind {
    /// Product of two sorts (A # B)
    Product {
        lhs: Box<SortExpression>,
        rhs: Box<SortExpression>,
    },
    /// Function sort (A -> B)
    Function {
        domain: Box<SortExpression>,
        range: Box<SortExpression>,
    },
    Struct {
        inner: Vec<ConstructorDecl>,
    },
    /// Reference to a named sort
    Reference(String),
    /// A bound sort (type) variable, such as the `S` in a container spec.
    TypeVar(String),
    /// A bound sort (type) variable after name resolution has assigned its
    /// [TypeVarId], mirroring how [Self::Reference] becomes [Self::Resolved].
    ResolvedTypeVar(TypeVarId),
    /// Built-in simple sort
    Simple(Sort),
    /// Parameterized complex sort
    Complex(ComplexSort, Box<SortExpression>),
    /// Resolved reference to a sort after name resolution
    Resolved(String, SortId),
    /// Function sort (A_0 # ... # A_n -> B) after flattening (performed during name resolution)
    FlattenedFunction {
        domain: Vec<SortExpression>,
        range: Box<SortExpression>,
    },
}

/// A sort expression paired with the source [Span] it was parsed from.
pub type SortExpression = Spanned<SortExpressionKind>;

impl SortExpressionKind {
    /// Wraps this kind together with a source `span` into a [SortExpression].
    pub fn spanned(self, span: Span) -> SortExpression {
        Spanned { node: self, span }
    }
}

impl From<SortExpressionKind> for SortExpression {
    /// For synthetic expressions that have no source location.
    fn from(kind: SortExpressionKind) -> Self {
        Spanned {
            node: kind,
            span: Span::default(),
        }
    }
}

/// Constructor declaration
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct ConstructorDecl {
    /// The constructor's own name (`c1`), with its declaration span.
    pub name: Spanned<String>,
    /// Each argument's optional projection-function name.
    pub args: Vec<(Option<Spanned<String>>, SortExpression)>,
    /// The recogniser function's name.
    pub projection: Option<Spanned<String>>,
}

/// Built-in simple sorts.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum Sort {
    Bool,
    Pos,
    Int,
    Nat,
    Real,
}

/// Complex (parameterized) sorts.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum ComplexSort {
    List,
    Set,
    FSet,
    FBag,
    Bag,
}
