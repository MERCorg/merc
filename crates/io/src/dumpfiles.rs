use std::{fs::File, path::PathBuf};

use merc_utilities::MercError;



/// A utility for dumping files, mostly used for testing and debugging
/// 
/// # Details
/// 
/// The given name is used to create a dedicated directory for the output files,
/// this is especially useful for files dumped from (random) tests. 
/// 
/// Uses the `MERC_DUMP=1` environment variable to enable or disable dumping files
/// to disk, to avoid unnecessary writes during normal runs. In combination with
/// `MERC_SEED` we can reproduce specific tests cases for random runs.
pub struct DumpFiles {
    directory: String,
}

impl DumpFiles {
    /// Creates a new `DumpFiles` instance with the given directory as output.
    pub fn new(directory: impl Into<String>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    /// Dumps a file with the given filename suffix by calling the provided function
    /// to write the contents.
    pub fn dump<F>(&mut self, filename: &str, mut write: F) -> Result<(), MercError>
    where
        F: FnMut(&mut File) -> Result<(), MercError>,
    {
        if std::env::var("MERC_DUMP").is_err() {
            // Not defined so we skip dumping files.
            return Ok(());
        }

        // Ensure the dump directory exists.
        let _ = std::fs::create_dir_all(&self.directory);

        let path = PathBuf::new().join(&self.directory).join(filename);
        let mut file = File::create(&path)?;
        write(&mut file)?;

        println!("Dumped file: {}", path.to_string_lossy());
        Ok(())
    }
}
