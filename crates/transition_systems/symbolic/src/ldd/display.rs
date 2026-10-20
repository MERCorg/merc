use std::collections::HashSet;
use std::fmt;

use itertools::Itertools;
use oxidd::ldd::LDDFunction;
use streaming_iterator::StreamingIterator;

use crate::Data;
use crate::iter;
use crate::iter_right;

/// Helper struct for displaying LDDs in DOT format.
pub struct LddDot<'a> {
    ldd: &'a LDDFunction,
}

impl<'a> LddDot<'a> {
    pub fn new(ldd: &'a LDDFunction) -> LddDot<'a> {
        LddDot { ldd }
    }
}

impl fmt::Display for LddDot<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            r#"
    digraph "DD" {{
    graph [dpi = 300];
    center = true;
    edge [dir = forward];

    "#
        )?;

        // Every node must be printed once, so keep track of already printed ones.
        #[allow(clippy::mutable_key_type)]
        let mut marked: HashSet<LDDFunction> = HashSet::new();

        // We don't show these nodes in the output since every right most node is 'false' and every bottom node is 'true'.
        // or in our terms empty_set and empty_vector. However, if the LDD itself is 'false' or 'true' we just show the single
        // node for clarity.
        if self.ldd.is_empty() {
            writeln!(f, "0 [shape=record, label=\"False\"];")?;
        } else if self.ldd.is_empty_vector() {
            writeln!(f, "1 [shape=record, label=\"True\"];")?;
        } else {
            print_node(f, &mut marked, self.ldd)?;
        }

        writeln!(f, "}}")
    }
}

/// Helper struct for displaying LDDs.
pub struct LddDisplay<'a> {
    ldd: &'a LDDFunction,
}

impl<'a> LddDisplay<'a> {
    pub fn new(ldd: &'a LDDFunction) -> LddDisplay<'a> {
        LddDisplay { ldd }
    }
}

impl fmt::Display for LddDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{{")?;

        let mut iter = iter(self.ldd);
        while let Some(vector) = iter.next() {
            writeln!(f, "\t[{}]", vector.iter().format(" "))?;
        }
        write!(f, "}}")
    }
}

#[allow(clippy::mutable_key_type)]
fn print_node(f: &mut fmt::Formatter<'_>, marked: &mut HashSet<LDDFunction>, ldd: &LDDFunction) -> fmt::Result {
    if marked.contains(ldd) || ldd.is_empty() || ldd.is_empty_vector() {
        Ok(())
    } else {
        // Print the node values
        write!(f, "{} [shape=record, label=\"", ldd.id())?;

        let mut first = true;
        for Data(value, _, _) in iter_right(ldd) {
            if !first {
                write!(f, "|")?;
            }

            write!(f, "<{value}> {value}")?;
            first = false;
        }
        writeln!(f, "\"];")?;

        // Print the edges.
        for Data(value, down, _) in iter_right(ldd) {
            if !down.is_empty() && !down.is_empty_vector() {
                writeln!(
                    f,
                    "{}:{} -> {}:{};",
                    ldd.id(),
                    value,
                    down.id(),
                    down.node().expect("down is an inner node").0
                )?;
            }
        }

        // Print all nodes.
        for Data(_, down, _) in iter_right(ldd) {
            print_node(f, marked, &down)?;
        }

        Ok(())
    }
}
