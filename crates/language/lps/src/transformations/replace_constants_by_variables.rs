use std::collections::HashMap;

use merc_aterm::ATerm;
use merc_aterm::Term;
use merc_data::DataApplication;
use merc_data::DataExpression;
use merc_data::DataExpressionKind;
use merc_data::DataVariable;
use merc_data::SortArrow;
use merc_data::SortExpression;
use merc_data::is_closed;
use merc_enumerate::FreshVariableGenerator;
use merc_sabre::DataRewriter;
use merc_sabre::RewriteEngine;

use crate::LinearProcessSpecification;

/// Replaces every maximal closed application in the summands by a variable that does not occur in
/// `lps` yet, returning the normal forms of these variables.
pub fn replace_constants_by_variables(
    mut lps: LinearProcessSpecification,
    rewriter: &mut DataRewriter,
) -> (LinearProcessSpecification, Vec<(DataVariable, DataExpression)>) {
    let mut replacer = ConstantReplacer {
        rewriter,
        generator: FreshVariableGenerator::new("@rewr_var", lps.variable_names()),
        substitutions: HashMap::new(),
        assignments: Vec::new(),
    };

    for summand in &mut lps.action_summands {
        summand.condition = replacer.replace(&summand.condition);
        for value in summand.next_state.iter_mut().flatten() {
            *value = replacer.replace(value);
        }
        for action in &mut summand.action.actions {
            for argument in &mut action.arguments {
                *argument = replacer.replace(argument);
            }
        }
        if let Some(time) = &mut summand.action.time {
            *time = replacer.replace(time);
        }
    }
    for summand in &mut lps.deadlock_summands {
        summand.condition = replacer.replace(&summand.condition);
    }

    let assignments = replacer.assignments;
    (lps, assignments)
}

/// Replaces maximal closed applications by variables.
struct ConstantReplacer<'a> {
    rewriter: &'a mut DataRewriter,
    generator: FreshVariableGenerator,
    substitutions: HashMap<DataExpression, DataVariable>,
    assignments: Vec<(DataVariable, DataExpression)>,
}

impl ConstantReplacer<'_> {
    fn replace(&mut self, expression: &DataExpression) -> DataExpression {
        let DataExpressionKind::Application(application) = expression.kind() else {
            return expression.clone();
        };

        if let Some(variable) = self.substitutions.get(expression) {
            return variable.clone().into();
        }

        if is_closed(expression) {
            let variable = self.generator.generate(application_sort(expression).copy());
            let value = self.rewriter.rewrite(expression);
            self.substitutions.insert(expression.clone(), variable.clone());
            self.assignments.push((variable.clone(), value));
            return variable.into();
        }

        let head = DataExpression::from(expression.arg(0).protect());
        let arguments: Vec<DataExpression> = application
            .data_arguments()
            .map(|argument| self.replace(&DataExpression::from(argument.protect())))
            .collect();
        DataApplication::with_args(&head, &arguments).into()
    }
}

/// The sort of an application, or of a function symbol or variable at its head: the codomain of the
/// sort of its head.
fn application_sort(expression: &DataExpression) -> SortExpression {
    match expression.kind() {
        DataExpressionKind::Application(_) => {
            let head = application_sort(&DataExpression::from(expression.arg(0).protect()));
            SortArrow::from(ATerm::from(head)).codomain().protect()
        }
        _ => expression.data_sort(),
    }
}
