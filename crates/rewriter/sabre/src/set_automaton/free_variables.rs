use std::convert::Infallible;
use std::ops::ControlFlow;

use ahash::HashSet;
use merc_data::DataAbstractionRef;
use merc_data::DataExpression;
use merc_data::DataExpressionVisitor;
use merc_data::DataVariable;
use merc_data::DataVariableRef;
use merc_data::DataWhereClauseRef;
use merc_utilities::Step;
use merc_utilities::Visit;

/// Returns the free variables of `expr`: occurrences bound by an enclosing binder or where
/// clause are skipped.
pub(crate) fn free_variables(expr: &DataExpression) -> impl Iterator<Item = DataVariable> + use<> {
    /// The context is the innermost scope, an index into `scopes`, which forms a linked list of
    /// the variables bound around the visited node.
    struct FreeVariables {
        scopes: Vec<(DataVariable, Option<usize>)>,
        free: HashSet<DataVariable>,
    }

    impl FreeVariables {
        fn bind(&mut self, variable: DataVariable, scope: Option<usize>) -> Option<usize> {
            self.scopes.push((variable, scope));
            Some(self.scopes.len() - 1)
        }

        fn is_bound(&self, variable: &DataVariableRef<'_>, mut scope: Option<usize>) -> bool {
            while let Some(index) = scope {
                let (bound, parent) = &self.scopes[index];
                if bound.copy() == *variable {
                    return true;
                }
                scope = *parent;
            }
            false
        }
    }

    impl DataExpressionVisitor for FreeVariables {
        type Context = Option<usize>;
        type Break = Infallible;
        type Error = Infallible;

        fn visit_variable(
            &mut self,
            variable: &DataVariableRef<'_>,
            scope: Option<usize>,
        ) -> Visit<Infallible, Option<usize>, Infallible, Infallible> {
            if !self.is_bound(variable, scope) {
                self.free.insert(variable.protect());
            }
            Ok(ControlFlow::Continue(Step::Into(scope)))
        }

        fn visit_binder(
            &mut self,
            binder: &DataAbstractionRef<'_>,
            mut scope: Option<usize>,
        ) -> Visit<Infallible, Option<usize>, Infallible, Infallible> {
            for variable in binder.variables() {
                scope = self.bind(variable, scope);
            }
            Ok(ControlFlow::Continue(Step::Into(scope)))
        }

        fn visit_where_clause(
            &mut self,
            clause: &DataWhereClauseRef<'_>,
            scope: Option<usize>,
        ) -> Visit<Infallible, Option<usize>, Infallible, Infallible> {
            // The declared values are in the enclosing scope, only the body is in the scope of
            // the declared variables, so the two are visited here with different contexts.
            let mut body_scope = scope;
            for declaration in clause.declarations() {
                self.visit(&declaration.expression(), scope);
                body_scope = self.bind(declaration.variable().protect(), body_scope);
            }
            self.visit(&clause.body(), body_scope);
            Ok(ControlFlow::Continue(Step::Prune))
        }
    }

    let mut visitor = FreeVariables {
        scopes: Vec::new(),
        free: HashSet::default(),
    };
    visitor.visit(&expr.copy(), None);
    visitor.free.into_iter()
}
