use std::io::BufReader;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;

use log::debug;
use log::info;
use merc_aterm::ATerm;
use merc_aterm::ATermList;
use merc_aterm::ATermRead;
use merc_aterm::ATermStreamable;
use merc_aterm::ATermWrite;
use merc_aterm::BinaryATermReader;
use merc_aterm::BinaryATermWriter;
use merc_aterm::Symbol;
use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_data::Mcrl2DataSpecification;
use merc_io::BitStreamRead;
use merc_io::BitStreamWrite;
use merc_lts::LtsAction;
use merc_lts::LtsMultiAction;
use merc_utilities::MercError;
use oxidd::ldd::LDDManagerRef;

use crate::BinaryLddReader;
use crate::BinaryLddWriter;
use crate::SummandGroup;
use crate::SymbolicLPS;
use crate::SymbolicLTS;
use crate::SymbolicLts;
use crate::TransitionGroup;

/// Reads a symbolic LTS from a binary stream in the mCRL2 `.sym` format.
///
/// # Details
///
/// The stream contains
///
/// ```plain
/// <marker>: ATerm
/// <data specification>
/// <process parameters>: ATermList<ATerm>
///
/// <initial state>: LDD
/// <states>: LDD
///
/// For each process parameter:
///   <number of entries>: u64
///   For each entry:
///     <value>: ATerm
///
/// <number of action labels>: u64
/// For each action label:
///   <action label>: ATerm
///
/// <number of summand groups>: u64
/// For each summand group:
///   <number of read parameters>: u64
///   For each read parameter:
///     <read parameter>: ATerm
///
///   <number of write parameters>: u64
///   For each write parameter:
///     <write parameter>: ATerm
/// ```
pub fn read_symbolic_lts<R: Read>(
    manager: &LDDManagerRef,
    reader: R,
) -> Result<SymbolicLts<LtsMultiAction<LtsAction>>, MercError> {
    info!("Reading symbolic LTS in the mCRL2 symbolic format...");

    let aterm_stream = BinaryATermReader::new(BufReader::new(reader))?;
    let mut stream = BinaryLddReader::new(manager, aterm_stream)?;

    if ATermRead::read_aterm(&mut stream)? != Some(symbolic_labelled_transition_system_mark()) {
        return Err("Expected symbolic labelled transition system stream".into());
    }

    let data_spec = Mcrl2DataSpecification::read(&mut stream)?;
    let process_parameters: ATermList<DataVariable> = stream.read_aterm()?.ok_or("Expected process parameters")?.into();
    let process_parameters: Vec<DataVariable> = process_parameters.to_vec();

    let initial_state = stream.read_ldd(manager)?;
    let states = stream.read_ldd(manager)?;

    // Read the values for the process parameters.
    let mut parameter_values: Vec<Vec<DataExpression>> = Vec::with_capacity(process_parameters.len());
    for parameter in &process_parameters {
        let num_of_entries = stream.read_integer()?;
        debug!(
            "Parameter {}: {} has {} entries",
            parameter_values.len(),
            parameter,
            num_of_entries
        );

        let mut values = Vec::with_capacity(num_of_entries as usize);
        for i in 0..num_of_entries {
            let value = stream.read_aterm()?.ok_or("Unexpected end of stream")?;

            let expr: DataExpression = value.into();
            debug!("  {i}:  {}", expr);
            values.push(expr);
        }

        parameter_values.push(values);
    }

    // Read the action labels.
    let num_of_action_labels = stream.read_integer()?;
    let mut action_labels = Vec::with_capacity(num_of_action_labels as usize);
    for _ in 0..num_of_action_labels {
        let action_label = stream.read_aterm()?.ok_or("Unexpected end of stream")?;
        let action = LtsMultiAction::from_mcrl2_aterm(action_label)?;

        debug!("Action {}: {}", action_labels.len(), action);
        action_labels.push(action);
    }

    // Read the summand groups.
    let mut summand_groups = Vec::new();
    let num_of_groups = stream.read_integer()?;
    for _ in 0..num_of_groups {
        // Note: this is not an ATermInt, as expected by `read_aterm_iter`, but a variable integer.
        let num_of_reads = stream.read_integer()?;
        let mut read_parameters: Vec<DataVariable> = Vec::with_capacity(num_of_reads as usize);
        for _ in 0..num_of_reads {
            read_parameters.push(stream.read_aterm()?.ok_or("Unexpected end of stream")?.into());
        }

        let num_of_writes = stream.read_integer()?;
        let mut write_parameters: Vec<DataVariable> = Vec::with_capacity(num_of_writes as usize);
        for _ in 0..num_of_writes {
            write_parameters.push(stream.read_aterm()?.ok_or("Unexpected end of stream")?.into());
        }

        let relation = stream.read_ldd(manager)?;

        summand_groups.push(SummandGroup::new(
            manager,
            &process_parameters,
            read_parameters,
            write_parameters,
            relation,
        )?);
    }

    info!("Finished reading symbolic LTS.");
    Ok(SymbolicLts::new(
        data_spec,
        process_parameters,
        states,
        initial_state,
        summand_groups,
        action_labels,
        parameter_values,
    ))
}

/// Writes a symbolic LTS to a binary stream in the mCRL2 `.sym` format, see [read_symbolic_lts]
/// for the structure of the stream.
pub fn write_symbolic_lts<W: Write>(
    manager: &LDDManagerRef,
    writer: W,
    lts: &SymbolicLts<LtsMultiAction<LtsAction>>,
) -> Result<(), MercError> {
    info!("Writing symbolic LTS in the mCRL2 symbolic format...");

    let aterm_stream = BinaryATermWriter::new(BufWriter::new(writer))?;
    let mut stream = BinaryLddWriter::new(manager, aterm_stream)?;

    stream.write_aterm(&symbolic_labelled_transition_system_mark())?;

    lts.data_specification().write(&mut stream)?;

    let process_parameters: ATerm =
        ATermList::<DataVariable>::from_double_iter(lts.process_parameters().iter().cloned()).into();
    stream.write_aterm(&process_parameters)?;

    stream.write_ldd(lts.initial_state())?;
    stream.write_ldd(lts.states())?;

    // Write the values for the process parameters.
    for (index, values) in lts.parameter_values().iter().enumerate() {
        debug!("Parameter {}: has {} entries", index, values.len());

        stream.write_integer(values.len() as u64)?;
        for (i, value) in values.iter().enumerate() {
            debug!("  {i}:  {}", value);
            stream.write_aterm(&value.clone().into())?;
        }
    }

    // Write the action labels.
    stream.write_integer(lts.action_labels().len() as u64)?;
    for (i, label) in lts.action_labels().iter().enumerate() {
        debug!("Action {}: {}", i, label);
        stream.write_aterm(&label.to_mcrl2_aterm()?)?;
    }

    // Write the summand groups.
    stream.write_integer(lts.transition_groups().len() as u64)?;
    for group in lts.transition_groups() {
        stream.write_integer(group.read_parameters().len() as u64)?;
        for parameter in group.read_parameters() {
            stream.write_aterm(&parameter.clone().into())?;
        }

        stream.write_integer(group.write_parameters().len() as u64)?;
        for parameter in group.write_parameters() {
            stream.write_aterm(&parameter.clone().into())?;
        }

        stream.write_ldd(group.relation())?;
    }

    ATermWrite::flush(&mut stream)?;
    info!("Finished writing symbolic LTS.");
    Ok(())
}

/// Returns the ATerm mark for symbolic labelled transition systems.
fn symbolic_labelled_transition_system_mark() -> ATerm {
    ATerm::constant(&Symbol::new("symbolic_labelled_transition_system", 0))
}

#[cfg(test)]
mod tests {
    use merc_utilities::random_test;
    use merc_utilities::test_logger;

    use crate::LDD_CACHE_CAPACITY;
    use crate::LDD_NODE_CAPACITY;
    use crate::SymbolicLPS;
    use crate::SymbolicLTS;
    use crate::TransitionGroup;
    use crate::random_symbolic_lts;

    use super::read_symbolic_lts;
    use super::write_symbolic_lts;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_read_symbolic_lts_wms_sym() {
        test_logger();
        let input = include_bytes!("../../../../examples/lts/WMS.sym");

        let ldd_manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let _lts = read_symbolic_lts(&ldd_manager, &input[..]).unwrap();
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_random_symbolic_lts_io() {
        random_test(20, |rng| {
            let ldd_manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
            let lts = random_symbolic_lts(rng, &ldd_manager, 5, 3).unwrap();

            let mut buffer: Vec<u8> = Vec::new();
            write_symbolic_lts(&ldd_manager, &mut buffer, &lts).unwrap();

            let result = read_symbolic_lts(&ldd_manager, &buffer[..]).unwrap();

            assert!(lts.states() == result.states());
            assert!(lts.initial_state() == result.initial_state());
            assert_eq!(lts.action_labels(), result.action_labels());
            assert_eq!(lts.parameter_values().len(), result.parameter_values().len());
            for (a, b) in lts.parameter_values().iter().zip(result.parameter_values()) {
                assert_eq!(a, b);
            }

            assert_eq!(lts.transition_groups().len(), result.transition_groups().len());
            for (a, b) in lts.transition_groups().iter().zip(result.transition_groups()) {
                assert!(a.relation() == b.relation());
                assert_eq!(a.read_parameters(), b.read_parameters());
                assert_eq!(a.write_parameters(), b.write_parameters());
            }
        });
    }
}
