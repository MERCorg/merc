use merc_lts::TransitionLabel;
use merc_reduction::DistinguishingFormula;
use merc_refinement::CounterExample;
use merc_utilities::Span;

use crate::ActFrmKind;
use crate::Action;
use crate::FixedPointOperator;
use crate::ModalityOperator;
use crate::MultiAction;
use crate::RegFrm;
use crate::RegFrmKind;
use crate::StateFrm;
use crate::StateFrmKind;
use crate::StateFrmOp;
use crate::StateVarDecl;

/// Generates a formula that characterizes the counter example trace.
pub fn generate_refinement_formula<L: TransitionLabel>(counter_example: &CounterExample<L>) -> StateFrm {
    match counter_example {
        CounterExample::Trace(trace) => {
            let mut expr: StateFrm = StateFrmKind::True.into();

            // We build the formula bottom up.
            for label in trace.iter().rev() {
                expr = StateFrmKind::Modality {
                    operator: ModalityOperator::Diamond,
                    formula: RegFrmKind::Action(ActFrmKind::MultAct(label_to_multi_action(label)).into()).into(),
                    expr: Box::new(expr),
                }
                .into()
            }

            expr
        }
        CounterExample::WeakTrace(trace) => {
            weaktrace_formula(trace, StateFrmKind::True.into(), ModalityOperator::Diamond)
        }
        CounterExample::Divergence(trace) => weaktrace_formula(
            trace,
            // For the divergence we use `nu X. <tau>X` to require an infinite tau path.
            StateFrmKind::FixedPoint {
                operator: FixedPointOperator::Greatest,
                variable: StateVarDecl {
                    identifier: "X".to_string(),
                    arguments: Vec::new(),
                    span: Span::default(),
                    id: None,
                },
                body: Box::new(
                    StateFrmKind::Modality {
                        operator: ModalityOperator::Diamond,
                        formula: RegFrmKind::Action(ActFrmKind::MultAct(MultiAction::tau()).into()).into(),
                        expr: Box::new(StateFrmKind::Id("X".to_string(), Vec::new()).into()),
                    }
                    .into(),
                ),
            }
            .into(),
            ModalityOperator::Diamond,
        ),
        CounterExample::StableFailures(trace, refusals) => {
            // Refused actions are characterized by box-false modalities.
            let inner = refusals
                .iter()
                .map(|l| {
                    StateFrmKind::Modality {
                        operator: ModalityOperator::Box,
                        formula: RegFrmKind::Action(ActFrmKind::MultAct(label_to_multi_action(l)).into()).into(),
                        expr: Box::new(StateFrmKind::False.into()),
                    }
                    .into()
                })
                .fold(
                    // Stable failures are only observed in stable states, so tau is refused.
                    StateFrmKind::Modality {
                        operator: ModalityOperator::Box,
                        formula: RegFrmKind::Action(ActFrmKind::MultAct(MultiAction::tau()).into()).into(),
                        expr: Box::new(StateFrmKind::False.into()),
                    }
                    .into(),
                    |acc, expr| {
                        StateFrmKind::Binary {
                            op: StateFrmOp::Conjunction,
                            lhs: Box::new(acc),
                            rhs: Box::new(expr),
                        }
                        .into()
                    },
                );

            weaktrace_formula(trace, inner, ModalityOperator::Diamond)
        }
        CounterExample::ImpossibleFutures(trace, futures) => {
            let expressions = futures
                .iter()
                .map(|future| weaktrace_formula(future, StateFrmKind::False.into(), ModalityOperator::Box))
                .collect::<Vec<_>>();

            // Generate a conjunction of the expressions for each future.
            let expr = expressions.into_iter().fold(StateFrmKind::True.into(), |acc, expr| {
                StateFrmKind::Binary {
                    op: StateFrmOp::Conjunction,
                    lhs: Box::new(acc),
                    rhs: Box::new(expr),
                }
                .into()
            });

            weaktrace_formula(trace, expr, ModalityOperator::Diamond)
        }
    }
}

/// Generates a state formula AST for a minimal-depth strong-bisimulation
/// distinguishing formula.
///
/// The resulting formula holds in one of the two compared states but not the
/// other, witnessing that they are not strongly bisimilar.
pub fn generate_distinguishing_formula<L: TransitionLabel>(formula: &DistinguishingFormula<L>) -> StateFrm {
    distinguishing_to_statefrm(formula, false)
}

/// Converts a [`DistinguishingFormula`] to a [`StateFrm`], treating it as
/// negated when `negated` is set.
///
/// Negation is pushed inward through the modalities via the modal De Morgan
/// laws (`!<a>phi == [a]!phi`, dually for `[a]`, distributing over the
/// conjuncts), rather than emitted as a bare [`StateFrmKind::Unary`] negation:
/// consumers such as `merc_vpg`'s parity-game translation only accept
/// formulas without free negation.
fn distinguishing_to_statefrm<L: TransitionLabel>(formula: &DistinguishingFormula<L>, negated: bool) -> StateFrm {
    match formula {
        DistinguishingFormula::Negate(inner) => distinguishing_to_statefrm(inner, !negated),
        DistinguishingFormula::Diamond { label, conjuncts } => {
            let (operator, op, unit): (_, _, StateFrm) = if negated {
                (
                    ModalityOperator::Box,
                    StateFrmOp::Disjunction,
                    StateFrmKind::False.into(),
                )
            } else {
                (
                    ModalityOperator::Diamond,
                    StateFrmOp::Conjunction,
                    StateFrmKind::True.into(),
                )
            };

            let expr = conjuncts
                .iter()
                .map(|conjunct| distinguishing_to_statefrm(conjunct, negated))
                .reduce(|lhs, rhs| {
                    StateFrmKind::Binary {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    }
                    .into()
                })
                .unwrap_or(unit);

            StateFrmKind::Modality {
                operator,
                formula: RegFrmKind::Action(ActFrmKind::MultAct(label_to_multi_action(label)).into()).into(),
                expr: Box::new(expr),
            }
            .into()
        }
    }
}

/// Generates a formula `[tau* . label1 . tau* . label2. ... . tau*]expr`, or
/// diamond based on `modality`, that characterizes the weak trace.
///
/// Note that every hidden label in the given `trace` is ignored, to ensure that
/// it is a valid weaktrace formula.
fn weaktrace_formula<L: TransitionLabel>(trace: &[L], expr: StateFrm, modality: ModalityOperator) -> StateFrm {
    // Build the formula tau*
    let tau_star: RegFrm = RegFrmKind::Iteration(Box::new(
        RegFrmKind::Action(ActFrmKind::MultAct(MultiAction::tau()).into()).into(),
    ))
    .into();

    // We build the formula bottom up: tau* . label . ... . tau*
    let mut result: StateFrm = StateFrmKind::Modality {
        operator: modality,
        formula: tau_star.clone(),
        expr: Box::new(expr),
    }
    .into();

    for label in trace.iter().rev().filter(|l| !l.is_tau_label()) {
        result = StateFrmKind::Modality {
            operator: modality,
            formula: tau_star.clone(),
            expr: Box::new(
                StateFrmKind::Modality {
                    operator: modality,
                    formula: RegFrmKind::Action(ActFrmKind::MultAct(label_to_multi_action(label)).into()).into(),
                    expr: Box::new(result),
                }
                .into(),
            ),
        }
        .into()
    }

    result
}

/// Converts a label to a multi-action, where tau labels are converted to the empty multi-action.
fn label_to_multi_action<L: TransitionLabel>(label: &L) -> MultiAction {
    if label.is_tau_label() {
        MultiAction::tau()
    } else {
        MultiAction::new(vec![Action::new(label.to_string(), Vec::new())])
    }
}
