use std::fmt;

use merc_data::DataExpressionRef;

use super::Annotation;
use super::Head;
use super::MachineView;
use super::TermStackId;
use super::Walk;
use crate::set_automaton::MatchResult;
use crate::set_automaton::SetAutomaton;
use crate::utilities::DataPosition;

/// Decides when the rewriting machine rewrites the arguments of a function symbol, and what the
/// variables of a right-hand side stand for.
///
/// The machine rewrites a term by rewriting its arguments, matching the redex and evaluating the
/// right-hand side of the rule that applies, and everything that it does differently is asked of
/// the scheduler.
///
/// # Example
///
/// The order of rewriting decides whether these rules terminate:
///
/// ```text
/// f(n)            = if(n == 0, 0, f(n - 1))
/// if(true, x, y)  = x
/// if(false, x, y) = y
/// ```
///
/// The [Innermost] scheduler changes nothing about what the machine does. The right-hand side of
/// `f` is evaluated from the inside out, as `n == 0`, `0`, `f(n - 1)` and `if`. Rewriting `f(0)`
/// therefore applies the rule of `f` again for `f(0 - 1)` before it reaches `if`, and so on
/// forever.
///
/// The jitty scheduler compiles the occurrence of `if` differently, because the rules of `if`
/// only need its first argument. The right-hand side computes `n == 0` and has [Scheduler::Hook]s
/// in place of both branches and of `if`. Rewriting `f(0)` then goes as follows:
///
/// 1. [Scheduler::enter] stores the arguments in an environment `E1 = [0]`, which is the
///    [Scheduler::Scope] of the right-hand side.
/// 2. The term stack computes `0 == 0`. [Scheduler::hook] pushes an empty slot for each branch,
///    and for `if` it schedules a [Scheduler::Continuation].
/// 3. [Scheduler::resume] tries the rules of `if` on `[true, _, _]`. The first one applies, so
///    its right-hand side `x` is evaluated in `E2 = [true, (T1, E1), (T2, E1)]`, where `T1` and
///    `T2` are the term stacks of the branches.
/// 4. `x` is a hook as well, which evaluates `T1` in `E1` and leaves `0`. `T2` is never
///    evaluated, so `f(n - 1)` is never built.
///
/// A lazy symbol in the input term starts through [Scheduler::decompose] instead, with its
/// unrewritten subterms in the slots of its arguments.
///
/// | Method | [Innermost] | Jitty |
/// |---|---|---|
/// | [Scheduler::decompose] | declines, so arguments are rewritten first | starts the strategy of a lazy symbol |
/// | [Scheduler::enter] | nothing | creates an environment if the right-hand side has hooks |
/// | [Scheduler::variable] | reads the arguments on the stack | also reads an environment |
/// | [Scheduler::hook] | there are none | suspends an argument, or starts a strategy |
/// | [Scheduler::resume] | there are none | takes the next steps of a strategy |
///
/// Every method gets a [MachineView], and changes the machine only through it. The stacks change
/// as each method says; what it schedules runs after it returns.
pub(crate) trait Scheduler: Sized + 'static {
    /// What a hook in a right-hand side stands for.
    type Hook;
    /// The work of the scheduler that is on the stack, see [Scheduler::resume].
    type Continuation: Copy + fmt::Debug + Send + Sync + 'static;
    /// What the variables of a right-hand side that is being evaluated stand for.
    type Scope: Copy + fmt::Debug + Send + Sync + 'static;
    /// What the scheduler changes while rewriting.
    type State: Default;

    /// Called when the machine takes `term` apart. Returns `false`, with the stacks unchanged, to
    /// have its arguments rewritten, from left to right, before it is matched. Otherwise the
    /// scheduler is responsible for pushing its normal form, in the end.
    fn decompose(&self, machine: &mut MachineView<'_, '_, '_, Self>, term: &DataExpressionRef<'_>) -> bool;

    /// Returns the scope for evaluating `right_hand_side`, of a rule that matches the redex with
    /// the `arity` arguments on top. The scheduler may remove those arguments.
    fn enter(
        &self,
        machine: &mut MachineView<'_, '_, '_, Self>,
        right_hand_side: TermStackId,
        arity: usize,
    ) -> Self::Scope;

    /// Pushes the value of the variable at `position` of the left-hand side, where `base` is the
    /// slot of the first argument of the redex.
    fn variable(
        &self,
        machine: &mut MachineView<'_, '_, '_, Self>,
        scope: Self::Scope,
        base: usize,
        position: &DataPosition,
    );

    /// Called when the evaluation of a right-hand side in `scope` has finished, which leaves the
    /// stacks to the machine.
    fn leave(&self, machine: &mut MachineView<'_, '_, '_, Self>, scope: Self::Scope);

    /// Evaluates `hook`, which a term stack has reached in `scope`. It changes the terms as the
    /// instruction that it stands for: it pushes one term, possibly by scheduling what does, and
    /// can replace terms that hooks before it pushed.
    fn hook(&self, machine: &mut MachineView<'_, '_, '_, Self>, hook: &Self::Hook, scope: Self::Scope);

    /// Continues the work that was scheduled with `continuation`.
    fn resume<'s>(
        &'s self,
        machine: &mut MachineView<'_, '_, '_, Self>,
        continuation: Self::Continuation,
    ) -> Resumed<'s, Self::Continuation>;

    /// Continues after [Resumed::Conditions], with the rule that `found` is, if any.
    fn matched(
        &self,
        machine: &mut MachineView<'_, '_, '_, Self>,
        continuation: Self::Continuation,
        found: Option<MatchResult<'_, Annotation>>,
    );
}

/// What [Scheduler::resume] leaves to do.
pub(crate) enum Resumed<'a, K> {
    /// Nothing, apart from what it has scheduled.
    Done,
    /// The walk through `automaton` stopped at a rule with conditions, for the redex that applies
    /// `head` to the `arity` arguments on top. Checking them rewrites, which the machine does
    /// before it calls [Scheduler::matched] with `continuation`.
    Conditions {
        automaton: &'a SetAutomaton<Annotation>,
        head: Head<'a>,
        arity: usize,
        walk: Walk<'a>,
        continuation: K,
    },
}
