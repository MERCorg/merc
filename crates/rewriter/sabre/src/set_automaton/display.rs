use itertools::Itertools;

use crate::set_automaton::SetAutomaton;
use crate::set_automaton::State;
use core::fmt;

use super::MatchAnnouncement;
use super::MatchObligation;
use super::Transition;

impl<M> fmt::Debug for Transition<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Transition {{ {}, announce: [{:?}], dest: [{:?}] }}",
            self.symbol,
            self.announcements.iter().map(|(x, _)| { x }).format(", "),
            self.destinations.iter().format_with(", ", |element, f| {
                f(&format_args!("{} -> {}", element.0, element.1))
            })
        )
    }
}

impl fmt::Debug for MatchAnnouncement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({})@{}", self.rule, self.position)
    }
}

impl fmt::Debug for MatchObligation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.pattern, self.position)
    }
}

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Label: {}, ", self.label())?;
        writeln!(f, "Match goals: [")?;
        for m in self.match_goals() {
            writeln!(f, "\t {m:?}")?;
        }
        write!(f, "]")
    }
}

impl<M> fmt::Debug for SetAutomaton<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "States: {{")?;

        for (state_index, s) in self.states().iter().enumerate() {
            writeln!(f, "State {state_index} {{\n{s:?}")?;

            writeln!(f, "Transitions: {{")?;
            for (from, _, tr) in self.iter_transitions() {
                if state_index == from {
                    writeln!(f, "\t {tr:?}")?;
                }
            }
            writeln!(f, "}}")?;
        }

        writeln!(f, "}}")
    }
}

pub struct DotFormatter<'a, M> {
    pub(crate) automaton: &'a SetAutomaton<M>,
    pub(crate) show_backtransitions: bool,
    pub(crate) show_final: bool,
}

impl<M> fmt::Display for DotFormatter<'_, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Write the header anf final states.
        writeln!(f, "digraph{{")?;

        if self.show_final {
            writeln!(f, "  final[label=\"💩\"];")?;
        }

        for (i, s) in self.automaton.states().iter().enumerate() {
            let match_goals = s.match_goals().iter().format_with("\\n", |goal, f| {
                f(&format_args!("{}", html_escape::encode_safe(&format!("{goal:?}"))))
            });

            writeln!(
                f,
                "  s{}[shape=record label=\"{{{{s{} | {}}} | {}}}\"]",
                i,
                i,
                s.label(),
                match_goals
            )?;
        }

        for (i, _, tr) in self.automaton.iter_transitions() {
            let announcements = tr.announcements.iter().format_with(", ", |(announcement, _), f| {
                f(&format_args!("{}@{}", announcement.rule.rhs, announcement.position))
            });

            if tr.destinations.is_empty() {
                if self.show_final {
                    writeln!(f, "  s{} -> final [label=\"{} \\[{}\\]\"]", i, tr.symbol, announcements)?;
                }
            } else {
                writeln!(f, "  \"s{}{}\" [shape=point]", i, tr.symbol,)?;
                writeln!(
                    f,
                    "  s{} -> \"s{}{}\" [label=\"{} \\[{}\\]\"]",
                    i, i, tr.symbol, tr.symbol, announcements
                )?;

                for (pos, des) in &tr.destinations {
                    if self.show_backtransitions || *des != 0 {
                        // Hide backpointers to the initial state.
                        writeln!(f, "  \"s{}{}\" -> s{} [label = \"{}\"]", i, tr.symbol, des, pos)?;
                    }
                }
            }
        }
        writeln!(f, "}}")
    }
}
