use merc_syntax::DataExpr;
use merc_syntax::DataExprKind;
use merc_syntax::IdDecl;
#[cfg(feature = "internal")]
use merc_syntax::PropVarInst;
use merc_syntax::UntypedDataSpecification;
use merc_syntax::VarId;
use merc_syntax::VarIdAllocator;
use merc_syntax::respan;
use merc_utilities::Span;

/// Resolves every free variable reference in a standalone expression's
/// own local binders: every binder `expr` declares is local to `expr` itself,
/// so resolution starts from an empty [Scope], exactly as it would for a fresh
/// `var`-block-less equation.
pub(crate) fn resolve_data_expr_variables(expr: &mut DataExpr) {
    let mut ids = VarIdAllocator::default();
    let mut scope = Scope::default();
    resolve_in_data_expr(expr, &mut scope, &mut ids);
}

/// Resolves every free variable reference in `spec`'s own `var`-block equations.
pub(crate) fn resolve_data_specification_variables(spec: &mut UntypedDataSpecification) {
    let mut ids = VarIdAllocator::default();

    for eqn_spec in &mut spec.equation_declarations {
        let mut scope = Scope::from_declarations(&mut eqn_spec.variables, &mut ids);

        for equation in &mut eqn_spec.equations {
            if let Some(condition) = &mut equation.condition {
                resolve_in_data_expr(condition, &mut scope, &mut ids);
            }

            resolve_in_data_expr(&mut equation.lhs, &mut scope, &mut ids);
            resolve_in_data_expr(&mut equation.rhs, &mut scope, &mut ids);
        }
    }
}

/// A stack of `(name, id)` bindings supporting shadowing lookup: the innermost (most recently
/// pushed) binding for a name wins, and dropping back to an outer scope is a cheap truncate.
/// Shared by [Scope] (`VarId`-keyed data/action binders) and `merc_formula`'s scope of the
/// `StateVarId`-keyed fixpoint variables — the two id namespaces variable resolution tracks.
#[derive(Clone)]
pub struct NameStack<Id>(Vec<(String, Id)>);

impl<Id> Default for NameStack<Id> {
    fn default() -> Self {
        NameStack(Vec::new())
    }
}

impl<Id: Clone> NameStack<Id> {
    /// Pushes one `(name, id)` binding.
    pub fn push(&mut self, name: String, id: Id) {
        self.0.push((name, id));
    }

    /// Drops the `count` most recently pushed bindings, restoring the stack to what it was
    /// before they were pushed.
    pub fn pop(&mut self, count: usize) {
        self.0.truncate(self.0.len() - count);
    }

    /// The innermost binding named `name`, if one is in scope.
    pub fn resolve(&self, name: &str) -> Option<Id> {
        self.0
            .iter()
            .rev()
            .find(|(bound, _)| bound == name)
            .map(|(_, id)| id.clone())
    }
}

/// The binders currently in scope, each paired with its declaration's own [VarId] and span, so two
/// occurrences of the same binder keep comparing equal once rewritten to
/// [`DataExprKind::Resolved`], and each occurrence knows where its binder is declared.
pub type Scope = NameStack<(VarId, Span)>;

impl Scope {
    /// Builds a scope from a binder's own declarations, assigning each a fresh [VarId].
    pub fn from_declarations<'a, Id: 'a, I>(variables: I, ids: &mut VarIdAllocator) -> Self
    where
        I: IntoIterator<Item = &'a mut IdDecl<Id>>,
    {
        let mut scope = Scope::default();
        scope.push_declarations(variables, ids);
        scope
    }

    /// Pushes each declaration in `variables` onto the scope, assigning it a fresh [VarId], and
    /// returns how many were pushed so the caller can [`Scope::pop`] them back off once its
    /// subtree is done. Each binder is declared at its identifier's span.
    pub fn push_declarations<'a, Id: 'a, I>(&mut self, variables: I, ids: &mut VarIdAllocator) -> usize
    where
        I: IntoIterator<Item = &'a mut IdDecl<Id>>,
    {
        let mut pushed = 0;
        for variable in variables {
            variable.var_id =
                Some(self.declare(variable.identifier.node.clone(), variable.identifier.span.clone(), ids));
            pushed += 1;
        }
        pushed
    }

    /// Declares a single binder at `declaration`: allocates it a fresh [VarId] and pushes `name`
    /// onto the scope under that id.
    fn declare(&mut self, name: String, declaration: Span, ids: &mut VarIdAllocator) -> VarId {
        let id = ids.alloc();
        self.push(name, (id, declaration));
        id
    }
}

#[cfg(feature = "internal")]
pub fn resolve_in_prop_var_inst(inst: &mut PropVarInst, scope: &mut Scope, ids: &mut VarIdAllocator) {
    for argument in &mut inst.arguments {
        resolve_in_data_expr(argument, scope, ids);
    }
}

/// Rewrites every `Id(name)` in `expr` found in `scope` into `Resolved(name, binder)`, extending
/// `scope` (and allocating from `ids`) for the data-level binders it descends through (`lambda`, a
/// quantifier, a set/bag comprehension, `whr`).
pub fn resolve_in_data_expr(expr: &mut DataExpr, scope: &mut Scope, ids: &mut VarIdAllocator) {
    match &mut expr.node {
        DataExprKind::Id(name) => {
            if let Some((id, declaration)) = scope.resolve(name) {
                expr.node = DataExprKind::Resolved(name.clone(), respan(declaration, id));
            }
        }
        DataExprKind::Resolved(_, _)
        | DataExprKind::Number(_)
        | DataExprKind::Bool(_)
        | DataExprKind::EmptyList
        | DataExprKind::EmptySet
        | DataExprKind::EmptyBag => {}
        DataExprKind::Application { function, arguments } => {
            resolve_in_data_expr(function, scope, ids);
            for argument in arguments {
                resolve_in_data_expr(argument, scope, ids);
            }
        }
        DataExprKind::List(elements) | DataExprKind::Set(elements) => {
            for element in elements {
                resolve_in_data_expr(element, scope, ids);
            }
        }
        DataExprKind::Bag(elements) => {
            for element in elements {
                resolve_in_data_expr(&mut element.expr, scope, ids);
                resolve_in_data_expr(&mut element.multiplicity, scope, ids);
            }
        }
        DataExprKind::SetBagComp { variable, predicate } => {
            let pushed = scope.push_declarations(std::slice::from_mut(variable), ids);
            resolve_in_data_expr(predicate, scope, ids);
            scope.pop(pushed);
        }
        DataExprKind::Lambda { variables, body } | DataExprKind::Quantifier { variables, body, .. } => {
            let pushed = scope.push_declarations(variables, ids);
            resolve_in_data_expr(body, scope, ids);
            scope.pop(pushed);
        }
        DataExprKind::Unary { expr, .. } => resolve_in_data_expr(expr, scope, ids),
        DataExprKind::Binary { lhs, rhs, .. } => {
            resolve_in_data_expr(lhs, scope, ids);
            resolve_in_data_expr(rhs, scope, ids);
        }
        DataExprKind::FunctionUpdate { expr, update } => {
            resolve_in_data_expr(expr, scope, ids);
            resolve_in_data_expr(&mut update.expr, scope, ids);
            resolve_in_data_expr(&mut update.update, scope, ids);
        }
        DataExprKind::Whr { expr, assignments } => {
            // Each assignment's right-hand side is resolved in the *outer* scope — bindings
            // don't see each other, only the body does.
            for assignment in assignments.iter_mut() {
                resolve_in_data_expr(&mut assignment.expr, scope, ids);
            }
            let pushed = assignments.len();
            for assignment in assignments.iter_mut() {
                assignment.id = Some(scope.declare(assignment.identifier.clone(), assignment.span.clone(), ids));
            }
            resolve_in_data_expr(expr, scope, ids);
            scope.pop(pushed);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::hash::DefaultHasher;
    use std::hash::Hash;
    use std::hash::Hasher;

    use merc_syntax::DataExprKind;
    use merc_syntax::OffsetSpans;
    use merc_syntax::UntypedDataSpecification;

    use super::resolve_data_specification_variables;

    /// Rebasing a resolved tree moves every binder's declaration span, but must leave its
    /// expressions equal to, and hashing the same as, the originals.
    #[test]
    #[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
    fn test_rebased_resolved_expression_keeps_its_identity() {
        let text = "map f: Nat -> Nat; var n: Nat; eqn f(n) = n;";
        let mut spec = UntypedDataSpecification::parse(text).unwrap();
        resolve_data_specification_variables(&mut spec);

        let mut rebased = spec.clone();
        rebased.offset_spans(100);

        let original = &spec.equation_declarations[0].equations[0].rhs;
        let shifted = &rebased.equation_declarations[0].equations[0].rhs;
        let (DataExprKind::Resolved(_, before), DataExprKind::Resolved(_, after)) = (&original.node, &shifted.node)
        else {
            panic!("expected the right-hand side to resolve to the equation variable");
        };
        assert_eq!(
            after.span.start,
            before.span.start + 100,
            "the declaration span is rebased"
        );

        assert_eq!(original, shifted);
        let hash = |expr: &merc_syntax::DataExpr| {
            let mut hasher = DefaultHasher::new();
            expr.hash(&mut hasher);
            hasher.finish()
        };
        assert_eq!(hash(original), hash(shifted));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
    fn test_data_specification_equation_variable_resolves_to_its_var_block_declaration() {
        let text = "map f: Nat -> Nat; var x: Nat; eqn f(x) = x;";
        let mut spec = UntypedDataSpecification::parse(text).unwrap();
        resolve_data_specification_variables(&mut spec);

        let declared = spec.equation_declarations[0].variables[0]
            .var_id
            .expect("the var-block variable was assigned a VarId");
        let equation = &spec.equation_declarations[0].equations[0];
        assert!(matches!(
            &equation.lhs.node,
            DataExprKind::Application { arguments, .. }
                if matches!(&arguments[0].node, DataExprKind::Resolved(name, var_id) if name == "x" && **var_id == declared)
        ));
        assert!(matches!(
            &equation.rhs.node,
            DataExprKind::Resolved(name, var_id) if name == "x" && **var_id == declared
        ));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
    fn test_data_specification_equation_condition_resolves_too() {
        let text = "map f: Bool -> Nat; var x: Bool; eqn x -> f(x) = 0;";
        let mut spec = UntypedDataSpecification::parse(text).unwrap();
        resolve_data_specification_variables(&mut spec);

        let declared = spec.equation_declarations[0].variables[0]
            .var_id
            .expect("the var-block variable was assigned a VarId");
        let equation = &spec.equation_declarations[0].equations[0];
        let condition = equation.condition.as_ref().expect("expected a condition");
        assert!(matches!(
            &condition.node,
            DataExprKind::Resolved(name, var_id) if name == "x" && **var_id == declared
        ));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
    fn test_data_specification_variable_scope_does_not_leak_across_var_blocks() {
        let text = "map f: Nat -> Nat; g: Bool -> Nat; var x: Nat; eqn f(x) = x; var y: Bool; eqn g(y) = y;";
        let mut spec = UntypedDataSpecification::parse(text).unwrap();
        resolve_data_specification_variables(&mut spec);

        // `y` isn't declared in the first block; it's a separate `var` block's own variable, so
        // this pass leaves it as-is when scoped to the first block. Only checking the second
        // block resolves correctly, and that the two blocks' variables never share an id, are the
        // interesting assertions here.
        let x_declared = spec.equation_declarations[0].variables[0].var_id.unwrap();
        let y_declared = spec.equation_declarations[1].variables[0]
            .var_id
            .expect("the second block's variable was assigned a VarId");
        assert_ne!(x_declared, y_declared);
        let second = &spec.equation_declarations[1].equations[0];
        assert!(matches!(
            &second.rhs.node,
            DataExprKind::Resolved(name, var_id) if name == "y" && **var_id == y_declared
        ));
    }
}
