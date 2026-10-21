#![forbid(unsafe_code)]

use std::ffi::OsStr;
use std::fs::File;
use std::path::Path;

use merc_data::Mcrl2DataSpecification;
use merc_utilities::MercError;
use merc_utilities::Timing;

use crate::LTS;
use crate::LabelledTransitionSystem;
use crate::LtsAction;
use crate::LtsMultiAction;
use crate::LtsStateLabels;
use crate::SimpleAction;
use crate::read_aut;
use crate::read_bcg;
use crate::read_lts;
use crate::read_mcrl2_aut;

/// Convenience macro to call `GenericLts::apply` with the same function for all variants.
/// Useful with generic functions that can be monomorphized for all label types.
///
/// The closure receives the LTS, its associated extra data `E` (the
/// [`merc_data::Mcrl2DataSpecification`] for the [`GenericLts::Lts`] variant, `()` for the
/// others), and the arguments.
///
/// Examples:
/// - apply_lts!(lts, args, my_fn)
/// - apply_lts!(lts, args, |lts, extra, args| do_something(lts))
#[macro_export]
macro_rules! apply_lts {
    ($lts:expr, $arguments:expr, $f:path) => {
        $lts.apply($arguments, $f, $f, $f)
    };
    ($lts:expr, $arguments:expr, $f:expr) => {
        $lts.apply($arguments, $f, $f, $f)
    };
}

/// Convenience macro to apply a function to a pair of `GenericLts` only when both
/// are the same variant; returns an error otherwise.
///
/// The closure receives both LTSs, their associated extra data `E` (a pair of
/// [`merc_data::Mcrl2DataSpecification`] for the [`GenericLts::Lts`] variant, `()` for the
/// others), and the arguments.
///
/// Examples:
/// - apply_lts_pair!(lhs, rhs, args, my_fn)
/// - apply_lts_pair!(lhs, rhs, args, |a, b, extra, args| do_something(a, b, args))
#[macro_export]
macro_rules! apply_lts_pair {
    ($lhs:expr, $rhs:expr, $arguments:expr, $f:path) => {
        $lhs.apply_pair($rhs, $arguments, $f, $f, $f)
    };
    ($lhs:expr, $rhs:expr, $arguments:expr, $f:expr) => {
        $lhs.apply_pair($rhs, $arguments, $f, $f, $f)
    };
}

/// Convenience macro to apply a function to a slice of `GenericLts` when all
/// are the same variant; returns an error otherwise.
///
/// Examples:
/// - apply_lts_slice!(lts_slice, args, my_fn)
/// - apply_lts_slice!(lts_slice, args, |lts_vec, args| do_something(lts_vec, args))
#[macro_export]
macro_rules! apply_lts_slice {
    ($lts_slice:expr, $arguments:expr, $f:path) => {
        $crate::apply_slice($lts_slice, $arguments, $f, $f, $f)
    };
    ($lts_slice:expr, $arguments:expr, $f:expr) => {
        $crate::apply_slice($lts_slice, $arguments, $f, $f, $f)
    };
}

/// Explicitly specify the LTS file format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum LtsFormat {
    /// The AUTomaton or ALDEBARAN format
    Aut,
    /// The [Self::Aut] format with `tau` as hidden label instead of `i`, and multi-actions as labels, used in the mCRL2 toolset.
    AutMcrl2,
    /// The mCRL2 binary LTS format
    Lts,
    /// The CADP BCG format (requires 'cadp' feature)
    Bcg,
}

/// Guesses the LTS file format from the file extension.
pub fn guess_lts_format_from_extension(path: &Path, format: Option<LtsFormat>) -> Option<LtsFormat> {
    if let Some(format) = format {
        return Some(format);
    }

    if path.extension() == Some(OsStr::new("aut")) {
        Some(LtsFormat::Aut)
    } else if path.extension() == Some(OsStr::new("lts")) {
        Some(LtsFormat::Lts)
    } else if path.extension() == Some(OsStr::new("bcg")) {
        Some(LtsFormat::Bcg)
    } else {
        None
    }
}

/// Resolves the output LTS file format for a command that writes a single output file (or
/// stdout): the explicitly given `output_format` when set (matching
/// [`guess_lts_format_from_extension`]'s own precedence), otherwise guessed from `output`'s
/// extension, otherwise `default`.
pub fn guess_lts_output_format(
    output: Option<&Path>,
    output_format: Option<LtsFormat>,
    default: LtsFormat,
) -> LtsFormat {
    match output {
        Some(output) => guess_lts_format_from_extension(output, output_format),
        None => output_format,
    }
    .unwrap_or(default)
}

/// A general struct to deal with the polymorphic LTS types. The `apply_lts`
/// macro can be then used to conveniently apply functions which are generic on
/// the LTS trait to all variants.
pub enum GenericLts {
    /// The LTS in the Aldebaran format.
    Aut(LabelledTransitionSystem<String>),
    /// The mCRL2 LTS in the Aldebaran format. Multi-action labels are stored as
    /// [`SimpleAction`]s so the label name and string arguments are accessible.
    AutMcrl2(LabelledTransitionSystem<LtsMultiAction<SimpleAction>>),
    /// The LTS in the mCRL2 binary `.lts` format, together with the data specification its
    /// action arguments are typed over. Multi-action labels are stored as proper terms so they
    /// can be written back via [`crate::io_lts::write_lts`], which requires that specification.
    Lts(
        LabelledTransitionSystem<LtsMultiAction<LtsAction>, LtsStateLabels>,
        Mcrl2DataSpecification,
    ),
    /// The LTS in the CADP BCG format.
    Bcg(LabelledTransitionSystem<String>),
}

impl GenericLts {
    /// Applies the given function to both LTSs when they are the same variant.
    /// Returns an error if the variants do not match.
    ///
    /// Besides the two LTSs and the `arguments`, every closure also receives the extra data
    /// associated with the variant: a pair of [`Mcrl2DataSpecification`] for [`GenericLts::Lts`],
    /// and `()` for the other variants.
    pub fn apply_pair<T, FAut, FAutMcrl2, FLts, R>(
        self,
        other: GenericLts,
        arguments: T,
        apply_aut: FAut,
        apply_aut_mcrl2: FAutMcrl2,
        apply_lts: FLts,
    ) -> R
    where
        FAut: FnOnce(LabelledTransitionSystem<String>, LabelledTransitionSystem<String>, (), T) -> R,
        FAutMcrl2: FnOnce(
            LabelledTransitionSystem<LtsMultiAction<SimpleAction>>,
            LabelledTransitionSystem<LtsMultiAction<SimpleAction>>,
            (),
            T,
        ) -> R,
        FLts: FnOnce(
            LabelledTransitionSystem<LtsMultiAction<LtsAction>, LtsStateLabels>,
            LabelledTransitionSystem<LtsMultiAction<LtsAction>, LtsStateLabels>,
            (Mcrl2DataSpecification, Mcrl2DataSpecification),
            T,
        ) -> R,
    {
        match (self, other) {
            (GenericLts::Aut(a), GenericLts::Aut(b)) => apply_aut(a, b, (), arguments),
            (GenericLts::AutMcrl2(a), GenericLts::AutMcrl2(b)) => apply_aut_mcrl2(a, b, (), arguments),
            (GenericLts::Lts(a, da), GenericLts::Lts(b, db)) => apply_lts(a, b, (da, db), arguments),
            (GenericLts::Bcg(a), GenericLts::Bcg(b)) => apply_aut(a, b, (), arguments),
            _ => unreachable!("Mismatched GenericLts variants in apply_pair; this indicates a programming error"),
        }
    }

    /// Applies the given function to the LTS.
    ///
    /// Besides the LTS and the `arguments`, the closure also receives the extra data associated
    /// with the variant: the [`Mcrl2DataSpecification`] for [`GenericLts::Lts`], and `()` for the
    /// other variants.
    pub fn apply<T, F, G, H, R>(self, arguments: T, apply_aut: F, apply_aut_mcrl2: G, apply_lts: H) -> R
    where
        F: FnOnce(LabelledTransitionSystem<String>, (), T) -> R,
        G: FnOnce(LabelledTransitionSystem<LtsMultiAction<SimpleAction>>, (), T) -> R,
        H: FnOnce(LabelledTransitionSystem<LtsMultiAction<LtsAction>, LtsStateLabels>, Mcrl2DataSpecification, T) -> R,
    {
        match self {
            GenericLts::Aut(lts) => apply_aut(lts, (), arguments),
            GenericLts::AutMcrl2(lts) => apply_aut_mcrl2(lts, (), arguments),
            GenericLts::Lts(lts, data_spec) => apply_lts(lts, data_spec, arguments),
            GenericLts::Bcg(lts) => apply_aut(lts, (), arguments),
        }
    }

    // These are convenience functions to get LTS metrics.

    /// Returns the number of states in the LTS.
    pub fn num_of_states(&self) -> usize {
        match self {
            GenericLts::Aut(lts) => lts.num_of_states(),
            GenericLts::AutMcrl2(lts) => lts.num_of_states(),
            GenericLts::Lts(lts, _) => lts.num_of_states(),
            GenericLts::Bcg(lts) => lts.num_of_states(),
        }
    }

    /// Returns the number of transitions in the LTS.
    pub fn num_of_transitions(&self) -> usize {
        match self {
            GenericLts::Aut(lts) => lts.num_of_transitions(),
            GenericLts::AutMcrl2(lts) => lts.num_of_transitions(),
            GenericLts::Lts(lts, _) => lts.num_of_transitions(),
            GenericLts::Bcg(lts) => lts.num_of_transitions(),
        }
    }
}

/// Internal helper function for the `apply_lts_slice!` macro.
/// Applies a function to a slice of `GenericLts` when all are the same variant.
pub fn apply_slice<T, FAut, FAutMcrl2, FLts, R>(
    lts_slice: &[GenericLts],
    arguments: T,
    apply_aut: FAut,
    apply_aut_mcrl2: FAutMcrl2,
    apply_lts: FLts,
) -> Result<R, MercError>
where
    FAut: FnOnce(Vec<&LabelledTransitionSystem<String>>, T) -> R,
    FAutMcrl2: FnOnce(Vec<&LabelledTransitionSystem<LtsMultiAction<SimpleAction>>>, T) -> R,
    FLts: FnOnce(Vec<&LabelledTransitionSystem<LtsMultiAction<LtsAction>, LtsStateLabels>>, T) -> R,
{
    if lts_slice.is_empty() {
        return Err("Cannot apply function to empty slice of GenericLts".into());
    }

    match &lts_slice[0] {
        GenericLts::Aut(_) | GenericLts::Bcg(_) => {
            let aut_lts: Result<Vec<&LabelledTransitionSystem<String>>, MercError> = lts_slice
                .iter()
                .enumerate()
                .map(|(idx, lts)| match lts {
                    GenericLts::Aut(aut) | GenericLts::Bcg(aut) => Ok(aut),
                    _ => Err(format!("Expected Aut/Bcg variant at index {}, got a different variant", idx).into()),
                })
                .collect();
            Ok(apply_aut(aut_lts?, arguments))
        }
        GenericLts::AutMcrl2(_) => {
            let aut_lts: Result<Vec<&LabelledTransitionSystem<LtsMultiAction<SimpleAction>>>, MercError> = lts_slice
                .iter()
                .enumerate()
                .map(|(idx, lts)| match lts {
                    GenericLts::AutMcrl2(aut) => Ok(aut),
                    _ => Err(format!("Expected AutMcrl2 variant at index {}, got a different variant", idx).into()),
                })
                .collect();
            Ok(apply_aut_mcrl2(aut_lts?, arguments))
        }
        GenericLts::Lts(..) => {
            let lts_lts: Result<Vec<&LabelledTransitionSystem<LtsMultiAction<LtsAction>, LtsStateLabels>>, MercError> =
                lts_slice
                    .iter()
                    .enumerate()
                    .map(|(idx, lts)| match lts {
                        GenericLts::Lts(lts_obj, _) => Ok(lts_obj),
                        _ => Err(format!("Expected Lts variant at index {}, got a different variant", idx).into()),
                    })
                    .collect();
            Ok(apply_lts(lts_lts?, arguments))
        }
    }
}

/// Reads an explicit labelled transition system from the given path and format.
pub fn read_explicit_lts(path: &Path, format: LtsFormat, timing: &mut Timing) -> Result<GenericLts, MercError> {
    timing.measure("read_explicit_lts", || {
        let result = match format {
            LtsFormat::Aut => {
                let file = File::open(path)?;
                GenericLts::Aut(read_aut(&file)?)
            }
            LtsFormat::AutMcrl2 => {
                let file = File::open(path)?;
                let lts = read_mcrl2_aut(&file)?;
                GenericLts::AutMcrl2(lts.relabel(|label| LtsMultiAction::<SimpleAction>::from_string(&label))?)
            }
            LtsFormat::Lts => {
                let file = File::open(path)?;
                let (lts, data_spec) = read_lts(&file, true)?;
                GenericLts::Lts(lts, data_spec)
            }
            LtsFormat::Bcg => GenericLts::Bcg(read_bcg(path)?),
        };

        Ok(result)
    })
}
