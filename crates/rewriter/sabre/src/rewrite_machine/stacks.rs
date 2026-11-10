use merc_aterm::ProtectedWriteGuard;
use merc_data::DataExpressionRef;
use merc_data::DataFunctionSymbolRef;

use super::Config;
use super::Evaluate;
use super::Scheduler;
use super::TermStackId;
use crate::subject::subterm;
use crate::utilities::DataPosition;

/// The two stacks of the rewriting machine, while the guards on them are held.
///
/// What is done to the terms happens immediately. What is scheduled happens after the current
/// step, the last one scheduled first: to rewrite `a` and then `b`, schedule `b` first.
///
/// A slot is an index in the terms, and can be empty for an argument that is filled in later.
pub(crate) struct Stacks<'c, 't, S: Scheduler> {
    pub(super) configs: &'c mut ProtectedWriteGuard<'c, 'static, Vec<Config<'c, S>>>,
    pub(super) terms: &'t mut ProtectedWriteGuard<'t, 'static, Vec<Option<DataExpressionRef<'t>>>>,
}

impl<S: Scheduler> Stacks<'_, '_, S> {
    /// Schedules pushing the normal form of `term`.
    pub fn schedule_rewrite(&mut self, term: &DataExpressionRef<'_>) {
        // SAFETY: term is pushed into the container on the next line.
        let term = unsafe { self.configs.protect(term) };
        self.configs.push(Config::Rewrite { term: term.into() });
    }

    /// Schedules pushing the normal form of the term in `slot`, which is left empty.
    pub fn schedule_rewrite_slot(&mut self, slot: usize) {
        let term = self.terms[slot].take().expect("the slot holds a term");
        self.schedule_rewrite(&term);
    }

    /// Schedules replacing the `arity` terms that are then on top by the normal form of `symbol`
    /// applied to them.
    pub fn schedule_construct(&mut self, symbol: &DataFunctionSymbolRef<'_>, arity: usize) {
        // SAFETY: symbol is pushed into the container on the next line.
        let symbol = unsafe { self.configs.protect(symbol) };
        self.configs.push(Config::Construct {
            symbol: symbol.into(),
            arity,
        });
    }

    /// Schedules replacing the terms from slot `base` on by the value of `term_stack` in `scope`.
    pub fn schedule_evaluate(&mut self, term_stack: TermStackId, base: usize, scope: S::Scope) {
        self.configs
            .push(Config::Evaluate(Evaluate::new(term_stack, base, scope)));
    }

    /// Schedules [Scheduler::resume] with `continuation`.
    pub fn schedule_resume(&mut self, continuation: S::Continuation) {
        self.configs.push(Config::Resume(continuation));
    }

    /// Returns the slot that the next term is pushed to.
    pub fn len(&self) -> usize {
        self.terms.len()
    }

    /// Returns the term in `slot`, which must not be empty.
    pub fn argument(&self, slot: usize) -> DataExpressionRef<'_> {
        self.terms[slot].as_ref().expect("the slot holds a term").copy()
    }

    /// Pushes `value`.
    pub fn push(&mut self, value: &DataExpressionRef<'_>) {
        // SAFETY: value is pushed into the container on the next line.
        let value = unsafe { self.terms.protect(value) };
        self.terms.push(Some(value.into()));
    }

    /// Pushes an empty slot.
    pub fn push_empty(&mut self) {
        self.terms.push(None);
    }

    /// Pushes the subterm at `position` of the arguments from slot `base` on.
    pub fn push_argument(&mut self, base: usize, position: &DataPosition) {
        // SAFETY: the value is pushed into the container on the next line.
        let value = unsafe { self.terms.protect(&subterm(&self.terms[base..], position)) };
        self.terms.push(Some(value.into()));
    }

    /// Moves the term on top to `slot`.
    pub fn move_top_to(&mut self, slot: usize) {
        let value = self.terms.pop().expect("there is a term to move");
        self.terms[slot] = value;
    }

    /// Removes the slots from `base` on.
    pub fn truncate(&mut self, base: usize) {
        self.terms.truncate(base);
    }

    /// Returns the `arity` slots from `base` on.
    pub(super) fn arguments(&self, base: usize, arity: usize) -> &[Option<DataExpressionRef<'_>>] {
        &self.terms[base..base + arity]
    }

    /// Replaces the slots from `base` on by `value`.
    pub(super) fn replace_from(&mut self, base: usize, value: &DataExpressionRef<'_>) {
        // SAFETY: value is pushed into the container below, before the guard is dropped.
        let value = unsafe { self.terms.protect(value) };
        self.terms.truncate(base);
        self.terms.push(Some(value.into()));
    }

    /// Checks in debug builds that what this step protected is on the stacks, see
    /// [ProtectedWriteGuard::checkpoint].
    pub(super) fn checkpoint(&self) {
        self.configs.checkpoint();
        self.terms.checkpoint();
    }
}
