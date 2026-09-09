//! Authors: Maurice Laveaux and Sjef van Loo

use std::io::BufWriter;
use std::io::Read;
use std::io::Write;

use itertools::Itertools;
use log::info;
use oxidd::BooleanFunction;
use oxidd::Manager;
use oxidd::ManagerRef;
use oxidd::bdd::BDDFunction;
use oxidd::bdd::BDDManagerRef;
use regex::Regex;
use streaming_iterator::StreamingIterator;

use merc_io::LineIterator;
use merc_io::TimeProgress;
use merc_symbolic::FormatConfigSet;
use merc_symbolic::minus;
use merc_utilities::MercError;

use crate::IOError;
use crate::PG;
use crate::Player;
use crate::Priority;
use crate::VariabilityParityGame;
use crate::VariabilityParityGameBuilder;
use crate::VertexIndex;

/// Reads a variability parity game in an extended PGSolver `.vpg` format from the given reader.
/// Note that the reader is buffered internally using a `BufReader`.
///
/// # Details
///
/// The format starts with a header, followed by the vertices
///
/// parity <num_of_vertices>;
/// `<index> <priority> <owner> <outgoing_vertex>,<outgoing_vertex>,...;`
/// Each outgoing edge is represented as `<to>|<configuration_set>`, where the configuration set
/// is a string \<entry\>+\<entry\>+..., each entry a sequence of '-', '0', and '1' (don't care,
/// false, true) with one character per boolean variable, in variable order.
pub fn read_vpg<R: Read>(manager: &BDDManagerRef, reader: R) -> Result<VariabilityParityGame, MercError> {
    info!("Reading variability parity game in .vpg format...");

    manager.with_manager_exclusive(|manager| {
        debug_assert_eq!(
            manager.num_vars(),
            0,
            "A BDD manager can only hold the variables for a single variability parity game"
        )
    });

    let mut lines = LineIterator::new(reader);
    lines.advance();
    let header = lines
        .get()
        .ok_or(IOError::InvalidHeader("The first line should be the confs header"))?;

    // Read the confs <configurations> line
    let confs_regex = Regex::new(r#"confs\s+([+-01]*)\s*;"#).expect("Regex compilation should not fail");
    let (_, [configurations_txt]) = confs_regex
        .captures(header)
        .ok_or(IOError::InvalidHeader("header does not match confs <configurations>;"))?
        .extract();
    let (variables, configurations) = parse_configuration(manager, configurations_txt)?;

    // Read the parity header
    let header_regex = Regex::new(r#"parity\s+([0-9]+)\s*;"#).expect("Regex compilation should not fail");
    let header = lines
        .next()
        .ok_or(IOError::InvalidHeader("The second line should be the parity header"))?;

    let (_, [num_of_vertices_txt]) = header_regex
        .captures(header)
        .ok_or(IOError::InvalidHeader(
            "header does not match parity <num_of_vertices>;",
        ))?
        .extract();

    let num_of_vertices: usize = num_of_vertices_txt.parse()?;

    // Collect the data in a builder and remove duplicate edges at the end.
    let mut builder = VariabilityParityGameBuilder::with_capacity(VertexIndex::new(0), num_of_vertices);
    if num_of_vertices > 0 {
        builder.add_vertex(VertexIndex::new(num_of_vertices - 1), Player::Even, Priority::new(0));
    }

    // Print progress messages
    let progress = TimeProgress::new(
        |(amount, total): (usize, usize)| info!("Read {} vertices ({}%)...", amount, amount * 100 / total),
        1,
    );
    let mut vertex_count = 0;
    while let Some(line) = lines.next() {
        // Parse the line: <index> <priority> <owner> <outgoing_vertex>, <outgoing_vertex>, ...;
        let mut parts = line.split_whitespace();

        let index: usize = parts
            .next()
            .ok_or(IOError::InvalidLine("Expected at least <index> ...;"))?
            .parse()?;
        let vertex_priority: usize = parts
            .next()
            .ok_or(IOError::InvalidLine("Expected at least <index> <priority> ...;"))?
            .parse()?;
        let vertex_owner = Player::try_from_index(
            parts
                .next()
                .ok_or(IOError::InvalidLine(
                    "Expected at least <index> <priority> <owner> ...;",
                ))?
                .parse()?,
        )
        .ok_or(IOError::InvalidLine("owner must be 0 (even) or 1 (odd)"))?;

        let vertex = VertexIndex::new(index);
        builder.add_vertex(vertex, vertex_owner, Priority::new(vertex_priority));

        for successors in parts {
            // Parse successors (remaining parts, removing trailing semicolon)
            for successor in successors
                .trim_end_matches(';')
                .split(',')
                .filter(|s| !s.trim().is_empty())
            {
                // Each successor is `<to>` or `<to>|<configuration_set>`.
                let (index_part, configuration_part) = match successor.trim().split_once('|') {
                    Some((index, configuration)) => (index, Some(configuration)),
                    None => (successor.trim(), None),
                };
                let successor = VertexIndex::new(index_part.trim().parse()?);

                let edge_configuration = match configuration_part {
                    Some(configuration) => parse_configuration_set(manager, &variables, configuration.trim())?,
                    // No configuration specified, use true (all configurations)
                    None => manager.with_manager_shared(|m| BDDFunction::t(m)),
                };

                builder.add_edge(vertex, edge_configuration, successor);
            }
        }

        progress.print((vertex_count + 1, num_of_vertices));
        vertex_count += 1;
    }

    Ok(builder.finish(manager, configurations, variables, true))
}

/// Parses a configuration set from a string representation into a BDD function, but also creates the necessary variables.
/// based on the length of the configurations.
fn parse_configuration(
    manager_ref: &BDDManagerRef,
    config: &str,
) -> Result<(Vec<BDDFunction>, BDDFunction), MercError> {
    // Check for existing variables
    if manager_ref.with_manager_shared(|manager| manager.num_vars()) != 0 {
        return Err("BDD manager must not contain any variables yet".into());
    }

    if let Some(first_part) = config.split('+').next() {
        let variables = manager_ref.with_manager_exclusive(|manager| {
            manager
                .add_vars(first_part.len() as u32)
                .map(|i| BDDFunction::var(manager, i))
                .collect::<Result<Vec<_>, _>>()
        })?;

        let configuration = parse_configuration_set(manager_ref, &variables, config)?;
        return Ok((variables.to_vec(), configuration));
    };

    Err(MercError::from(IOError::InvalidHeader("Empty configuration string")))
}

/// Parses a configuration from a string representation into a BDD function.
///
/// # Details
///
/// A configuration is represented as a string \<entry\>+\<entry\>+..., where each entry is either
/// a sequence consisting of '-', '0', and '1', representing don't care, false, and true respectively.
/// The length of the sequence determines the number of boolean variables. So `-1--` represents a boolean
/// function over 4 variables.
///
/// The variables must be defined beforehand and are assumed to be in order, i.e., the first character
/// corresponds to variable 0, the second to variable 1, and so on.
pub fn parse_configuration_set(
    manager_ref: &BDDManagerRef,
    variables: &[BDDFunction],
    config: &str,
) -> Result<BDDFunction, MercError> {
    manager_ref.with_manager_shared(|manager| -> Result<BDDFunction, MercError> {
        let mut result = BDDFunction::f(manager);

        for part in config.split('+') {
            let mut conjunction = BDDFunction::t(manager);

            for (i, c) in part.chars().enumerate() {
                let var = variables.get(i).ok_or(IOError::InvalidLine(
                    "configuration entry has more characters than there are variables",
                ))?;
                match c {
                    '1' => conjunction = conjunction.and(var)?,
                    '0' => conjunction = minus(&conjunction, var)?,
                    '-' => {} // don't care
                    _ => {
                        return Err(MercError::from(IOError::InvalidHeader(
                            "Invalid character in configuration",
                        )));
                    }
                }
            }

            result = result.or(&conjunction)?;
        }

        Ok(result)
    })
}

/// Writes the given parity game to the given writer in .vpg format.
/// Note that the writer is buffered internally using a `BufWriter`.
pub fn write_vpg<W: Write>(writer: &mut W, game: &VariabilityParityGame) -> Result<(), MercError> {
    info!("Writing variability parity game to .vpg format...");
    let mut writer = BufWriter::new(writer);

    writeln!(writer, "confs {};", FormatConfigSet(game.configuration()))?;
    writeln!(writer, "parity {};", game.num_of_vertices())?;

    let progress = TimeProgress::new(
        |(index, total): (usize, usize)| info!("Wrote {} vertices ({}%)...", index, index * 100 / total),
        1,
    );
    for v in game.iter_vertices() {
        let prio = game.priority(v);
        let owner = game.owner(v).to_index();

        write!(writer, "{} {} {} ", v.value(), prio.value(), owner)?;
        write!(
            writer,
            "{}",
            game.outgoing_edges(v).format_with(",", |edge, fmt| {
                fmt(&format_args!("{}|{}", edge.to(), FormatConfigSet(edge.label())))
            })
        )?;

        writeln!(writer, ";")?;
        progress.print((v.value() + 1, game.num_of_vertices()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::PG;
    use super::read_vpg;

    #[test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_read_vpg() {
        let manager = oxidd::bdd::new_manager(2048, 1024, 8);

        let parity_game = read_vpg(
            &manager,
            include_bytes!("../../../../examples/vpg/example.vpg") as &[u8],
        )
        .unwrap();

        assert_eq!(parity_game.num_of_vertices(), 3002);
        assert_eq!(parity_game.num_of_edges(), 4408);
    }
}
