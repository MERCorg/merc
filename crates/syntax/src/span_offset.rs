//! Shifts every [`Span`] reachable from a parsed tree by a fixed `delta` — the rebasing
//! counterpart of padding a file's text with `delta` leading bytes before handing it to pest so
//! every offset it reports already lands in the shared, [`SourceMap`](merc_utilities::SourceMap)
//! wide space. Parsing the unpadded text and then shifting every span here in one pass is both
//! cheaper (no leading-byte padding to allocate and scan) and lets a caller reuse an already-parsed
//! tree — clone it and shift the clone — instead of re-parsing the same text at a new base offset.
//!
//! [`Traverse`](crate::Traverse) cannot do this on its own: its recursion only ever descends into
//! children of the *same* node type (a [`SortExpression`]'s children are other `SortExpression`s),
//! so it never reaches a declaration's own span, an identifier's [`Spanned`] name, or any other
//! differently-typed field that also carries a span. [`OffsetSpans`] walks every such field
//! explicitly instead.

use crate::ActDecl;
use crate::ActFrm;
use crate::ActFrmKind;
use crate::Assignment;
use crate::BagElement;
use crate::ConstructorDecl;
use crate::DataExpr;
use crate::DataExprKind;
use crate::EqnDecl;
use crate::EqnSpec;
use crate::IdDecl;
use crate::MultiAction;
use crate::ProcDecl;
use crate::ProcessExpr;
use crate::ProcessExprKind;
use crate::RegFrm;
use crate::RegFrmKind;
use crate::SortDecl;
use crate::SortExpression;
use crate::SortExpressionKind;
use crate::StateFrm;
use crate::StateFrmKind;
use crate::StateVarAssignment;
use crate::StateVarDecl;
use crate::UntypedDataSpecification;
use crate::UntypedProcessSpecification;
use crate::UntypedStateFrmSpec;

/// Implemented by every top-level parsed specification [`crate::imports`] and the bundled/generated
/// template machinery need to rebase into a shared [`SourceMap`](merc_utilities::SourceMap).
pub trait OffsetSpans {
    /// Shifts every span reachable from `self` by `delta`.
    fn offset_spans(&mut self, delta: usize);
}

impl OffsetSpans for UntypedDataSpecification {
    fn offset_spans(&mut self, delta: usize) {
        for decl in &mut self.sort_declarations {
            offset_sort_decl(decl, delta);
        }
        for decl in &mut self.constructor_declarations {
            offset_id_decl(decl, delta);
        }
        for decl in &mut self.map_declarations {
            offset_id_decl(decl, delta);
        }
        for eqn_spec in &mut self.equation_declarations {
            offset_eqn_spec(eqn_spec, delta);
        }
        for decl in &mut self.type_var_declarations {
            decl.span.shift(delta);
        }
    }
}

impl OffsetSpans for UntypedProcessSpecification {
    fn offset_spans(&mut self, delta: usize) {
        self.data_specification.offset_spans(delta);
        for decl in &mut self.global_variables {
            offset_id_decl(decl, delta);
        }
        for decl in &mut self.action_declarations {
            offset_act_decl(decl, delta);
        }
        for decl in &mut self.process_declarations {
            offset_proc_decl(decl, delta);
        }
        if let Some(init) = &mut self.init {
            offset_process_expr(init, delta);
        }
    }
}

impl OffsetSpans for UntypedStateFrmSpec {
    fn offset_spans(&mut self, delta: usize) {
        self.data_specification.offset_spans(delta);
        for decl in &mut self.action_declarations {
            offset_act_decl(decl, delta);
        }
        offset_state_frm(&mut self.formula, delta);
    }
}

fn offset_sort_decl(decl: &mut SortDecl, delta: usize) {
    decl.span.shift(delta);
    if let Some(expr) = &mut decl.expr {
        offset_sort_expression(expr, delta);
    }
}

fn offset_id_decl<Id>(decl: &mut IdDecl<Id>, delta: usize) {
    decl.identifier.span.shift(delta);
    offset_sort_expression(&mut decl.sort, delta);
}

fn offset_sort_expression(sort: &mut SortExpression, delta: usize) {
    sort.span.shift(delta);
    match &mut sort.node {
        SortExpressionKind::Product { lhs, rhs } => {
            offset_sort_expression(lhs, delta);
            offset_sort_expression(rhs, delta);
        }
        SortExpressionKind::Function { domain, range } => {
            offset_sort_expression(domain, delta);
            offset_sort_expression(range, delta);
        }
        SortExpressionKind::FlattenedFunction { domain, range } => {
            for sort in domain {
                offset_sort_expression(sort, delta);
            }
            offset_sort_expression(range, delta);
        }
        SortExpressionKind::Struct { inner } => {
            for constructor in inner {
                offset_constructor_decl(constructor, delta);
            }
        }
        SortExpressionKind::Complex(_, sort) => offset_sort_expression(sort, delta),
        SortExpressionKind::Reference(_)
        | SortExpressionKind::TypeVar(_)
        | SortExpressionKind::ResolvedTypeVar(_)
        | SortExpressionKind::Simple(_)
        | SortExpressionKind::Resolved(_, _) => {}
    }
}

fn offset_constructor_decl(constructor: &mut ConstructorDecl, delta: usize) {
    constructor.name.span.shift(delta);
    for (name, sort) in &mut constructor.args {
        if let Some(name) = name {
            name.span.shift(delta);
        }
        offset_sort_expression(sort, delta);
    }
    if let Some(projection) = &mut constructor.projection {
        projection.span.shift(delta);
    }
}

fn offset_eqn_spec(eqn_spec: &mut EqnSpec, delta: usize) {
    eqn_spec.span.shift(delta);
    for variable in &mut eqn_spec.variables {
        offset_id_decl(variable, delta);
    }
    for equation in &mut eqn_spec.equations {
        offset_eqn_decl(equation, delta);
    }
}

fn offset_eqn_decl(equation: &mut EqnDecl, delta: usize) {
    equation.span.shift(delta);
    if let Some(condition) = &mut equation.condition {
        offset_data_expr(condition, delta);
    }
    offset_data_expr(&mut equation.lhs, delta);
    offset_data_expr(&mut equation.rhs, delta);
}

fn offset_data_expr(expr: &mut DataExpr, delta: usize) {
    expr.span.shift(delta);
    match &mut expr.node {
        DataExprKind::Id(_)
        | DataExprKind::Resolved(_, _)
        | DataExprKind::Number(_)
        | DataExprKind::Bool(_)
        | DataExprKind::EmptyList
        | DataExprKind::EmptySet
        | DataExprKind::EmptyBag => {}
        DataExprKind::Application { function, arguments } => {
            offset_data_expr(function, delta);
            for argument in arguments {
                offset_data_expr(argument, delta);
            }
        }
        DataExprKind::List(exprs) | DataExprKind::Set(exprs) => {
            for expr in exprs {
                offset_data_expr(expr, delta);
            }
        }
        DataExprKind::Bag(elements) => {
            for element in elements {
                offset_bag_element(element, delta);
            }
        }
        DataExprKind::SetBagComp { variable, predicate } => {
            offset_id_decl(variable, delta);
            offset_data_expr(predicate, delta);
        }
        DataExprKind::Lambda { variables, body } | DataExprKind::Quantifier { variables, body, .. } => {
            for variable in variables {
                offset_id_decl(variable, delta);
            }
            offset_data_expr(body, delta);
        }
        DataExprKind::Unary { expr, .. } => offset_data_expr(expr, delta),
        DataExprKind::Binary { lhs, rhs, .. } => {
            offset_data_expr(lhs, delta);
            offset_data_expr(rhs, delta);
        }
        DataExprKind::FunctionUpdate { expr, update } => {
            offset_data_expr(expr, delta);
            offset_data_expr(&mut update.expr, delta);
            offset_data_expr(&mut update.update, delta);
        }
        DataExprKind::Whr { expr, assignments } => {
            offset_data_expr(expr, delta);
            for assignment in assignments {
                offset_assignment(assignment, delta);
            }
        }
    }
}

fn offset_bag_element(element: &mut BagElement, delta: usize) {
    offset_data_expr(&mut element.expr, delta);
    offset_data_expr(&mut element.multiplicity, delta);
}

fn offset_assignment(assignment: &mut Assignment, delta: usize) {
    assignment.span.shift(delta);
    offset_data_expr(&mut assignment.expr, delta);
}

fn offset_act_decl(decl: &mut ActDecl, delta: usize) {
    decl.span.shift(delta);
    decl.identifier.span.shift(delta);
    for sort in &mut decl.args {
        offset_sort_expression(sort, delta);
    }
}

fn offset_proc_decl(decl: &mut ProcDecl, delta: usize) {
    decl.span.shift(delta);
    decl.identifier.span.shift(delta);
    for param in &mut decl.params {
        offset_id_decl(param, delta);
    }
    offset_process_expr(&mut decl.body, delta);
}

fn offset_process_expr(expr: &mut ProcessExpr, delta: usize) {
    expr.span.shift(delta);
    match &mut expr.node {
        ProcessExprKind::Delta | ProcessExprKind::Tau => {}
        ProcessExprKind::Id(name, assignments) => {
            name.span.shift(delta);
            for assignment in assignments {
                offset_assignment(assignment, delta);
            }
        }
        ProcessExprKind::Action(name, args) => {
            name.span.shift(delta);
            for arg in args {
                offset_data_expr(arg, delta);
            }
        }
        ProcessExprKind::Sum { variables, operand } => {
            for variable in variables {
                offset_id_decl(variable, delta);
            }
            offset_process_expr(operand, delta);
        }
        ProcessExprKind::Dist {
            variables,
            expr,
            operand,
        } => {
            for variable in variables {
                offset_id_decl(variable, delta);
            }
            offset_data_expr(expr, delta);
            offset_process_expr(operand, delta);
        }
        ProcessExprKind::Binary { lhs, rhs, .. } => {
            offset_process_expr(lhs, delta);
            offset_process_expr(rhs, delta);
        }
        ProcessExprKind::Hide { actions, operand } | ProcessExprKind::Block { actions, operand } => {
            for action in actions {
                action.span.shift(delta);
            }
            offset_process_expr(operand, delta);
        }
        ProcessExprKind::Rename { renames, operand } => {
            for rename in renames {
                rename.from.span.shift(delta);
                rename.to.span.shift(delta);
            }
            offset_process_expr(operand, delta);
        }
        ProcessExprKind::Allow { actions, operand } => {
            for label in actions {
                for action in &mut label.actions {
                    action.span.shift(delta);
                }
            }
            offset_process_expr(operand, delta);
        }
        ProcessExprKind::Comm { comm, operand } => {
            for expr in comm {
                for action in &mut expr.from.actions {
                    action.span.shift(delta);
                }
                expr.to.span.shift(delta);
            }
            offset_process_expr(operand, delta);
        }
        ProcessExprKind::Condition { condition, then, else_ } => {
            offset_data_expr(condition, delta);
            offset_process_expr(then, delta);
            if let Some(operand) = else_ {
                offset_process_expr(operand, delta);
            }
        }
        ProcessExprKind::At { expr, operand } => {
            offset_process_expr(expr, delta);
            offset_data_expr(operand, delta);
        }
    }
}

fn offset_state_frm(formula: &mut StateFrm, delta: usize) {
    formula.span.shift(delta);
    match &mut formula.node {
        StateFrmKind::True | StateFrmKind::False => {}
        StateFrmKind::Delay(time) | StateFrmKind::Yaled(time) => {
            if let Some(expr) = time {
                offset_data_expr(expr, delta);
            }
        }
        StateFrmKind::Id(_, args) | StateFrmKind::Resolved(_, args, _) => {
            for arg in args {
                offset_data_expr(arg, delta);
            }
        }
        StateFrmKind::DataValExprLeftMult(expr, formula) => {
            offset_data_expr(expr, delta);
            offset_state_frm(formula, delta);
        }
        StateFrmKind::DataValExprRightMult(formula, expr) => {
            offset_state_frm(formula, delta);
            offset_data_expr(expr, delta);
        }
        StateFrmKind::DataValExpr(expr) => offset_data_expr(expr, delta),
        StateFrmKind::Modality {
            formula: reg_frm, expr, ..
        } => {
            offset_reg_frm(reg_frm, delta);
            offset_state_frm(expr, delta);
        }
        StateFrmKind::Unary { expr, .. } => offset_state_frm(expr, delta),
        StateFrmKind::Binary { lhs, rhs, .. } => {
            offset_state_frm(lhs, delta);
            offset_state_frm(rhs, delta);
        }
        StateFrmKind::Quantifier { variables, body, .. } | StateFrmKind::Bound { variables, body, .. } => {
            for variable in variables {
                offset_id_decl(variable, delta);
            }
            offset_state_frm(body, delta);
        }
        StateFrmKind::FixedPoint { variable, body, .. } => {
            offset_state_var_decl(variable, delta);
            offset_state_frm(body, delta);
        }
    }
}

fn offset_state_var_decl(decl: &mut StateVarDecl, delta: usize) {
    decl.span.shift(delta);
    for argument in &mut decl.arguments {
        offset_state_var_assignment(argument, delta);
    }
}

fn offset_state_var_assignment(assignment: &mut StateVarAssignment, delta: usize) {
    assignment.identifier.span.shift(delta);
    offset_sort_expression(&mut assignment.sort, delta);
    offset_data_expr(&mut assignment.expr, delta);
}

fn offset_reg_frm(formula: &mut RegFrm, delta: usize) {
    formula.span.shift(delta);
    match &mut formula.node {
        RegFrmKind::Action(act_frm) => offset_act_frm(act_frm, delta),
        RegFrmKind::Iteration(inner) | RegFrmKind::Plus(inner) => offset_reg_frm(inner, delta),
        RegFrmKind::Sequence { lhs, rhs } | RegFrmKind::Choice { lhs, rhs } => {
            offset_reg_frm(lhs, delta);
            offset_reg_frm(rhs, delta);
        }
    }
}

fn offset_act_frm(formula: &mut ActFrm, delta: usize) {
    formula.span.shift(delta);
    match &mut formula.node {
        ActFrmKind::True | ActFrmKind::False => {}
        ActFrmKind::MultAct(multi_action) => offset_multi_action(multi_action, delta),
        ActFrmKind::DataExprVal(expr) => offset_data_expr(expr, delta),
        ActFrmKind::Negation(inner) => offset_act_frm(inner, delta),
        ActFrmKind::Quantifier { variables, body, .. } => {
            for variable in variables {
                offset_id_decl(variable, delta);
            }
            offset_act_frm(body, delta);
        }
        ActFrmKind::Binary { lhs, rhs, .. } => {
            offset_act_frm(lhs, delta);
            offset_act_frm(rhs, delta);
        }
    }
}

fn offset_multi_action(multi_action: &mut MultiAction, delta: usize) {
    for action in &mut multi_action.actions {
        action.id.span.shift(delta);
        for arg in &mut action.args {
            offset_data_expr(arg, delta);
        }
    }
}
