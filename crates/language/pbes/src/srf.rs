use std::collections::HashMap;
use std::collections::VecDeque;

use merc_data::DataVariable;
use merc_utilities::FreshNameGenerator;
use merc_utilities::MercError;
use merc_utilities::optimized_and;
use merc_utilities::optimized_conjunction;
use merc_utilities::optimized_disjunction;
use merc_utilities::optimized_not;
use merc_utilities::optimized_or;

use crate::FixpointSymbol;
use crate::Pbes;
use crate::PbesEquation;
use crate::PbesExpression;
use crate::PbesExpressionRef;
use crate::PbesFold;
use crate::PbesKind;
use crate::PropositionalVariable;
use crate::PropositionalVariableInstantiation;
use crate::SrfEquation;
use crate::SrfPbes;
use crate::SrfSummand;
use crate::distribute_not_over_and;
use crate::fold_pbes_expr;
use crate::is_simple_expression;
use crate::simple_expression::SimpleExpressionCache;
use crate::split_and;
use crate::split_or;

/// Converts a PBES into standard recursive form.
///
/// `pbes` must already be normalised (no `!`/`=>` left at the PBES level), and must be well
/// typed: a [crate::PbesWellTypedError] is returned otherwise.
pub fn pbes_to_srf(pbes: &Pbes, merge_simple_expressions: bool) -> Result<SrfPbes, MercError> {
    let srf = pbes_to_pre_srf_impl(pbes, false, merge_simple_expressions)?;
    Ok(simplify_srf_pbes(&srf))
}

/// Converts a PBES into *pre*-SRF: like [pbes_to_srf], but counter-example propositional variables
/// may remain as simple expressions in summand conditions instead of forcing a new equation.
///
/// `pbes` must already be normalised (no `!`/`=>` left at the PBES level), and must be well
/// typed: a [crate::PbesWellTypedError] is returned otherwise.
pub fn pbes_to_pre_srf(pbes: &Pbes, merge_simple_expressions: bool) -> Result<SrfPbes, MercError> {
    pbes_to_pre_srf_impl(pbes, true, merge_simple_expressions)
}

/// Simplifies an SRF PBES by removing summands whose condition simplifies to `true` or `false`: a
/// vacuous conjunct or dead disjunct is dropped, and an unconditionally true/false summand
/// collapses the whole equation to a single summand recursing into `X_true`/`X_false`, which
/// avoids spurious dependencies on the other, now irrelevant, summands.
pub fn simplify_srf_pbes(pbes: &SrfPbes) -> SrfPbes {
    let x_false = pbes.false_equation.summands[0].variable.clone();
    let x_true = pbes.true_equation.summands[0].variable.clone();

    // Shared by all guards, which typically have many conditions in common.
    let mut simplified = HashMap::new();
    let mut simplify_equation = |equation: &SrfEquation| {
        let conjunctive = equation.conjunctive;
        let mut summands = Vec::new();
        let mut equation_is_trivial = false;

        for summand in &equation.summands {
            // For a conjunctive equation, to_pbes_expression() builds
            // `distribute_not_over_and(condition) || variable`; mirror that here to tell a
            // vacuously true or unsatisfiable guard apart from a real one.
            let guard = if conjunctive {
                simplify_boolean(
                    &distribute_not_over_and(&summand.condition.copy()).copy(),
                    &mut simplified,
                )
            } else {
                summand.condition.clone()
            };
            let is_true = matches!(guard.kind(), PbesKind::True);
            let is_false = matches!(guard.kind(), PbesKind::False);
            let targets_false = summand.parameters.is_empty() && summand.variable.name() == x_false.name();
            let targets_true = summand.parameters.is_empty() && summand.variable.name() == x_true.name();

            if conjunctive {
                if is_true {
                    continue; // forall params . true || variable always holds.
                }
                if is_false && targets_false {
                    equation_is_trivial = true;
                    break; // forall <> . false || X_false == X_false.
                }
            } else {
                if is_false {
                    continue; // exists params . false && variable never holds.
                }
                if is_true && targets_true {
                    equation_is_trivial = true;
                    break; // exists <> . true && X_true == X_true.
                }
            }

            summands.push(summand.clone());
        }

        if equation_is_trivial {
            let target = if conjunctive { x_false.clone() } else { x_true.clone() };
            summands = vec![SrfSummand::new(Vec::new(), PbesExpression::true_constant(), target)];
        } else if summands.is_empty() {
            let target = if conjunctive { x_true.clone() } else { x_false.clone() };
            summands = vec![SrfSummand::new(Vec::new(), PbesExpression::true_constant(), target)];
        }

        SrfEquation::new(equation.symbol, equation.variable.clone(), summands, conjunctive)
    };

    SrfPbes::new(
        pbes.data_specification.clone(),
        pbes.global_variables.clone(),
        pbes.equations.iter().map(&mut simplify_equation).collect(),
        simplify_equation(&pbes.false_equation),
        simplify_equation(&pbes.true_equation),
        pbes.initial_state.clone(),
    )
}

/// Returns true iff `pbes` is already in standard recursive form, i.e. converting it does not
/// introduce any equation beyond the two generated `X_false`/`X_true` ones.
pub fn is_srf(pbes: &Pbes) -> Result<bool, MercError> {
    let srf = pbes_to_srf(pbes, true)?;
    Ok(srf.equations.len() == pbes.equations.len())
}

/// Determines whether a normalised PBES equation's right-hand side `phi` should become a
/// conjunctive (`forall`-based) or disjunctive (`exists`-based) SRF equation.
pub fn is_conjunctive(phi: &PbesExpressionRef<'_>, allow_ce: bool) -> Result<bool, MercError> {
    is_conjunctive_with(phi, &mut |expr| is_simple_expression(expr, allow_ce))
}

fn pbes_to_pre_srf_impl(pbes: &Pbes, allow_ce: bool, merge_simple_expressions: bool) -> Result<SrfPbes, MercError> {
    pbes.check_well_typed()?;

    // Fresh propositional variable names, distinct from every equation of the input.
    let mut id_generator = FreshNameGenerator::new(
        pbes.equations
            .iter()
            .map(|equation| equation.variable.name().value().to_string()),
    );
    let x_false = id_generator.generate("X_false");
    let x_true = id_generator.generate("X_true");

    // mu X_false = false || X_false; nu X_true = X_true: the escape equations every other
    // equation can route to.
    let false_variable = PropositionalVariable::new(&x_false, &[]);
    let true_variable = PropositionalVariable::new(&x_true, &[]);
    let false_formula = PbesExpression::or(
        &PbesExpression::false_constant(),
        &PropositionalVariableInstantiation::identity(&false_variable).into(),
    );
    let true_formula = PropositionalVariableInstantiation::identity(&true_variable).into();
    let eqn_false = PbesEquation::new(FixpointSymbol::Mu, false_variable, false_formula);
    let eqn_true = PbesEquation::new(FixpointSymbol::Nu, true_variable, true_formula);

    let mut builder = SrfBuilder {
        pending: pbes.equations.iter().cloned().collect(),
        id_generator,
        x_true,
        x_false,
        merge_simple_expressions,
        allow_ce,
        simple: SimpleExpressionCache::default(),
    };

    let mut equations = Vec::new();
    while let Some(eqn) = builder.pending.pop_front() {
        equations.push(builder.build_equation(&eqn)?);
    }

    // Both escape equations are already in SRF, so they spawn nothing.
    let false_equation = builder.build_equation(&eqn_false)?;
    let true_equation = builder.build_equation(&eqn_true)?;
    debug_assert!(builder.pending.is_empty());

    Ok(SrfPbes::new(
        pbes.data_specification.clone(),
        pbes.global_variables.clone(),
        equations,
        false_equation,
        true_equation,
        pbes.initial_state.clone(),
    ))
}

/// Builds the SRF equations and summands for one input PBES.
struct SrfBuilder {
    /// Equations still to be processed: equations spawned while processing one equation are
    /// inserted at the front, so they are handled immediately after it.
    pending: VecDeque<PbesEquation>,
    id_generator: FreshNameGenerator,
    x_true: String,
    x_false: String,
    merge_simple_expressions: bool,
    allow_ce: bool,
    simple: SimpleExpressionCache,
}

impl SrfBuilder {
    fn escape_target(&self, conjunctive: bool) -> PropositionalVariableInstantiation {
        let name = if conjunctive { &self.x_false } else { &self.x_true };
        PropositionalVariableInstantiation::new(name, &[])
    }

    /// Defers `phi` (the raw, unprocessed expression) to a freshly named equation with
    /// `variables` as its parameters, and returns the summand that invokes it.
    fn spawn_equation(
        &mut self,
        eqn: &PbesEquation,
        variables: &[DataVariable],
        phi: &PbesExpressionRef<'_>,
    ) -> SrfSummand {
        let name = self.id_generator.generate(eqn.variable.name().value());
        let variable = PropositionalVariable::new(&name, variables);
        let instantiation = PropositionalVariableInstantiation::identity(&variable);
        self.pending
            .push_front(PbesEquation::new(eqn.symbol, variable, phi.protect()));

        SrfSummand::new(Vec::new(), PbesExpression::true_constant(), instantiation)
    }

    /// Turns `eqn` into an SRF equation; equations it spawns are queued in `pending`.
    fn build_equation(&mut self, eqn: &PbesEquation) -> Result<SrfEquation, MercError> {
        let formula = eqn.formula.copy();
        let allow_ce = self.allow_ce;
        let conjunctive = is_conjunctive_with(&formula, &mut |expr| self.simple.is_simple(expr, allow_ce))?;
        let variables = eqn.variable.parameters().to_vec();
        let summands = self.collect_summands(conjunctive, &formula, eqn, &variables)?;
        Ok(SrfEquation::new(
            eqn.symbol,
            eqn.variable.clone(),
            summands,
            conjunctive,
        ))
    }

    /// Collects the summands for `phi`, the (sub)formula of equation `eqn` currently being turned
    /// into SRF; `conjunctive` is the shape of `eqn`, and `variables` are the parameters in scope
    /// (those of the equation plus every quantifier bound on the way down).
    fn collect_summands(
        &mut self,
        conjunctive: bool,
        phi: &PbesExpressionRef<'_>,
        eqn: &PbesEquation,
        variables: &[DataVariable],
    ) -> Result<Vec<SrfSummand>, MercError> {
        // Every node is tried against the generic "free of propositional variables" case before
        // any connective or quantifier specific handler, so a quantifier with a simple body is
        // caught here.
        if self.simple.is_simple(phi, self.allow_ce) {
            return Ok(vec![SrfSummand::new(
                Vec::new(),
                leaf_condition(conjunctive, phi),
                self.escape_target(conjunctive),
            )]);
        }

        match phi.kind() {
            PbesKind::Instantiation(inst) => Ok(vec![SrfSummand::new(
                Vec::new(),
                PbesExpression::true_constant(),
                inst.protect(),
            )]),
            PbesKind::And(..) if conjunctive => self.collect_own_connective(conjunctive, phi, eqn, variables),
            PbesKind::Or(..) if !conjunctive => self.collect_own_connective(conjunctive, phi, eqn, variables),
            PbesKind::And(lhs, rhs) | PbesKind::Or(lhs, rhs) => {
                self.collect_other_connective(conjunctive, phi, &lhs, &rhs, eqn, variables)
            }
            PbesKind::Forall(bound, body) if conjunctive => {
                self.collect_quantifier(conjunctive, &bound.to_vec(), &body, eqn, variables)
            }
            PbesKind::Exists(bound, body) if !conjunctive => {
                self.collect_quantifier(conjunctive, &bound.to_vec(), &body, eqn, variables)
            }
            PbesKind::Forall(..) | PbesKind::Exists(..) => Ok(vec![self.spawn_equation(eqn, variables, phi)]),
            _ => Err(format!("collect_summands: PBES is not normalised, unexpected expression {phi}").into()),
        }
    }

    /// The quantifier matches the equation's shape, so its variables become summand parameters.
    ///
    /// The summands of `body` end up under `bound`, next to conditions that come from outside the
    /// quantifier (see [strengthen]). This cannot capture any variable, because a well-typed PBES
    /// has no quantifier that shadows a parameter, a global variable, or an enclosing quantifier;
    /// see [Pbes::check_well_typed].
    fn collect_quantifier(
        &mut self,
        conjunctive: bool,
        bound: &[DataVariable],
        body: &PbesExpressionRef<'_>,
        eqn: &PbesEquation,
        variables: &[DataVariable],
    ) -> Result<Vec<SrfSummand>, MercError> {
        let mut extended = variables.to_vec();
        extended.extend(bound.iter().cloned());
        let mut summands = self.collect_summands(conjunctive, body, eqn, &extended)?;
        for summand in &mut summands {
            summand.add_variables(bound);
        }
        Ok(summands)
    }

    /// `phi`'s top-level connective matches `conjunctive` (an `&&` chain in a conjunctive
    /// equation, or an `||` chain in a disjunctive one): flattens the whole chain and recurses into
    /// every clause, optionally pulling out clauses free of propositional variables to strengthen
    /// the summands coming from the rest.
    fn collect_own_connective(
        &mut self,
        conjunctive: bool,
        phi: &PbesExpressionRef<'_>,
        eqn: &PbesEquation,
        variables: &[DataVariable],
    ) -> Result<Vec<SrfSummand>, MercError> {
        let clauses = split_own(conjunctive, phi);
        let (simple, complex): (Vec<_>, Vec<_>) = if self.merge_simple_expressions {
            // Always checked without counter-example variables, regardless of `allow_ce`: the
            // strengthening condition built from `simple` is spliced into other summands'
            // conditions as-is, so it must not depend on any propositional variable.
            clauses
                .into_iter()
                .partition(|clause| self.simple.is_simple(clause, false))
        } else {
            (Vec::new(), clauses)
        };

        let mut summands = Vec::new();
        for clause in &simple {
            summands.extend(self.collect_summands(conjunctive, clause, eqn, variables)?);
        }

        if simple.is_empty() {
            for clause in &complex {
                summands.extend(self.collect_summands(conjunctive, clause, eqn, variables)?);
            }
            return Ok(summands);
        }

        let combined = combine_simple(conjunctive, simple.iter().map(|clause| clause.protect()));
        let condition = strengthen(conjunctive, &combined.copy());
        for clause in &complex {
            let start = summands.len();
            summands.extend(self.collect_summands(conjunctive, clause, eqn, variables)?);
            for summand in &mut summands[start..] {
                summand.add_condition(&condition);
            }
        }
        Ok(summands)
    }

    /// `phi`'s top-level connective is the *other* one (an `||` found while building a conjunctive
    /// equation, or an `&&` while building a disjunctive one). Unlike
    /// [Self::collect_own_connective] this only looks at the two immediate operands: if exactly
    /// one of them is free of propositional variables, it strengthens the summands coming from the
    /// other side; otherwise the whole `phi` is deferred to a freshly spawned equation, which
    /// flattens it once reprocessed as its own top-level connective.
    fn collect_other_connective(
        &mut self,
        conjunctive: bool,
        phi: &PbesExpressionRef<'_>,
        lhs: &PbesExpressionRef<'_>,
        rhs: &PbesExpressionRef<'_>,
        eqn: &PbesEquation,
        variables: &[DataVariable],
    ) -> Result<Vec<SrfSummand>, MercError> {
        // A simple side next to an instantiation is always pulled out, even without merging: that
        // is the shape by which [is_conjunctive] classified the equation, so spawning `phi` instead
        // would produce an equation identical to this one, and so on forever.
        let is_instantiation = |expr: &PbesExpressionRef<'_>| matches!(expr.kind(), PbesKind::Instantiation(_));
        let one_sided = if self.simple.is_simple(lhs, self.allow_ce)
            && (self.merge_simple_expressions || is_instantiation(rhs))
        {
            Some((lhs, rhs))
        } else if self.simple.is_simple(rhs, self.allow_ce) && (self.merge_simple_expressions || is_instantiation(lhs))
        {
            Some((rhs, lhs))
        } else {
            None
        };

        if let Some((simple_side, complex_side)) = one_sided {
            let mut summands = self.collect_summands(conjunctive, complex_side, eqn, variables)?;
            // The other connective is the own connective of the opposite shape, so the condition
            // has the polarity that shape would use.
            let condition = strengthen(!conjunctive, simple_side);
            for summand in &mut summands {
                summand.add_condition(&condition);
            }
            return Ok(summands);
        }

        Ok(vec![self.spawn_equation(eqn, variables, phi)])
    }
}

/// See [is_conjunctive]; `is_simple` decides whether an expression is simple, so that a caller can
/// answer from a cache.
fn is_conjunctive_with<F>(phi: &PbesExpressionRef<'_>, is_simple: &mut F) -> Result<bool, MercError>
where
    F: FnMut(&PbesExpressionRef<'_>) -> bool,
{
    if is_simple(phi) {
        return Ok(false);
    }

    match phi.kind() {
        PbesKind::Instantiation(_) => Ok(false),
        PbesKind::Or(lhs, rhs) => Ok(one_side_is_simple_instantiation(&lhs, &rhs, is_simple)),
        PbesKind::And(lhs, rhs) => Ok(!one_side_is_simple_instantiation(&lhs, &rhs, is_simple)),
        PbesKind::Exists(..) => Ok(false),
        PbesKind::Forall(..) => Ok(true),
        _ => Err(format!("is_conjunctive: unexpected case {phi}").into()),
    }
}

fn one_side_is_simple_instantiation<F>(
    lhs: &PbesExpressionRef<'_>,
    rhs: &PbesExpressionRef<'_>,
    is_simple: &mut F,
) -> bool
where
    F: FnMut(&PbesExpressionRef<'_>) -> bool,
{
    let is_instantiation = |expr: &PbesExpressionRef<'_>| matches!(expr.kind(), PbesKind::Instantiation(_));
    (is_simple(lhs) && is_instantiation(rhs)) || (is_simple(rhs) && is_instantiation(lhs))
}

fn split_own<'a>(conjunctive: bool, expr: &PbesExpressionRef<'a>) -> Vec<PbesExpressionRef<'a>> {
    if conjunctive { split_and(expr) } else { split_or(expr) }
}

/// The condition of the summand produced for a leaf (non-recursive) expression: negated for a
/// conjunctive equation, since its summands are joined by `||` rather than `&&`.
fn leaf_condition(conjunctive: bool, expr: &PbesExpressionRef<'_>) -> PbesExpression {
    if conjunctive {
        optimized_not(expr.protect())
    } else {
        expr.protect()
    }
}

/// The extra conjunct added to a summand's condition to account for a sibling simple clause
/// pulled out of the same `&&`/`||` chain: negated for a disjunctive equation, dually to
/// [leaf_condition].
fn strengthen(conjunctive: bool, condition: &PbesExpressionRef<'_>) -> PbesExpression {
    if conjunctive {
        condition.protect()
    } else {
        optimized_not(condition.protect())
    }
}

/// Merges several simple clauses of a chain into the one condition passed to [strengthen].
fn combine_simple<I>(conjunctive: bool, clauses: I) -> PbesExpression
where
    I: IntoIterator<Item = PbesExpression>,
{
    if conjunctive {
        optimized_conjunction(clauses)
    } else {
        optimized_disjunction(clauses)
    }
}

/// A minimal bottom-up `true`/`false` absorption over `!`, `&&` and `||` (see [optimized_and]),
/// scoped to what [simplify_srf_pbes] needs to decide whether a guard is a literal `true`/`false`;
/// not a general PBES rewriter. The other connectives and quantifiers are rebuilt around their
/// simplified operands, which never makes them a literal.
fn simplify_boolean(
    expr: &PbesExpressionRef<'_>,
    cache: &mut HashMap<PbesExpression, PbesExpression>,
) -> PbesExpression {
    fold_pbes_expr(expr, cache, |expr, node| match node {
        PbesFold::Not(inner) => optimized_not(inner),
        PbesFold::And(lhs, rhs) => optimized_and(lhs, rhs),
        PbesFold::Or(lhs, rhs) => optimized_or(lhs, rhs),
        PbesFold::Imp(lhs, rhs) => PbesExpression::imp(&lhs, &rhs),
        PbesFold::Forall(variables, body) => PbesExpression::forall(&variables.to_vec(), &body),
        PbesFold::Exists(variables, body) => PbesExpression::exists(&variables.to_vec(), &body),
        PbesFold::True | PbesFold::False | PbesFold::Data(_) | PbesFold::Instantiation(_) => expr.protect(),
    })
}

#[cfg(test)]
mod tests {
    //! Mostly ported from `libraries/pbes/test/srf_pbes_test.cpp` of mCRL2. The PBESs are built
    //! directly rather than parsed, since the conversion only looks at their structure: data
    //! expressions are applications of function symbols that merely carry the right sorts.

    use merc_data::DataApplication;
    use merc_data::DataExpression;
    use merc_data::DataFunctionSymbol;
    use merc_data::DataVariable;
    use merc_data::Mcrl2DataSpecification;
    use merc_data::SortExpression;
    use merc_utilities::Boolean;
    use merc_utilities::optimized_conjunction;

    use crate::FixpointSymbol;
    use crate::Pbes;
    use crate::PbesEquation;
    use crate::PbesExpression;
    use crate::PbesWellTypedError;
    use crate::PropositionalVariable;
    use crate::PropositionalVariableInstantiation;
    use crate::SrfEquation;
    use crate::SrfPbes;
    use crate::SrfSummand;
    use crate::pbes_to_pre_srf;
    use crate::pbes_to_srf;
    use crate::simplify_srf_pbes;
    use crate::split_and;

    /// The PBES `sigma X(b: Bool) = formula; init X(true);` of most tests.
    fn single(symbol: FixpointSymbol, formula: PbesExpression) -> Pbes {
        let b = DataVariable::with_sort("b", SortExpression::bool().copy());
        Pbes::new(
            Mcrl2DataSpecification::default(),
            Vec::new(),
            vec![PbesEquation::new(
                symbol,
                PropositionalVariable::new("X", &[b]),
                formula,
            )],
            PropositionalVariableInstantiation::new("X", &[DataExpression::bool_literal(true)]),
        )
    }

    /// The parameter `b` of [single].
    fn b() -> DataExpression {
        DataVariable::with_sort("b", SortExpression::bool().copy()).into()
    }

    /// `X(arguments)`.
    fn x(arguments: &[DataExpression]) -> PbesExpression {
        PropositionalVariableInstantiation::new("X", arguments).into()
    }

    /// The application of the binary `Nat` operator `name`, of the given result sort.
    fn nat_operator(
        name: &str,
        codomain: &SortExpression,
        lhs: &DataExpression,
        rhs: &DataExpression,
    ) -> DataExpression {
        let nat = SortExpression::nat();
        let symbol = DataFunctionSymbol::with_signature(name, &[nat.clone(), nat], codomain);
        DataApplication::with_args(&symbol, &[lhs.copy(), rhs.copy()]).into()
    }

    fn equation<'a>(srf: &'a SrfPbes, name: &str) -> &'a SrfEquation {
        srf.equations()
            .find(|equation| equation.variable().name().value() == name)
            .unwrap_or_else(|| panic!("equation {name} not found"))
    }

    // A bare PVI in a disjunctive equation produces summand ([], true, PVI).
    #[test]
    fn test_srf_or_pvi() {
        let srf = pbes_to_srf(&single(FixpointSymbol::Nu, x(&[b()])), true).unwrap();
        assert_eq!(srf.equations().count(), 3);

        let main = &srf.equations[0];
        assert!(!main.is_conjunctive());
        assert_eq!(main.summands().len(), 1);
        let summand = &main.summands()[0];
        assert!(summand.parameters().is_empty());
        assert!(summand.condition().is_true());
        assert_eq!(summand.variable().name().value(), "X");
    }

    // A disjunction of two PVIs produces two summands, each with condition true.
    #[test]
    fn test_srf_or_two_pvis() {
        let formula = PbesExpression::or(
            &x(&[DataExpression::bool_literal(true)]),
            &x(&[DataExpression::bool_literal(false)]),
        );
        let srf = pbes_to_srf(&single(FixpointSymbol::Nu, formula), true).unwrap();
        assert_eq!(srf.equations().count(), 3);

        let main = &srf.equations[0];
        assert!(!main.is_conjunctive());
        assert_eq!(main.summands().len(), 2);
        for summand in main.summands() {
            assert!(summand.parameters().is_empty());
            assert!(summand.condition().is_true());
            assert_eq!(summand.variable().name().value(), "X");
        }
    }

    // An exists with a PVI body in a disjunctive equation recurses into the body and prepends the
    // bound variable as the summand parameter.
    #[test]
    fn test_srf_or_exists_pvi_body() {
        let n = DataVariable::with_sort("n", SortExpression::nat().copy());
        let formula = PbesExpression::exists(
            std::slice::from_ref(&n),
            &x(&[DataExpression::equal(
                &SortExpression::nat(),
                &n.clone().into(),
                &DataFunctionSymbol::with_sort("0", SortExpression::nat().copy()).into(),
            )]),
        );
        let srf = pbes_to_srf(&single(FixpointSymbol::Nu, formula), true).unwrap();
        assert_eq!(srf.equations().count(), 3);

        let main = &srf.equations[0];
        assert!(!main.is_conjunctive());
        assert_eq!(main.summands().len(), 1);
        let summand = &main.summands()[0];
        assert_eq!(summand.parameters(), [n]);
        assert!(summand.condition().is_true());
        assert_eq!(summand.variable().name().value(), "X");
    }

    // A forall with a PVI body in a conjunctive equation recurses into the body and prepends the
    // bound variable as the summand parameter.
    #[test]
    fn test_srf_and_forall_pvi_body() {
        let n = DataVariable::with_sort("n", SortExpression::nat().copy());
        let formula = PbesExpression::forall(
            std::slice::from_ref(&n),
            &x(&[DataExpression::equal(
                &SortExpression::nat(),
                &n.clone().into(),
                &DataFunctionSymbol::with_sort("0", SortExpression::nat().copy()).into(),
            )]),
        );
        let srf = pbes_to_srf(&single(FixpointSymbol::Mu, formula), true).unwrap();
        assert_eq!(srf.equations().count(), 3);

        let main = &srf.equations[0];
        assert!(main.is_conjunctive());
        assert_eq!(main.summands().len(), 1);
        let summand = &main.summands()[0];
        assert_eq!(summand.parameters(), [n]);
        assert!(summand.condition().is_true());
        assert_eq!(summand.variable().name().value(), "X");
    }

    // A conjunction of two PVIs is conjunctive; both PVIs become separate summands ([], true, PVI).
    #[test]
    fn test_srf_and_two_pvis() {
        let formula = PbesExpression::and(
            &x(&[DataExpression::bool_literal(true)]),
            &x(&[DataExpression::bool_literal(false)]),
        );
        let srf = pbes_to_srf(&single(FixpointSymbol::Mu, formula), true).unwrap();
        assert_eq!(srf.equations().count(), 3);

        let main = &srf.equations[0];
        assert!(main.is_conjunctive());
        assert_eq!(main.summands().len(), 2);
        for summand in main.summands() {
            assert!(summand.parameters().is_empty());
            assert!(summand.condition().is_true());
            assert_eq!(summand.variable().name().value(), "X");
        }
    }

    /// Checks that `main` has two summands, one for `X(b)` and one, with condition true and no
    /// parameters, for the single equation that was spawned.
    fn assert_spawned_one(srf: &SrfPbes, conjunctive: bool) {
        // The original equation, the spawned one, X_false and X_true.
        assert_eq!(srf.equations().count(), 4);

        let main = &srf.equations[0];
        assert_eq!(main.is_conjunctive(), conjunctive);
        assert_eq!(main.summands().len(), 2);
        let fresh: Vec<_> = main
            .summands()
            .iter()
            .filter(|summand| {
                summand.variable().name().value() != "X"
                    && summand.variable().name() != srf.false_equation().variable().name()
                    && summand.variable().name() != srf.true_equation().variable().name()
            })
            .collect();
        assert_eq!(fresh.len(), 1);
        assert!(fresh[0].parameters().is_empty());
        assert!(fresh[0].condition().is_true());
    }

    // A forall with a non-simple body in a disjunctive equation cannot be inlined; a fresh
    // equation is introduced and the main equation gets a summand pointing to it.
    #[test]
    fn test_srf_or_forall_non_simple_body() {
        let n = DataVariable::with_sort("n", SortExpression::nat().copy());
        let formula = PbesExpression::or(
            &x(&[b()]),
            &PbesExpression::forall(
                std::slice::from_ref(&n),
                &x(&[DataExpression::equal(
                    &SortExpression::nat(),
                    &n.clone().into(),
                    &DataFunctionSymbol::with_sort("0", SortExpression::nat().copy()).into(),
                )]),
            ),
        );
        assert_spawned_one(&pbes_to_srf(&single(FixpointSymbol::Nu, formula), true).unwrap(), false);
    }

    // Dually, an exists with a non-simple body in a conjunctive equation.
    #[test]
    fn test_srf_and_exists_non_simple_body() {
        let n = DataVariable::with_sort("n", SortExpression::nat().copy());
        let formula = PbesExpression::and(
            &x(&[b()]),
            &PbesExpression::exists(
                std::slice::from_ref(&n),
                &x(&[DataExpression::equal(
                    &SortExpression::nat(),
                    &n.clone().into(),
                    &DataFunctionSymbol::with_sort("0", SortExpression::nat().copy()).into(),
                )]),
            ),
        );
        assert_spawned_one(&pbes_to_srf(&single(FixpointSymbol::Mu, formula), true).unwrap(), true);
    }

    // `val(b) && X(b)` is disjunctive (simple && PVI does not make an `&&` conjunctive); the simple
    // side becomes the condition of the summand rather than a fresh equation.
    #[test]
    fn test_srf_or_and_simple_left() {
        let formula = PbesExpression::and(&b().into(), &x(&[b()]));
        let srf = pbes_to_srf(&single(FixpointSymbol::Nu, formula), true).unwrap();
        assert_eq!(srf.equations().count(), 3);

        let main = &srf.equations[0];
        assert!(!main.is_conjunctive());
        assert_eq!(main.summands().len(), 1);
        let summand = &main.summands()[0];
        assert!(summand.parameters().is_empty());
        assert_eq!(summand.condition(), &PbesExpression::from(b()));
        assert_eq!(summand.variable().name().value(), "X");
    }

    // `val(b) || X(b)` is conjunctive (simple || PVI); the negated simple side becomes the
    // condition of the summand.
    #[test]
    fn test_srf_and_or_simple_left() {
        let formula = PbesExpression::or(&b().into(), &x(&[b()]));
        let srf = pbes_to_srf(&single(FixpointSymbol::Mu, formula), true).unwrap();
        assert_eq!(srf.equations().count(), 3);

        let main = &srf.equations[0];
        assert!(main.is_conjunctive());
        assert_eq!(main.summands().len(), 1);
        let summand = &main.summands()[0];
        assert!(summand.parameters().is_empty());
        assert_eq!(summand.condition(), &PbesExpression::not(&b().into()));
        assert_eq!(summand.variable().name().value(), "X");
    }

    // `X(b) || (X(true) && X(false))`: the `&&` in a disjunctive equation has no simple side, so a
    // fresh equation is introduced for it.
    #[test]
    fn test_srf_or_and_both_non_simple() {
        let formula = PbesExpression::or(
            &x(&[b()]),
            &PbesExpression::and(
                &x(&[DataExpression::bool_literal(true)]),
                &x(&[DataExpression::bool_literal(false)]),
            ),
        );
        assert_spawned_one(&pbes_to_srf(&single(FixpointSymbol::Nu, formula), true).unwrap(), false);
    }

    // Dually, `X(b) && (X(true) || X(false))`.
    #[test]
    fn test_srf_and_or_both_non_simple() {
        let formula = PbesExpression::and(
            &x(&[b()]),
            &PbesExpression::or(
                &x(&[DataExpression::bool_literal(true)]),
                &x(&[DataExpression::bool_literal(false)]),
            ),
        );
        assert_spawned_one(&pbes_to_srf(&single(FixpointSymbol::Mu, formula), true).unwrap(), true);
    }

    // In pre-SRF, a simple clause of an `||` chain in a disjunctive equation gets its own summand
    // (to X_true), and the other clauses get its negation added to their condition.
    #[test]
    fn test_pre_srf_or_simple_clause() {
        let formula = PbesExpression::or(
            &PbesExpression::or(&b().into(), &x(&[DataExpression::bool_literal(true)])),
            &x(&[DataExpression::bool_literal(false)]),
        );
        let srf = pbes_to_pre_srf(&single(FixpointSymbol::Nu, formula), true).unwrap();
        assert_eq!(srf.equations().count(), 3);

        let main = &srf.equations[0];
        assert!(!main.is_conjunctive());
        assert_eq!(main.summands().len(), 3);
        let x_true = srf.true_equation().variable().name();
        for summand in main.summands() {
            if summand.variable().name() == x_true {
                assert_eq!(summand.condition(), &PbesExpression::from(b()));
            } else {
                assert_eq!(summand.condition(), &PbesExpression::not(&b().into()));
            }
        }
        let to_true = main.summands().iter().filter(|s| s.variable().name() == x_true);
        assert_eq!(to_true.count(), 1);
    }

    // Dually, a simple clause of an `&&` chain in a conjunctive equation gets its own summand (to
    // X_false, negated), and the other clauses get it added to their condition.
    #[test]
    fn test_pre_srf_and_simple_clause() {
        let formula = PbesExpression::and(
            &PbesExpression::and(&b().into(), &x(&[DataExpression::bool_literal(true)])),
            &x(&[DataExpression::bool_literal(false)]),
        );
        let srf = pbes_to_pre_srf(&single(FixpointSymbol::Mu, formula), true).unwrap();
        assert_eq!(srf.equations().count(), 3);

        let main = &srf.equations[0];
        assert!(main.is_conjunctive());
        assert_eq!(main.summands().len(), 3);
        let x_false = srf.false_equation().variable().name();
        for summand in main.summands() {
            if summand.variable().name() == x_false {
                assert_eq!(summand.condition(), &PbesExpression::not(&b().into()));
            } else {
                assert_eq!(summand.condition(), &PbesExpression::from(b()));
            }
        }
        let to_false = main.summands().iter().filter(|s| s.variable().name() == x_false);
        assert_eq!(to_false.count(), 1);
    }

    // A simple exists inside a conjunctive equation is a leaf: it becomes the negated condition
    // of a summand to X_false, under the enclosing forall variable.
    // See also https://github.com/mCRL2org/mCRL2/issues/1913.
    #[test]
    fn test_srf_and_exists_simple_body() {
        let n = DataVariable::with_sort("n", SortExpression::nat().copy());
        let k = DataVariable::with_sort("k", SortExpression::nat().copy());
        let less = nat_operator("<", &SortExpression::bool(), &k.clone().into(), &n.clone().into());
        let simple = PbesExpression::exists(std::slice::from_ref(&k), &less.into());
        let formula = PbesExpression::forall(std::slice::from_ref(&n), &PbesExpression::and(&simple, &x(&[b()])));
        let srf = pbes_to_srf(&single(FixpointSymbol::Mu, formula), true).unwrap();
        assert_eq!(srf.equations().count(), 3);

        let main = &srf.equations[0];
        assert!(main.is_conjunctive());
        assert_eq!(main.summands().len(), 2);
        let x_false = srf.false_equation().variable().name();
        let to_false: Vec<_> = main
            .summands()
            .iter()
            .filter(|s| s.variable().name() == x_false)
            .collect();
        assert_eq!(to_false.len(), 1);
        assert_eq!(to_false[0].parameters(), [n]);
        assert_eq!(to_false[0].condition(), &PbesExpression::not(&simple));
    }

    // A simple forall inside a disjunctive equation is a leaf: it becomes the condition of a
    // summand to X_true, without parameters.
    // See also https://github.com/mCRL2org/mCRL2/issues/1913.
    #[test]
    fn test_srf_or_forall_simple_body() {
        let n = DataVariable::with_sort("n", SortExpression::nat().copy());
        let less = nat_operator(
            "<",
            &SortExpression::bool(),
            &n.clone().into(),
            &DataFunctionSymbol::with_sort("5", SortExpression::nat().copy()).into(),
        );
        let simple = PbesExpression::forall(std::slice::from_ref(&n), &less.into());
        let formula = PbesExpression::or(&PbesExpression::or(&x(&[b()]), &simple), &x(&[b()]));
        let srf = pbes_to_srf(&single(FixpointSymbol::Nu, formula), true).unwrap();
        assert_eq!(srf.equations().count(), 3);

        let main = &srf.equations[0];
        assert!(!main.is_conjunctive());
        // mCRL2 has 3 summands here: for plain SRF it neither flattens the `||` chain nor merges its
        // simple clauses, so `X(b)` occurs twice. Here the chain is split into distinct clauses.
        assert_eq!(main.summands().len(), 2);
        let x_true = srf.true_equation().variable().name();
        let leaf: Vec<_> = main
            .summands()
            .iter()
            .filter(|s| s.variable().name() == x_true)
            .collect();
        assert_eq!(leaf.len(), 1);
        assert!(leaf[0].parameters().is_empty());
        assert_eq!(leaf[0].condition(), &simple);
    }

    /// The first part of the example from A. Stramaglia, J.J.A. Keiren, M. Laveaux and
    /// T.A.C. Willemse, "Efficient Evidence Generation for Modal mu-Calculus Model Checking",
    /// TACAS 2025: in pre-SRF the counter-example variables stay in the conditions. The second
    /// part, substituting them by `true`/`false`, has no counterpart here yet.
    #[test]
    fn test_example_stramaglia_paper() {
        let s = DataVariable::with_sort("s", SortExpression::nat().copy());
        let s1 = DataVariable::with_sort("s1", SortExpression::nat().copy());
        let n = DataVariable::with_sort("n", SortExpression::nat().copy());
        let s_: DataExpression = s.clone().into();
        let n_: DataExpression = n.clone().into();
        let sn = nat_operator("+", &SortExpression::nat(), &s_, &n_);
        let less = |lhs: &DataExpression, rhs: &DataExpression| nat_operator("<", &SortExpression::bool(), &lhs, &rhs);
        let instantiation = |name: &str, arguments: &[DataExpression]| -> PbesExpression {
            PropositionalVariableInstantiation::new(name, arguments).into()
        };

        // mu X(s: Nat) = (exists n: Nat. val(s == 1) && val(0 < n && n < 3)
        //                  && (X(s+n) || Zneg_0_(s, s+n)) && Zpos_0_(s, s+n)) || Y(s);
        let body = optimized_conjunction([
            DataExpression::equal(
                &SortExpression::nat(),
                &s_,
                &DataFunctionSymbol::with_sort("1", SortExpression::nat().copy()).into(),
            )
            .into(),
            DataExpression::and(
                &less(
                    &DataFunctionSymbol::with_sort("0", SortExpression::nat().copy()).into(),
                    &n_,
                ),
                &less(
                    &n_,
                    &DataFunctionSymbol::with_sort("3", SortExpression::nat().copy()).into(),
                ),
            )
            .into(),
            PbesExpression::or(
                &instantiation("X", std::slice::from_ref(&sn)),
                &instantiation("Zneg_0_", &[s_.clone(), sn.clone()]),
            ),
            instantiation("Zpos_0_", &[s_.clone(), sn.clone()]),
        ]);
        let x = PbesExpression::or(
            &PbesExpression::exists(std::slice::from_ref(&n), &body),
            &instantiation("Y", std::slice::from_ref(&s_)),
        );

        // nu Y(s: Nat) = val(s == 3) && (Y(s) || Zneg_1_(s, s)) && Zpos_1_(s, s);
        let y = optimized_conjunction([
            DataExpression::equal(
                &SortExpression::nat(),
                &s_,
                &DataFunctionSymbol::with_sort("3", SortExpression::nat().copy()).into(),
            )
            .into(),
            PbesExpression::or(
                &instantiation("Y", std::slice::from_ref(&s_)),
                &instantiation("Zneg_1_", &[s_.clone(), s_.clone()]),
            ),
            instantiation("Zpos_1_", &[s_.clone(), s_.clone()]),
        ]);

        let parameters = [s.clone(), s1];
        let equation_of = |symbol, name: &str, parameters: &[DataVariable], formula| {
            PbesEquation::new(symbol, PropositionalVariable::new(name, parameters), formula)
        };
        let pbes = Pbes::new(
            Mcrl2DataSpecification::default(),
            Vec::new(),
            vec![
                equation_of(FixpointSymbol::Mu, "X", std::slice::from_ref(&s), x),
                equation_of(FixpointSymbol::Nu, "Y", std::slice::from_ref(&s), y),
                equation_of(
                    FixpointSymbol::Nu,
                    "Zpos_0_",
                    &parameters,
                    PbesExpression::true_constant(),
                ),
                equation_of(
                    FixpointSymbol::Nu,
                    "Zpos_1_",
                    &parameters,
                    PbesExpression::true_constant(),
                ),
                equation_of(
                    FixpointSymbol::Mu,
                    "Zneg_0_",
                    &parameters,
                    PbesExpression::false_constant(),
                ),
                equation_of(
                    FixpointSymbol::Mu,
                    "Zneg_1_",
                    &parameters,
                    PbesExpression::false_constant(),
                ),
            ],
            PropositionalVariableInstantiation::new(
                "X",
                &[DataFunctionSymbol::with_sort("0", SortExpression::nat().copy()).into()],
            ),
        );

        let pre_srf = pbes_to_pre_srf(&pbes, true).unwrap();
        assert_eq!(equation(&pre_srf, "X").summands().len(), 3);
    }

    /// The targets of the summands of `X` after [simplify_srf_pbes], where `X` is the equation with
    /// the given summands, each to an equation without parameters given by name. The PBES also has
    /// the escape equations, each with its single self-referencing summand.
    fn simplified_targets(
        conjunctive: bool,
        symbol: FixpointSymbol,
        summands: &[(PbesExpression, &str)],
    ) -> Vec<String> {
        let summand = |condition: &PbesExpression, target: &str| {
            SrfSummand::new(
                Vec::new(),
                condition.clone(),
                PropositionalVariableInstantiation::new(target, &[]),
            )
        };
        let escape = |symbol, name: &str| {
            SrfEquation::new(
                symbol,
                PropositionalVariable::new(name, &[]),
                vec![summand(&PbesExpression::true_constant(), name)],
                false,
            )
        };
        let x = SrfEquation::new(
            symbol,
            PropositionalVariable::new("X", &[]),
            summands
                .iter()
                .map(|(condition, target)| summand(condition, target))
                .collect(),
            conjunctive,
        );
        let srf = SrfPbes::new(
            Mcrl2DataSpecification::default(),
            Vec::new(),
            vec![x],
            escape(FixpointSymbol::Mu, "X_false"),
            escape(FixpointSymbol::Nu, "X_true"),
            PropositionalVariableInstantiation::new("X_true", &[]),
        );

        let result = simplify_srf_pbes(&srf);
        equation(&result, "X")
            .summands()
            .iter()
            .map(|summand| summand.variable().name().value().to_string())
            .collect()
    }

    #[test]
    fn test_simplify_srf_pbes_drops_dead_disjunct() {
        // exists <> . false && Y() never holds and is dropped; exists <> . true && Z() survives.
        let summands = [
            (PbesExpression::false_constant(), "Y"),
            (PbesExpression::true_constant(), "Z"),
        ];
        assert_eq!(simplified_targets(false, FixpointSymbol::Mu, &summands), ["Z"]);
    }

    #[test]
    fn test_simplify_srf_pbes_drops_vacuous_conjunct() {
        // The guard of a conjunctive summand is negated, so a stored condition `false` makes
        // `true || Y()` vacuously true and the conjunct is dropped; `false || Z()` survives.
        let summands = [
            (PbesExpression::false_constant(), "Y"),
            (PbesExpression::true_constant(), "Z"),
        ];
        assert_eq!(simplified_targets(true, FixpointSymbol::Mu, &summands), ["Z"]);
    }

    #[test]
    fn test_simplify_srf_pbes_disjunctive_equation_collapses_to_true() {
        // An unconditional summand to X_true makes the whole disjunctive equation true.
        let summands = [
            (PbesExpression::true_constant(), "Y"),
            (PbesExpression::true_constant(), "X_true"),
        ];
        assert_eq!(simplified_targets(false, FixpointSymbol::Mu, &summands), ["X_true"]);
    }

    #[test]
    fn test_simplify_srf_pbes_conjunctive_equation_collapses_to_false() {
        // Dually, a summand whose negated guard is `false`, to X_false, makes the whole
        // conjunctive equation false.
        let summands = [
            (PbesExpression::true_constant(), "Y"),
            (PbesExpression::true_constant(), "X_false"),
        ];
        assert_eq!(simplified_targets(true, FixpointSymbol::Nu, &summands), ["X_false"]);
    }

    #[test]
    fn test_simplify_srf_pbes_all_disjuncts_dead_becomes_false() {
        // An empty disjunction is false, represented by a single summand to X_false.
        let summands = [
            (PbesExpression::false_constant(), "Y"),
            (PbesExpression::false_constant(), "Z"),
        ];
        assert_eq!(simplified_targets(false, FixpointSymbol::Mu, &summands), ["X_false"]);
    }

    #[test]
    fn test_simplify_srf_pbes_all_conjuncts_vacuous_becomes_true() {
        // Dually, an empty conjunction is true, represented by a single summand to X_true.
        let summands = [
            (PbesExpression::false_constant(), "Y"),
            (PbesExpression::false_constant(), "Z"),
        ];
        assert_eq!(simplified_targets(true, FixpointSymbol::Nu, &summands), ["X_true"]);
    }

    #[test]
    fn test_simplify_srf_pbes_leaves_ordinary_equations_unchanged() {
        let b1: PbesExpression =
            DataExpression::from(DataVariable::with_sort("b1", SortExpression::bool().copy())).into();
        let b2: PbesExpression =
            DataExpression::from(DataVariable::with_sort("b2", SortExpression::bool().copy())).into();
        let summands = [(b1, "Y"), (b2, "Z")];
        assert_eq!(simplified_targets(true, FixpointSymbol::Nu, &summands), ["Y", "Z"]);
    }

    /// `X(b: Bool) = val(b) || exists b: Bool . Y(b)`: moving the guard `!val(b)` under the
    /// existential would capture it, so the PBES is rejected as ill-typed instead.
    #[test]
    fn test_quantifier_shadowing_parameter_is_rejected() {
        let b_variable = DataVariable::with_sort("b", SortExpression::bool().copy());
        let parameters = std::slice::from_ref(&b_variable);
        let y_b: PbesExpression = PropositionalVariableInstantiation::new("Y", &[b()]).into();
        let pbes = Pbes::new(
            Mcrl2DataSpecification::default(),
            Vec::new(),
            vec![
                PbesEquation::new(
                    FixpointSymbol::Nu,
                    PropositionalVariable::new("X", parameters),
                    PbesExpression::or(&b().into(), &PbesExpression::exists(parameters, &y_b)),
                ),
                PbesEquation::new(
                    FixpointSymbol::Nu,
                    PropositionalVariable::new("Y", parameters),
                    b().into(),
                ),
            ],
            PropositionalVariableInstantiation::new("X", &[DataExpression::bool_literal(false)]),
        );

        for merge in [false, true] {
            let error = pbes_to_srf(&pbes, merge).err().expect("expected an ill-typed PBES");
            assert!(
                matches!(
                    error.downcast_ref::<PbesWellTypedError>(),
                    Some(PbesWellTypedError::QuantifierShadowsParameter { .. })
                ),
                "got {error:?}"
            );
        }
    }

    /// `X = true && Y` is classified as disjunctive by its one-sided shape; without merging this
    /// used to spawn an identical equation forever.
    #[test]
    fn test_one_sided_without_merging_terminates() {
        let y: PbesExpression = PropositionalVariableInstantiation::new("Y", &[]).into();
        let pbes = Pbes::new(
            Mcrl2DataSpecification::default(),
            Vec::new(),
            vec![
                PbesEquation::new(
                    FixpointSymbol::Mu,
                    PropositionalVariable::new("X", &[]),
                    PbesExpression::and(&PbesExpression::true_constant(), &y),
                ),
                PbesEquation::new(
                    FixpointSymbol::Mu,
                    PropositionalVariable::new("Y", &[]),
                    PbesExpression::false_constant(),
                ),
            ],
            PropositionalVariableInstantiation::new("X", &[]),
        );

        let srf = pbes_to_srf(&pbes, false).unwrap();
        assert_eq!(srf.equations.len(), 2);
    }

    /// `e(n+1) = e(n) && e(n)` is linear in size as a term graph, so splitting it must be too.
    #[test]
    fn test_split_shared_conjunction() {
        let x: PbesExpression = PropositionalVariableInstantiation::new("X", &[]).into();
        let mut expr = x.clone();
        for _ in 0..128 {
            expr = PbesExpression::and(&expr, &expr);
        }

        let operands: Vec<_> = split_and(&expr.copy()).into_iter().map(|e| e.protect()).collect();
        assert_eq!(operands, [x]);
    }
}
