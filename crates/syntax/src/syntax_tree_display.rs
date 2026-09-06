use std::fmt;

use itertools::Itertools;

use merc_utilities::Span;

use crate::ActDecl;
use crate::ActFrm;
use crate::ActFrmBinaryOp;
use crate::ActFrmKind;
use crate::Action;
use crate::ActionName;
use crate::Assignment;
use crate::Bound;
use crate::CommExpr;
use crate::ComplexSort;
use crate::ConstructorDecl;
use crate::DataExpr;
use crate::DataExprBinaryOp;
use crate::DataExprKind;
use crate::DataExprUnaryOp;
use crate::DataExprUpdate;
use crate::EqnDecl;
use crate::EqnSpec;
use crate::FixedPointOperator;
use crate::IdDecl;
use crate::ModalityOperator;
use crate::MultiAction;
use crate::MultiActionLabel;
use crate::PbesEquation;
use crate::PbesExpr;
use crate::PbesExprBinaryOp;
use crate::PbesExprKind;
use crate::ProcDecl;
use crate::ProcExprBinaryOp;
use crate::ProcessExpr;
use crate::ProcessExprKind;
use crate::PropVarDecl;
use crate::PropVarInst;
use crate::Quantifier;
use crate::RegFrm;
use crate::RegFrmKind;
use crate::Rename;
use crate::Sort;
use crate::SortDecl;
use crate::SortExpression;
use crate::SortExpressionKind;
use crate::StateFrm;
use crate::StateFrmKind;
use crate::StateFrmOp;
use crate::StateFrmUnaryOp;
use crate::StateVarAssignment;
use crate::StateVarDecl;
use crate::UntypedDataSpecification;
use crate::UntypedPbes;
use crate::UntypedProcessSpecification;
use crate::UntypedStateFrmSpec;

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

// Display implementations
impl fmt::Display for Sort {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl fmt::Display for ComplexSort {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl fmt::Display for Assignment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} = {}", self.identifier, self.expr)
    }
}

impl fmt::Display for UntypedProcessSpecification {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{}", self.data_specification)?;

        if !self.action_declarations.is_empty() {
            writeln!(f, "act")?;
            for act_decl in &self.action_declarations {
                writeln!(f, "   {act_decl};")?;
            }

            writeln!(f)?;
        }

        if !self.process_declarations.is_empty() {
            writeln!(f, "proc")?;
            for proc_decl in &self.process_declarations {
                writeln!(f, "   {proc_decl};")?;
            }

            writeln!(f)?;
        }

        if !self.global_variables.is_empty() {
            writeln!(f, "glob")?;
            for var_decl in &self.global_variables {
                writeln!(f, "   {var_decl};")?;
            }

            writeln!(f)?;
        }

        if let Some(init) = &self.init {
            writeln!(f, "init {init};")?;
        }
        Ok(())
    }
}

impl fmt::Display for UntypedDataSpecification {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if !self.sort_declarations.is_empty() {
            writeln!(f, "sort")?;
            for decl in &self.sort_declarations {
                writeln!(f, "   {decl};")?;
            }

            writeln!(f)?;
        }

        if !self.constructor_declarations.is_empty() {
            writeln!(f, "cons")?;
            for decl in &self.constructor_declarations {
                writeln!(f, "   {decl};")?;
            }

            writeln!(f)?;
        }

        if !self.map_declarations.is_empty() {
            writeln!(f, "map")?;
            for decl in &self.map_declarations {
                writeln!(f, "   {decl};")?;
            }

            writeln!(f)?;
        }

        for decl in &self.equation_declarations {
            writeln!(f, "{decl}")?;
        }
        Ok(())
    }
}

impl fmt::Display for UntypedPbes {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{}", self.data_specification)?;
        writeln!(f)?;
        if !self.global_variables.is_empty() {
            writeln!(f, "glob")?;
            for var_decl in &self.global_variables {
                writeln!(f, "   {var_decl};")?;
            }

            writeln!(f)?;
        }
        writeln!(f)?;

        if !self.equations.is_empty() {
            writeln!(f, "pbes")?;
            for equation in &self.equations {
                writeln!(f, "   {equation};")?;
            }
        }

        writeln!(f, "init {};", self.init)
    }
}

impl fmt::Display for PropVarInst {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.arguments.is_empty() {
            write!(f, "{}", self.identifier)
        } else {
            write!(f, "{}({})", self.identifier, self.arguments.iter().format(", "))
        }
    }
}

impl fmt::Display for PbesEquation {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {} = {}", self.operator, self.variable, self.formula)
    }
}

impl fmt::Display for PropVarDecl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.parameters.is_empty() {
            write!(f, "{}", self.identifier)
        } else {
            write!(f, "{}({})", self.identifier, self.parameters.iter().format(", "))
        }
    }
}

impl fmt::Display for PbesExpr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.node {
            PbesExprKind::True => write!(f, "true"),
            PbesExprKind::False => write!(f, "false"),
            PbesExprKind::PropVarInst(instance) => write!(f, "{instance}"),
            PbesExprKind::Negation(expr) => write!(f, "(! {expr})"),
            PbesExprKind::Binary { op, lhs, rhs } => write!(f, "({lhs} {op} {rhs})"),
            PbesExprKind::Quantifier {
                quantifier,
                variables,
                body,
            } => write!(f, "({} {} . {})", quantifier, variables.iter().format(", "), body),
            PbesExprKind::DataValExpr(data_expr) => write!(f, "val({data_expr})"),
        }
    }
}

impl fmt::Display for PbesExprBinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PbesExprBinaryOp::Conjunction => write!(f, "&&"),
            PbesExprBinaryOp::Disjunction => write!(f, "||"),
            PbesExprBinaryOp::Implies => write!(f, "=>"),
        }
    }
}

impl fmt::Display for EqnSpec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // The grammar requires at least one declaration after `var`, so only
        // emit the section when there are variables to declare.
        if !self.variables.is_empty() {
            writeln!(f, "var")?;
            for decl in &self.variables {
                writeln!(f, "   {decl};")?;
            }
        }

        writeln!(f, "eqn")?;
        for decl in &self.equations {
            writeln!(f, "   {decl};")?;
        }
        Ok(())
    }
}

impl fmt::Display for SortDecl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.identifier)?;

        if let Some(expr) = &self.expr {
            write!(f, " = {expr}")?;
        }

        Ok(())
    }
}

impl fmt::Display for ActDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // An action declaration is `id: sort # sort # ...`, matching the
        // `IdList ~ ":" ~ SortProduct` grammar rule.
        if self.args.is_empty() {
            write!(f, "{}", self.identifier)
        } else {
            write!(f, "{}: {}", self.identifier, self.args.iter().format(" # "))
        }
    }
}

impl fmt::Display for EqnDecl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.condition {
            Some(condition) => write!(f, "{} -> {} = {}", condition, self.lhs, self.rhs),
            None => write!(f, "{} = {}", self.lhs, self.rhs),
        }
    }
}

impl fmt::Display for DataExprUnaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataExprUnaryOp::Negation => write!(f, "!"),
            DataExprUnaryOp::Minus => write!(f, "-"),
            DataExprUnaryOp::Size => write!(f, "#"),
        }
    }
}

impl fmt::Display for DataExpr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.node {
            DataExprKind::EmptyList => write!(f, "[]"),
            DataExprKind::EmptyBag => write!(f, "{{:}}"),
            DataExprKind::EmptySet => write!(f, "{{}}"),
            DataExprKind::List(expressions) => write!(f, "[{}]", expressions.iter().format(", ")),
            DataExprKind::Bag(expressions) => write!(
                f,
                "{{ {} }}",
                expressions
                    .iter()
                    .format_with(", ", |e, f| f(&format_args!("{}: {}", e.expr, e.multiplicity)))
            ),
            DataExprKind::Set(expressions) => write!(f, "{{ {} }}", expressions.iter().format(", ")),
            DataExprKind::Id(identifier) | DataExprKind::Resolved(identifier, _) => write!(f, "{identifier}"),
            DataExprKind::Binary { op, lhs, rhs } => write!(f, "({lhs} {op} {rhs})"),
            DataExprKind::Unary { op, expr } => write!(f, "({op} {expr})"),
            DataExprKind::Bool(value) => write!(f, "{value}"),
            DataExprKind::Quantifier { op, variables, body } => {
                write!(f, "({} {} . {})", op, variables.iter().format(", "), body)
            }
            DataExprKind::Lambda { variables, body } => {
                write!(f, "(lambda {} . {})", variables.iter().format(", "), body)
            }
            DataExprKind::Application { function, arguments } => {
                if arguments.is_empty() {
                    write!(f, "{function}")
                } else {
                    write!(f, "{}({})", function, arguments.iter().format(", "))
                }
            }
            DataExprKind::Number(value) => write!(f, "{value}"),
            DataExprKind::FunctionUpdate { expr, update } => write!(f, "{expr}[{update}]"),
            DataExprKind::SetBagComp { variable, predicate } => write!(f, "{{ {variable} | {predicate} }}"),
            DataExprKind::Whr { expr, assignments } => {
                write!(f, "{} whr {} end", expr, assignments.iter().format(", "))
            }
        }
    }
}

impl<Id> fmt::Display for IdDecl<Id> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: {}", self.identifier.node, self.sort)
    }
}

impl fmt::Display for DataExprUpdate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} -> {}", self.expr, self.update)
    }
}

impl fmt::Display for SortExpression {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.node {
            SortExpressionKind::Product { lhs, rhs } => write!(f, "({lhs} # {rhs})"),
            SortExpressionKind::Function { domain, range } => write!(f, "({domain} -> {range})"),
            SortExpressionKind::Reference(name) => write!(f, "{name}"),
            SortExpressionKind::TypeVar(id) => write!(f, "'{id}"),
            SortExpressionKind::Simple(sort) => write!(f, "{sort}"),
            SortExpressionKind::Complex(complex, inner) => write!(f, "{complex}({inner})"),
            SortExpressionKind::Struct { inner } => {
                write!(f, "struct ")?;
                write!(f, "{}", inner.iter().format(" | "))
            }
            SortExpressionKind::Resolved(name, _id) => write!(f, "{name}"),
            SortExpressionKind::FlattenedFunction { domain, range } => {
                let domain = domain.iter().format(" # ");
                write!(f, "({domain} -> {range})")
            }
        }
    }
}

impl fmt::Display for UntypedStateFrmSpec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{}", self.data_specification)?;

        // Wrap the formula in a `form ...;` section: the bare-formula grammar
        // alternative is only valid when no specification elements precede it.
        writeln!(f, "form {};", self.formula)
    }
}

impl fmt::Display for StateFrmUnaryOp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StateFrmUnaryOp::Minus => write!(f, "-"),
            StateFrmUnaryOp::Negation => write!(f, "!"),
        }
    }
}

impl fmt::Display for FixedPointOperator {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FixedPointOperator::Greatest => write!(f, "nu"),
            FixedPointOperator::Least => write!(f, "mu"),
        }
    }
}

impl fmt::Display for StateFrm {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.node {
            StateFrmKind::True => write!(f, "true"),
            StateFrmKind::False => write!(f, "false"),
            StateFrmKind::DataValExpr(expr) => write!(f, "val({expr})"),
            StateFrmKind::Id(identifier, args) | StateFrmKind::Resolved(identifier, args, _) => {
                if args.is_empty() {
                    write!(f, "{identifier}")
                } else {
                    write!(f, "{}({})", identifier, args.iter().format(", "))
                }
            }
            StateFrmKind::Unary { op, expr } => write!(f, "({op} {expr})"),
            StateFrmKind::Modality {
                operator,
                formula,
                expr,
            } => match operator {
                ModalityOperator::Box => write!(f, "[{formula}]{expr}"),
                ModalityOperator::Diamond => write!(f, "<{formula}>{expr}"),
            },
            StateFrmKind::Quantifier {
                quantifier,
                variables,
                body,
            } => {
                write!(f, "({} {} . {})", quantifier, variables.iter().format(", "), body)
            }
            StateFrmKind::Bound {
                bound: quantifier,
                variables,
                body,
            } => {
                write!(f, "({} {} . {})", quantifier, variables.iter().format(", "), body)
            }
            StateFrmKind::Binary { op, lhs, rhs } => {
                write!(f, "({lhs} {op} {rhs})")
            }
            StateFrmKind::FixedPoint {
                operator,
                variable,
                body,
            } => {
                write!(f, "({operator} {variable} . {body})")
            }
            StateFrmKind::Delay(Some(expr)) => write!(f, "delay@({expr})"),
            StateFrmKind::Delay(None) => write!(f, "delay"),
            StateFrmKind::Yaled(Some(expr)) => write!(f, "yaled@({expr})"),
            StateFrmKind::Yaled(None) => write!(f, "yaled"),
            StateFrmKind::DataValExprLeftMult(value, expr) => write!(f, "(val({value}) * {expr})"),
            StateFrmKind::DataValExprRightMult(expr, value) => write!(f, "({expr} * val({value}))"),
        }
    }
}

impl fmt::Display for StateVarDecl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.arguments.is_empty() {
            write!(f, "{}", self.identifier)
        } else {
            write!(f, "{}({})", self.identifier, self.arguments.iter().format(","))
        }
    }
}

impl fmt::Display for StateVarAssignment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} : {} = {}", self.identifier.node, self.sort, self.expr)
    }
}

impl fmt::Display for StateFrmOp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StateFrmOp::Implies => write!(f, "=>"),
            StateFrmOp::Conjunction => write!(f, "&&"),
            StateFrmOp::Disjunction => write!(f, "||"),
            StateFrmOp::Addition => write!(f, "+"),
        }
    }
}

impl fmt::Display for RegFrm {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.node {
            RegFrmKind::Action(action) => write!(f, "{action}"),
            RegFrmKind::Iteration(body) => write!(f, "({body})*"),
            RegFrmKind::Plus(body) => write!(f, "({body})+"),
            RegFrmKind::Choice { lhs, rhs } => write!(f, "({lhs} + {rhs})"),
            RegFrmKind::Sequence { lhs, rhs } => write!(f, "({lhs} . {rhs})"),
        }
    }
}

impl fmt::Display for DataExprBinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DataExprBinaryOp::At => write!(f, "."),
            DataExprBinaryOp::Concat => write!(f, "++"),
            DataExprBinaryOp::Cons => write!(f, "|>"),
            DataExprBinaryOp::Equal => write!(f, "=="),
            DataExprBinaryOp::NotEqual => write!(f, "!="),
            DataExprBinaryOp::LessThan => write!(f, "<"),
            DataExprBinaryOp::LessEqual => write!(f, "<="),
            DataExprBinaryOp::GreaterThan => write!(f, ">"),
            DataExprBinaryOp::GreaterEqual => write!(f, ">="),
            DataExprBinaryOp::Conj => write!(f, "&&"),
            DataExprBinaryOp::Disj => write!(f, "||"),
            DataExprBinaryOp::Add => write!(f, "+"),
            DataExprBinaryOp::Subtract => write!(f, "-"),
            DataExprBinaryOp::Div => write!(f, "/"),
            DataExprBinaryOp::Implies => write!(f, "=>"),
            DataExprBinaryOp::In => write!(f, "in"),
            DataExprBinaryOp::IntDiv => write!(f, "div"),
            DataExprBinaryOp::Mod => write!(f, "mod"),
            DataExprBinaryOp::Multiply => write!(f, "*"),
            DataExprBinaryOp::Snoc => write!(f, "<|"),
        }
    }
}

impl fmt::Display for ActFrm {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.node {
            ActFrmKind::False => write!(f, "false"),
            ActFrmKind::True => write!(f, "true"),
            ActFrmKind::MultAct(action) => write!(f, "{action}"),
            ActFrmKind::Binary { op, lhs, rhs } => {
                // Wrap the whole expression (not just the operands) so that a
                // surrounding tighter operator such as `!` cannot re-associate.
                write!(f, "({lhs} {op} {rhs})")
            }
            ActFrmKind::DataExprVal(expr) => write!(f, "val({expr})"),
            ActFrmKind::Quantifier {
                quantifier,
                variables,
                body,
            } => write!(f, "({} {} . {})", quantifier, variables.iter().format(", "), body),
            ActFrmKind::Negation(expr) => write!(f, "(!{expr})"),
        }
    }
}

impl fmt::Display for ActFrmBinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ActFrmBinaryOp::Implies => write!(f, "=>"),
            ActFrmBinaryOp::Intersect => write!(f, "&&"),
            ActFrmBinaryOp::Union => write!(f, "||"),
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

impl fmt::Display for MultiAction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.actions.is_empty() {
            write!(f, "tau")
        } else {
            write!(f, "{}", self.actions.iter().format("|"))
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.args.is_empty() {
            write!(f, "{}", self.id)
        } else {
            write!(f, "{}({})", self.id, self.args.iter().format(", "))
        }
    }
}

impl fmt::Display for Quantifier {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Quantifier::Exists => write!(f, "exists"),
            Quantifier::Forall => write!(f, "forall"),
        }
    }
}

impl fmt::Display for ConstructorDecl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.args.is_empty() {
            write!(f, "{}", self.name.node)?;

            if let Some(projection) = &self.projection {
                write!(f, "?{}", projection.node)?;
            }

            Ok(())
        } else {
            write!(f, "{}(", self.name.node)?;
            for (i, (name, sort)) in self.args.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                match name {
                    Some(name) => write!(f, "{} : {sort}", name.node)?,
                    None => write!(f, "{sort}")?,
                }
            }
            write!(f, ")")?;

            if let Some(projection) = &self.projection {
                write!(f, "?{}", projection.node)?;
            }

            Ok(())
        }
    }
}

impl fmt::Display for ProcDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.params.is_empty() {
            write!(f, "{} = {}", self.identifier, self.body)
        } else {
            write!(
                f,
                "{}({}) = {}",
                self.identifier,
                self.params.iter().format(", "),
                self.body
            )
        }
    }
}

impl fmt::Display for ProcExprBinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProcExprBinaryOp::Sequence => write!(f, "."),
            ProcExprBinaryOp::Choice => write!(f, "+"),
            ProcExprBinaryOp::Parallel => write!(f, "||"),
            ProcExprBinaryOp::LeftMerge => write!(f, "||_"),
            ProcExprBinaryOp::CommMerge => write!(f, "|"),
            ProcExprBinaryOp::Until => write!(f, "<<"),
        }
    }
}

impl fmt::Display for ProcessExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.node {
            ProcessExprKind::Id(identifier, assignments) => {
                if assignments.is_empty() {
                    write!(f, "{identifier}")
                } else {
                    write!(f, "{}({})", identifier, assignments.iter().format(", "))
                }
            }
            ProcessExprKind::Action(identifier, data_exprs) => {
                if data_exprs.is_empty() {
                    write!(f, "{identifier}")
                } else {
                    write!(f, "{}({})", identifier, data_exprs.iter().format(", "))
                }
            }
            ProcessExprKind::Delta => write!(f, "delta"),
            ProcessExprKind::Tau => write!(f, "tau"),
            ProcessExprKind::Sum { variables, operand } => {
                write!(f, "(sum {} . {})", variables.iter().format(", "), operand)
            }
            ProcessExprKind::Dist {
                variables,
                expr,
                operand,
            } => write!(f, "(dist {} [{}] . {})", variables.iter().format(", "), expr, operand),
            ProcessExprKind::Binary { op, lhs, rhs } => write!(f, "({lhs} {op} {rhs})"),
            ProcessExprKind::Hide { actions, operand } => {
                if !actions.is_empty() {
                    write!(f, "hide({{{}}}, {})", actions.iter().format(", "), operand)
                } else {
                    Ok(())
                }
            }
            ProcessExprKind::Rename { renames, operand } => {
                if !renames.is_empty() {
                    write!(f, "rename({{{}}}, {})", renames.iter().format(", "), operand)
                } else {
                    Ok(())
                }
            }
            ProcessExprKind::Allow { actions, operand } => {
                if !actions.is_empty() {
                    write!(f, "allow({{{}}}, {})", actions.iter().format(", "), operand)
                } else {
                    Ok(())
                }
            }
            ProcessExprKind::Block { actions, operand } => {
                if !actions.is_empty() {
                    write!(f, "block({{{}}}, {})", actions.iter().format(", "), operand)
                } else {
                    Ok(())
                }
            }
            ProcessExprKind::Comm { comm, operand } => {
                if !comm.is_empty() {
                    write!(f, "comm({{{}}}, {})", comm.iter().format(", "), operand)
                } else {
                    Ok(())
                }
            }
            ProcessExprKind::Condition { condition, then, else_ } => {
                // Wrap the whole conditional so it stays a single unit when it is
                // an operand of a higher-precedence operator such as sequence.
                if let Some(else_) = else_ {
                    write!(f, "(({condition}) -> ({then}) <> ({else_}))")
                } else {
                    write!(f, "(({condition}) -> ({then}))")
                }
            }
            ProcessExprKind::At { expr, operand } => write!(f, "({expr})@({operand})"),
        }
    }
}

impl fmt::Display for CommExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} -> {}", self.from, self.to)
    }
}

impl fmt::Display for Rename {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} -> {}", self.from, self.to)
    }
}

impl fmt::Display for ActionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.node)
    }
}

impl fmt::Display for MultiActionLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.actions.is_empty() {
            write!(f, "tau")
        } else {
            write!(f, "{}", self.actions.iter().format("|"))
        }
    }
}
