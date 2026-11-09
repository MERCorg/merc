use std::io::BufReader;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;

use merc_aterm::ATerm;
use merc_aterm::ATermInt;
use merc_aterm::ATermRead;
use merc_aterm::ATermStreamable;
use merc_aterm::ATermWrite;
use merc_aterm::BinaryATermReader;
use merc_aterm::BinaryATermWriter;
use merc_aterm::Symbol;
use merc_aterm::is_int_term;
use merc_data::Mcrl2DataSpecification;
use merc_data::is_data_variable;
use merc_utilities::MercError;

use crate::FixpointSymbol;
use crate::Pbes;
use crate::PbesEquation;
use crate::is_pbes_expression;
use crate::is_propositional_variable;
use crate::is_propositional_variable_instantiation;

/// Reads a PBES in the binary `.pbes` format of the mCRL2 toolset, and checks that it is well
/// typed; see [Pbes::check_well_typed].
pub fn read_pbes<R: Read>(reader: R) -> Result<Pbes, MercError> {
    let mut reader = BinaryATermReader::new(BufReader::new(reader))?;
    let pbes = Pbes::read(&mut reader)?;
    pbes.check_well_typed()?;
    Ok(pbes)
}

/// Writes a PBES in the binary `.pbes` format of the mCRL2 toolset.
pub fn write_pbes<W: Write>(pbes: &Pbes, writer: W) -> Result<(), MercError> {
    let mut writer = BinaryATermWriter::new(BufWriter::new(writer))?;
    pbes.write(&mut writer)?;
    writer.flush()
}

/// The layout is a marker, the data specification, the global variables, the
/// equations and the initial state. Each equation is written as three
/// consecutive terms instead of one, so the equations are a count followed by
/// these triples.
impl ATermStreamable for Pbes {
    fn write<W: ATermWrite>(&self, writer: &mut W) -> Result<(), MercError> {
        writer.write_aterm(&pbes_marker())?;
        self.data_specification.write(writer)?;
        writer.write_aterm_iter(self.global_variables.iter().cloned().map(ATerm::from))?;

        writer.write_aterm(&ATermInt::new(self.equations.len()))?;
        for equation in &self.equations {
            writer.write_aterm(&equation.symbol.to_term())?;
            writer.write_aterm(&equation.variable)?;
            writer.write_aterm(&equation.formula)?;
        }

        writer.write_aterm(&self.initial_state)
    }

    fn read<R: ATermRead>(reader: &mut R) -> Result<Self, MercError> {
        if next_term(reader, "PBES marker")? != pbes_marker() {
            return Err("Stream does not contain a parameterised boolean equation system (PBES).".into());
        }

        let data_specification = Mcrl2DataSpecification::read(reader)?;
        let global_variables = reader
            .read_aterm_iter()?
            .map(|term| expect(term?, |term| is_data_variable(term), "data variable"))
            .collect::<Result<Vec<_>, MercError>>()?;

        let count = next_term(reader, "number of equations")?;
        if !is_int_term(&count) {
            return Err(format!("Expected the number of equations, found {count}").into());
        }

        let count = ATermInt::from(count).value();
        let mut equations = Vec::with_capacity(count);
        for _ in 0..count {
            let symbol = fixpoint(&next_term(reader, "fixpoint symbol")?)?;
            let variable = expect(
                next_term(reader, "propositional variable")?,
                |term| is_propositional_variable(term),
                "propositional variable",
            )?;
            let formula = expect(
                next_term(reader, "equation formula")?,
                |term| is_pbes_expression(term),
                "PBES expression",
            )?;
            equations.push(PbesEquation::new(symbol, variable, formula));
        }

        let initial_state = expect(
            next_term(reader, "initial state")?,
            |term| is_propositional_variable_instantiation(term),
            "propositional variable instantiation",
        )?;

        Ok(Pbes::new(
            data_specification,
            global_variables,
            equations,
            initial_state,
        ))
    }
}

fn pbes_marker() -> ATerm {
    ATerm::constant(&Symbol::new("parameterised_boolean_equation_system", 0))
}

fn next_term<R: ATermRead>(reader: &mut R, what: &str) -> Result<ATerm, MercError> {
    reader
        .read_aterm()?
        .ok_or_else(|| format!("Unexpected end of stream, expected the {what}").into())
}

/// Converts `term` after checking its head symbol with `recogniser`; the subterms are not checked.
fn expect<T, F>(term: ATerm, recogniser: F, what: &str) -> Result<T, MercError>
where
    T: From<ATerm>,
    F: Fn(&ATerm) -> bool,
{
    if !recogniser(&term) {
        return Err(format!("Expected a {what}, found {term}").into());
    }

    Ok(term.into())
}

fn fixpoint(term: &ATerm) -> Result<FixpointSymbol, MercError> {
    FixpointSymbol::from_term(term).ok_or_else(|| format!("Expected a fixpoint symbol, found {term}").into())
}
