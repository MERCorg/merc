use std::collections::HashMap;
use std::collections::HashSet;
use std::convert::Infallible;
use std::ops::ControlFlow;

use merc_syntax::DataExpr;
use merc_syntax::FixpointEquation;
use merc_syntax::IdDecl;
use merc_syntax::PropVarInst;
use merc_syntax::SortExpression;
use merc_syntax::SourceMap;
use merc_syntax::Span;
use merc_syntax::Traverse;
use merc_syntax::UntypedEquationSystem;
use merc_syntax::VarId;
use merc_syntax::VarIdAllocator;
use merc_typecheck::DataSpecification;
use merc_typecheck::NumberEncoding;
use merc_typecheck::ResolvedName;
use merc_typecheck::TypingInfo;
use merc_typecheck::internal::ResolvedSortId;
use merc_typecheck::internal::SortReference;
use merc_typecheck::internal::VariableScope;
use merc_typecheck::internal::checking;
use merc_typecheck::internal::checking::Scope;
use merc_typecheck::internal::checking::check_expression_against;
use merc_typecheck::internal::checking::collect_binder_sorts;
use merc_typecheck::internal::checking::declare_variables;
use merc_typecheck::internal::collect_sort_name_references;
use merc_typecheck::internal::declared_span;
use merc_typecheck::internal::push_resolved_name;
use merc_typecheck::internal::push_sort_references;
use merc_typecheck::internal::resolve_in_data_expr;
use merc_typecheck::internal::resolve_in_prop_var_inst;
use merc_utilities::Step;

use crate::EquationSystemError;

/// The kind of equation system, such as a PBES or a PRES, determined by the formulas on the
/// right-hand sides of its equations.
///
/// Type checking only needs to know, for every node of a formula, which of the shapes of
/// [ExprNode] it has; the traversal itself is provided by [Traverse].
pub trait EquationSystem {
    /// The formula on the right-hand side of an equation.
    type Expr: Traverse;

    /// The sort that the data expressions embedded in a formula must have: `Bool` for a PBES,
    /// `Real` for a PRES.
    fn value_sort(data: &DataSpecification) -> ResolvedSortId;

    /// The shape of `expr` itself, ignoring its subformulas.
    fn node(expr: &Self::Expr) -> ExprNode<'_>;

    /// See [EquationSystem::node].
    fn node_mut(expr: &mut Self::Expr) -> ExprNodeMut<'_>;
}

/// The shape of a single formula node, as far as type checking is concerned.
pub enum ExprNode<'a> {
    /// A data expression of the [EquationSystem::value_sort], such as `val(e)`.
    Data(&'a DataExpr),
    /// A propositional variable instantiation `X(e₁, …, eₙ)`.
    Instantiation(&'a PropVarInst),
    /// A binder, such as a quantifier, of the given data variables.
    Binder(&'a [IdDecl]),
    /// Any other node, whose subformulas are checked on their own.
    Other,
}

/// See [ExprNode].
pub enum ExprNodeMut<'a> {
    Data(&'a mut DataExpr),
    Instantiation(&'a mut PropVarInst),
    Binder(&'a mut [IdDecl]),
    Other,
}

/// A type-checked equation system: the data specification plus its `glob`, propositional-variable
/// equations, and `init`, all resolved and checked against it. See the module doc comment for
/// what's in and out of scope.
pub struct Specification<S: EquationSystem> {
    /// The original specification, *minus* its data specification.
    spec: UntypedEquationSystem<S::Expr>,
    data: DataSpecification,
    /// Every checked expression's `TypingInfo`, merged during the construction walk (see
    /// [`Self::typing_info`]).
    typing: TypingInfo,
}

impl<S: EquationSystem> Specification<S> {
    /// Type checks `spec`, using the default number encoding. See [`Self::from_untyped_with`].
    pub fn from_untyped(spec: UntypedEquationSystem<S::Expr>) -> Result<Self, EquationSystemError> {
        Self::from_untyped_with(spec, NumberEncoding::default())
    }

    /// Type checks `spec`: its data specification first (exactly as
    /// [`DataSpecification::from_untyped_with`] does), then its global variables, its
    /// propositional-variable equations' parameters, and every equation's formula and `init`
    /// against them.
    pub fn from_untyped_with(
        mut spec: UntypedEquationSystem<S::Expr>,
        encoding: NumberEncoding,
    ) -> Result<Self, EquationSystemError> {
        // A pure syntactic pass, before anything else needs `spec`.
        resolve_variables::<S>(&mut spec);

        let data_spec = std::mem::take(&mut spec.data_specification);
        // An equation system is not (yet) part of the shared, file-based `%import` pipeline, so its
        // embedded data specification gets its own, throwaway `SourceMap`.
        let mut data = DataSpecification::from_untyped_with(data_spec, encoding, &mut SourceMap::new())?;

        let tables = DeclarationTables::build(&mut data, &spec)?;
        let typing = check_specification::<S>(&mut data, &tables, &spec)?;

        Ok(Specification { spec, data, typing })
    }

    /// The checked data specification.
    pub fn data_specification(&self) -> &DataSpecification {
        &self.data
    }

    /// Consumes `self`, returning the checked data specification.
    pub fn into_data_specification(self) -> DataSpecification {
        self.data
    }

    /// The `glob` declarations, in scope in every equation's formula and in `init`.
    pub fn global_variables(&self) -> &[IdDecl] {
        &self.spec.global_variables
    }

    /// The propositional-variable equations.
    pub fn equations(&self) -> &[FixpointEquation<S::Expr>] {
        &self.spec.equations
    }

    /// The `init` propositional-variable instantiation.
    pub fn init(&self) -> &PropVarInst {
        &self.spec.init
    }

    /// Every checked expression's typing across the *whole* specification: every data equation
    /// (see [`DataSpecification::typing_info`]) plus every expression checked outside the data
    /// specification.
    pub fn typing_info(&self) -> TypingInfo {
        let mut info = self.data.typing_info();
        info.merge(self.typing.clone());
        info
    }
}

/// Resolves every free variable reference in `spec`'s equation bodies and `init`.
pub fn resolve_variables<S: EquationSystem>(spec: &mut UntypedEquationSystem<S::Expr>) {
    let mut ids = VarIdAllocator::default();
    let globals = VariableScope::from_declarations(&mut spec.global_variables, &mut ids);

    for equation in &mut spec.equations {
        let mut scope = globals.clone();
        scope.push_declarations(&mut equation.variable.parameters, &mut ids);
        resolve_in_expr::<S>(&mut equation.formula, &mut scope, &mut ids);
    }

    // `init` sits outside every equation's own parameter scope — only globals apply.
    let mut scope = globals;
    resolve_in_prop_var_inst(&mut spec.init, &mut scope, &mut ids);
}

fn resolve_in_expr<S: EquationSystem>(expr: &mut S::Expr, scope: &mut VariableScope, ids: &mut VarIdAllocator) {
    // The number of variables every enclosing node pushed onto `scope`, popped again once its
    // subtree is done, since `exit` does not get to see the node itself.
    let mut state = (scope, Vec::new());
    let _: Result<Option<Infallible>, Infallible> = expr.apply_scoped(
        (),
        &mut state,
        |expr, (), (scope, pushed)| {
            let count = match S::node_mut(expr) {
                ExprNodeMut::Data(data_expr) => {
                    resolve_in_data_expr(data_expr, scope, ids);
                    0
                }
                ExprNodeMut::Instantiation(inst) => {
                    resolve_in_prop_var_inst(inst, scope, ids);
                    0
                }
                ExprNodeMut::Binder(variables) => scope.push_declarations(variables, ids),
                ExprNodeMut::Other => 0,
            };
            pushed.push(count);
            Ok(ControlFlow::Continue(Step::Into(())))
        },
        |(), (scope, pushed)| scope.pop(pushed.pop().expect("every exit follows an enter")),
    );
}

/// The resolved global-variable and per-equation tables, built once by [`Self::build`] and used by
/// [`check_specification`]'s scoped walk to resolve every `PropVarInst` it reaches.
struct DeclarationTables {
    /// Resolved sort of each `glob` declaration, parallel to `spec.global_variables`.
    global_sorts: Vec<ResolvedSortId>,
    /// The resolved declaration of each equation, parallel to `spec.equations`.
    equations: Vec<EquationDeclaration>,
    /// Propositional-variable name -> index into `spec.equations`/`equations`.
    equations_by_name: HashMap<String, usize>,
}

/// The resolved declaration `X(d₁: D₁, …, dₙ: Dₙ)` of one equation.
struct EquationDeclaration {
    /// The span of the propositional variable's identifier `X`.
    identifier_span: Span,
    /// The resolved sort `Dᵢ` of each parameter, in declaration order.
    parameter_sorts: Vec<ResolvedSortId>,
}

impl DeclarationTables {
    fn build<E>(data: &mut DataSpecification, spec: &UntypedEquationSystem<E>) -> Result<Self, EquationSystemError> {
        let mut global_sorts = Vec::with_capacity(spec.global_variables.len());
        let mut seen_globals = HashSet::new();
        for decl in &spec.global_variables {
            if !seen_globals.insert(decl.identifier.as_str()) {
                return Err(EquationSystemError::DuplicateGlobalVariable {
                    name: decl.identifier.node.clone(),
                    span: decl.identifier.span.clone(),
                });
            }
            global_sorts.push(resolve_declared_sort(data, &decl.sort)?);
        }

        let mut equations = Vec::with_capacity(spec.equations.len());
        let mut equations_by_name: HashMap<String, usize> = HashMap::new();
        for (index, eqn) in spec.equations.iter().enumerate() {
            let mut parameter_sorts = Vec::with_capacity(eqn.variable.parameters.len());
            let mut seen = HashSet::new();
            for param in &eqn.variable.parameters {
                if !seen.insert(param.identifier.as_str()) {
                    return Err(EquationSystemError::DuplicateEquationParameter {
                        equation: eqn.variable.identifier.node.clone(),
                        name: param.identifier.node.clone(),
                        span: param.identifier.span.clone(),
                    });
                }
                parameter_sorts.push(resolve_declared_sort(data, &param.sort)?);
            }

            if equations_by_name
                .insert(eqn.variable.identifier.node.clone(), index)
                .is_some()
            {
                return Err(EquationSystemError::DuplicatePropositionalVariable {
                    name: eqn.variable.identifier.node.clone(),
                    span: eqn.variable.identifier.span.clone(),
                });
            }
            equations.push(EquationDeclaration {
                identifier_span: eqn.variable.identifier.span.clone(),
                parameter_sorts,
            });
        }

        Ok(DeclarationTables {
            global_sorts,
            equations,
            equations_by_name,
        })
    }
}

/// Resolves a sort expression occurring in a `glob`/equation-parameter declaration or a binder, see
/// [`checking::resolve_declared_sort`].
fn resolve_declared_sort(
    data: &mut DataSpecification,
    sort: &SortExpression,
) -> Result<ResolvedSortId, EquationSystemError> {
    checking::resolve_declared_sort(data, sort, |span| EquationSystemError::AnonymousStructInDeclaration {
        span,
    })
}

/// Checks the equations and `init` of `spec` against its declaration tables, returning typing
/// information for all expressions.
fn check_specification<S: EquationSystem>(
    data: &mut DataSpecification,
    tables: &DeclarationTables,
    spec: &UntypedEquationSystem<S::Expr>,
) -> Result<TypingInfo, EquationSystemError> {
    let mut typing = TypingInfo::default();
    let mut sort_references = Vec::new();

    for decl in &spec.global_variables {
        collect_sort_name_references(&decl.sort, &mut sort_references);
    }

    for eqn in &spec.equations {
        for param in &eqn.variable.parameters {
            collect_sort_name_references(&param.sort, &mut sort_references);
        }
    }

    let mut globals = Vec::new();
    declare_variables(
        data,
        &mut globals,
        &mut typing,
        &spec.global_variables,
        tables.global_sorts.iter().copied(),
    );

    for (eqn, declaration) in spec.equations.iter().zip(&tables.equations) {
        // An equation's own parameters are in scope throughout its formula.
        let mut scope = globals.clone();
        let sorts = declaration.parameter_sorts.iter().copied();
        declare_variables(data, &mut scope, &mut typing, &eqn.variable.parameters, sorts);
        collect_scope::<S>(data, &eqn.formula, &mut scope, &mut sort_references, &mut typing)?;
        check_expr::<S>(data, tables, &scope, &eqn.formula, &mut typing)?;
    }

    // `init` is a bare `PropVarInst`, checked the same way as one appearing inside a formula —
    // scope = globals only, since it sits outside every equation's own parameter scope.
    check_prop_var_inst(data, tables, &globals, &spec.init, &mut typing)?;

    push_sort_references(data, &sort_references, &mut typing);
    Ok(typing)
}

/// Resolves the declared sort of every binder in `expr`, extending `scope`.
fn collect_scope<S: EquationSystem>(
    data: &mut DataSpecification,
    expr: &S::Expr,
    scope: &mut Vec<(VarId, ResolvedSortId)>,
    sort_references: &mut Vec<SortReference>,
    typing: &mut TypingInfo,
) -> Result<(), EquationSystemError> {
    expr.try_visit(|expr| {
        if let ExprNode::Binder(variables) = S::node(expr) {
            collect_binder_sorts(data, scope, sort_references, typing, variables, resolve_declared_sort)?;
        }
        Ok(ControlFlow::Continue(()))
    })
    .map(|_: Option<Infallible>| ())
}

/// Type-checks a formula: every embedded data expression against the [EquationSystem::value_sort]
/// and every `PropVarInst` against the equation table.
fn check_expr<S: EquationSystem>(
    data: &mut DataSpecification,
    tables: &DeclarationTables,
    scope: &Scope,
    expr: &S::Expr,
    typing: &mut TypingInfo,
) -> Result<(), EquationSystemError> {
    expr.try_visit(|expr| {
        match S::node(expr) {
            ExprNode::Data(data_expr) => {
                let sort = S::value_sort(data);
                check_expression_against::<EquationSystemError>(data, scope, data_expr, sort, typing)?;
            }
            ExprNode::Instantiation(inst) => check_prop_var_inst(data, tables, scope, inst, typing)?,
            ExprNode::Binder(_) | ExprNode::Other => {}
        }
        Ok(ControlFlow::Continue(()))
    })
    .map(|_: Option<Infallible>| ())
}

/// Resolves `inst.identifier` against the equation table (`UndeclaredPropositionalVariable` if
/// missing), checks its argument count against the declared parameter count (`ArityMismatch`), and
/// checks each argument against its parameter's sort. On success, also pushes a
/// [`ResolvedName::PropositionalVariable`] at `inst.identifier`'s own span: an equation is never
/// overloaded, so the equation table's single match is the answer.
fn check_prop_var_inst(
    data: &mut DataSpecification,
    tables: &DeclarationTables,
    scope: &Scope,
    inst: &PropVarInst,
    typing: &mut TypingInfo,
) -> Result<(), EquationSystemError> {
    let Some(&index) = tables.equations_by_name.get(&inst.identifier.node) else {
        return Err(EquationSystemError::UndeclaredPropositionalVariable {
            name: inst.identifier.node.clone(),
            span: inst.span.clone(),
            candidates: tables.equations_by_name.keys().cloned().collect(),
        });
    };
    push_resolved_name(
        typing,
        inst.identifier.span.clone(),
        ResolvedName::PropositionalVariable {
            name: inst.identifier.node.clone(),
            declaration: declared_span(&tables.equations[index].identifier_span),
        },
    );

    let sorts = &tables.equations[index].parameter_sorts;
    if inst.arguments.len() != sorts.len() {
        return Err(EquationSystemError::ArityMismatch {
            name: inst.identifier.node.clone(),
            expected: sorts.len(),
            found: inst.arguments.len(),
            span: inst.span.clone(),
        });
    }

    for (arg, sort) in inst.arguments.iter().zip(sorts) {
        check_expression_against::<EquationSystemError>(data, scope, arg, *sort, typing)?;
    }
    Ok(())
}
