use merc_utilities::IdAllocator;
use merc_utilities::TagIndex;

use crate::spanned::Spanned;

mod actfrm;
mod dataexpr;
mod pbesexpr;
mod presexpr;
mod procexpr;
mod regfrm;
mod sortexpr;
mod specs;
mod statefrm;

pub use actfrm::*;
pub use dataexpr::*;
pub use pbesexpr::*;
pub use presexpr::*;
pub use procexpr::*;
pub use regfrm::*;
pub use sortexpr::*;
pub use specs::*;
pub use statefrm::*;

/// A unique type for sort declarations.
pub struct SortTag;

/// The index type for a sort declaration, assigned during name resolution.
pub type SortId = TagIndex<usize, SortTag>;

/// A unique type for constructor declarations.
pub struct ConstructorTag;

/// The index type for a constructor declaration, local to
/// `UntypedDataSpecification::constructor_declarations`.
pub type ConstructorId = TagIndex<usize, ConstructorTag>;

/// A unique type for map declarations.
pub struct MapTag;

/// The index type for a map declaration, local to
/// `UntypedDataSpecification::map_declarations`.
pub type MapId = TagIndex<usize, MapTag>;

/// A unique type for equation specification blocks (`var ... eqn ...`).
pub struct EqnSpecTag;

/// The index type for an equation specification block, local to
/// `UntypedDataSpecification::equation_declarations`.
pub type EqnSpecId = TagIndex<usize, EqnSpecTag>;

/// A unique type for equation declarations.
pub struct EquationTag;

/// The index type for a single equation, local to its enclosing `EqnSpec`.
pub type EquationId = TagIndex<usize, EquationTag>;

/// A unique type for variable-binder occurrences.
pub struct VarTag;

/// The index type assigned to every variable binder during variable resolution, spec-wide.
pub type VarId = TagIndex<usize, VarTag>;

/// Hands out fresh, spec-wide [VarId]s during variable resolution.
pub type VarIdAllocator = IdAllocator<VarTag>;

/// A unique type for a state-formula fixpoint-variable (`mu X`/`nu X`) binder.
pub struct StateVarTag;

/// The index type assigned to every fixpoint-variable binder during variable resolution,
/// spec-wide, mirroring [VarId] for the propositional namespace.
pub type StateVarId = TagIndex<usize, StateVarTag>;

/// Hands out fresh, spec-wide [StateVarId]s during variable resolution.
pub type StateVarIdAllocator = IdAllocator<StateVarTag>;

/// A unique type for a bound sort (type) variable.
pub struct TypeVarTag;

/// The index type for a bound sort variable.
pub type TypeVarId = TagIndex<usize, TypeVarTag>;

/// An identifier occurrence naming an action or process, paired with the [merc_utilities::Span] it
/// was parsed from, so a later pass can point at the individual name rather than the whole
/// enclosing expression. Used both for a name inside a `hide`/`block`/`allow`/`comm`/`rename` set
/// and for `ProcessExprKind::Action`/`Id`'s own name. Equality, ordering and hashing ignore the
/// span (see [Spanned]).
pub type ActionName = Spanned<String>;

/// A process-declaration identifier occurrence.
pub type ProcessName = Spanned<String>;

/// A propositional-variable identifier occurrence.
pub type PropVarName = Spanned<String>;

/// A declaration of an identifier with its sort.
///
/// Reused for every "name: sort" binding in the grammar. It defaults to [SortId]
/// for the binder-like uses that never assign one, and is instantiated with
/// [ConstructorId] or [MapId] where appropriate.
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct IdDecl<Id = SortId> {
    /// Identifier being declared.
    pub identifier: Spanned<String>,
    /// Sort expression for this identifier
    pub sort: SortExpression,
    /// Unique ID assigned to this declaration during name/id resolution.
    pub id: Option<Id>,
    /// Assigned during variable resolution when this declaration is a variable binder (every
    /// site except a constructor/map declaration, which isn't a variable); `None` otherwise. See
    /// [VarId].
    pub var_id: Option<VarId>,
}

impl<Id> IdDecl<Id> {
    /// Creates a new identifier declaration with the given identifier, sort, and the identifier's
    /// own span.
    pub fn new(identifier: String, sort: SortExpression, span: merc_utilities::Span) -> Self {
        IdDecl {
            identifier: Spanned { node: identifier, span },
            sort,
            id: None,
            var_id: None,
        }
    }

    /// Reinterprets this declaration under a different id type.
    pub fn retag<NewId>(self) -> IdDecl<NewId> {
        IdDecl {
            identifier: self.identifier,
            sort: self.sort,
            id: None,
            var_id: self.var_id,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum Quantifier {
    Exists,
    Forall,
}

// TODO: What should this be called?
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Bound {
    Inf,
    Sup,
    Sum,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum Eq {
    EqInf,
    EqnInf,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum Condition {
    Condsm,
    Condeq,
}
