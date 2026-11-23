#![forbid(unsafe_code)]

use std::fmt;

use ahash::AHashMap;
use merc_data::DataFunctionSymbol;
use merc_data::DataFunctionSymbolRef;

use crate::Rule;

/// What a rewriter did during its last rewrite, to compare rewriters and to find the rules that
/// dominate a computation.
///
/// Only gathered with the `metrics` feature, since counting slows down every step; without it
/// every count is zero.
#[derive(Clone, Debug, Default)]
pub struct RewriteMetrics {
    /// The number of rule applications, including natively evaluated operations.
    pub rewrite_steps: usize,
    /// The number of symbols that the automata inspected while matching.
    pub symbol_comparisons: usize,
    /// The number of normalisations, including the nested ones of conditions and quantifiers.
    pub recursions: usize,
    /// The number of condition sides whose normal form was in the condition cache.
    pub condition_cache_hits: usize,
    /// The number of strategy steps that tried rules, see [crate::JittyRewriter].
    pub tries: usize,
    /// The number of those that applied a rule.
    pub successful_tries: usize,
    /// The number of applications of each rule.
    rules: AHashMap<Rule, usize>,
    /// The number of native evaluations of each machine-word operation, by operation id.
    native: AHashMap<usize, (DataFunctionSymbol, usize)>,
}

impl RewriteMetrics {
    /// Returns the number of applications of every applied rule.
    pub fn rules(&self) -> impl Iterator<Item = (&Rule, usize)> {
        self.rules.iter().map(|(rule, count)| (rule, *count))
    }

    /// Returns the number of native evaluations of every evaluated machine-word operation.
    pub fn native(&self) -> impl Iterator<Item = (&DataFunctionSymbol, usize)> {
        self.native.values().map(|(symbol, count)| (symbol, *count))
    }

    /// Clears the counts, keeping the allocated tables.
    pub(crate) fn reset(&mut self) {
        if cfg!(feature = "metrics") {
            self.rewrite_steps = 0;
            self.symbol_comparisons = 0;
            self.recursions = 0;
            self.condition_cache_hits = 0;
            self.tries = 0;
            self.successful_tries = 0;
            self.rules.clear();
            self.native.clear();
        }
    }

    pub(crate) fn rule_applied(&mut self, rule: &Rule) {
        if cfg!(feature = "metrics") {
            self.rewrite_steps += 1;
            match self.rules.get_mut(rule) {
                Some(count) => *count += 1,
                None => {
                    self.rules.insert(rule.clone(), 1);
                }
            }
        }
    }

    pub(crate) fn native_applied(&mut self, symbol: &DataFunctionSymbolRef<'_>) {
        if cfg!(feature = "metrics") {
            self.rewrite_steps += 1;
            self.native
                .entry(symbol.operation_id())
                .or_insert_with(|| (symbol.protect(), 0))
                .1 += 1;
        }
    }

    pub(crate) fn symbol_compared(&mut self) {
        if cfg!(feature = "metrics") {
            self.symbol_comparisons += 1;
        }
    }

    pub(crate) fn recursion(&mut self) {
        if cfg!(feature = "metrics") {
            self.recursions += 1;
        }
    }

    pub(crate) fn condition_cache_hit(&mut self) {
        if cfg!(feature = "metrics") {
            self.condition_cache_hits += 1;
        }
    }

    pub(crate) fn tried(&mut self, applied: bool) {
        if cfg!(feature = "metrics") {
            self.tries += 1;
            self.successful_tries += usize::from(applied);
        }
    }
}

impl fmt::Display for RewriteMetrics {
    /// Shows the counts, and the most applied rules and operations; the alternate form `{:#}`
    /// shows all of them.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if cfg!(not(feature = "metrics")) {
            return write!(f, "enable the 'metrics' feature to see rewrite metrics");
        }

        write!(
            f,
            "{} steps, {} symbol comparisons, {} normalisations, {} condition cache hits",
            self.rewrite_steps, self.symbol_comparisons, self.recursions, self.condition_cache_hits
        )?;
        if self.tries > 0 {
            write!(f, ", {} of {} tries applied a rule", self.successful_tries, self.tries)?;
        }

        let mut applied: Vec<(String, usize)> = self
            .rules
            .iter()
            .map(|(rule, count)| (rule.to_string(), *count))
            .chain(
                self.native
                    .values()
                    .map(|(symbol, count)| (format!("{symbol} (native)"), *count)),
            )
            .collect();
        // The names break ties, so that the output does not depend on the hash order.
        applied.sort_by(|(left, left_count), (right, right_count)| {
            right_count.cmp(left_count).then_with(|| left.cmp(right))
        });

        let shown = if f.alternate() { applied.len() } else { 10 };
        for (rule, count) in applied.iter().take(shown) {
            write!(f, "\n{count:>12}  {rule}")?;
        }
        if applied.len() > shown {
            write!(f, "\n{:>12}  more rules", applied.len() - shown)?;
        }
        Ok(())
    }
}
