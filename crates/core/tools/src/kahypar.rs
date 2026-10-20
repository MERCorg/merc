use std::path::PathBuf;

use merc_utilities::MercError;

/// Command line arguments locating the [KaHyPar](https://github.com/kahypar/kahypar) hypergraph
/// partitioner, which the MINCE variable reordering is implemented on top of.
#[derive(clap::Args, Clone, Debug, Default)]
pub struct KaHyParArgs {
    /// Explicit path to the kahypar tools.
    #[arg(long)]
    pub kahypar_path: Option<PathBuf>,

    /// Explicit path to the kahypar.ini file to use.
    #[arg(long)]
    pub kahypar_ini_path: Option<PathBuf>,
}

impl KaHyParArgs {
    /// Resolves the `KaHyPar` executable and the `kahypar.ini` it is invoked with.
    ///
    /// The executable is looked up in `--kahypar-path`, or otherwise in the `PATH`. The configuration
    /// file defaults to the `kahypar.ini` next to the current executable. Fails when either cannot be
    /// found, since running MINCE without them is not possible.
    pub fn resolve(&self) -> Result<(PathBuf, PathBuf), MercError> {
        let kahypar_path = if let Some(path) = &self.kahypar_path {
            which::which_in("KaHyPar", Some(path), std::env::current_dir()?).map_err(|_e| "Cannot find KaHyPar")?
        } else {
            which::which("KaHyPar").map_err(|_e| "Cannot find KaHyPar in PATH")?
        };

        let kahypar_ini_path = if let Some(path) = &self.kahypar_ini_path {
            if !path.is_file() {
                return Err(format!(
                    "The specified kahypar.ini path '{}' does not exist or is not a file.",
                    path.display()
                )
                .into());
            }
            path.clone()
        } else {
            // Get path relative to the current executable, and obtain a path to the `kahypar.ini`
            // configuration file.
            let mut default_kahypar_ini_path = std::env::current_exe()?;
            default_kahypar_ini_path.pop(); // remove the executable filename
            default_kahypar_ini_path.push("kahypar.ini");

            if !default_kahypar_ini_path.is_file() {
                return Err(format!(
                    "Could not find '{}'. The 'kahypar.ini' file must be present next to the executable, or passed via --kahypar-ini-path.",
                    default_kahypar_ini_path.display()
                )
                .into());
            }
            default_kahypar_ini_path
        };

        Ok((kahypar_path, kahypar_ini_path))
    }
}
