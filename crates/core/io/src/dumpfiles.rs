use std::fs::File;
use std::path::Path;
use std::path::PathBuf;

use log::info;
use merc_utilities::MercError;
use tempfile::TempDir;

/// Writes files to disk for test and debug inspection, gated by the `MERC_DUMP` environment variable.
///
/// Dumping is a no-op unless `MERC_DUMP` is set to an absolute directory path; combine it with
/// `MERC_SEED` to reproduce the files written by a specific randomized test run.
pub struct DumpFiles {
    // None when dumping is disabled.
    directory: Option<PathBuf>,
}

impl DumpFiles {
    /// Creates a `DumpFiles` that writes under a `directory` subdirectory of the `MERC_DUMP` path.
    pub fn new(directory: &str) -> Self {
        Self::with_dump_dir(std::env::var("MERC_DUMP").ok().as_deref(), directory)
    }

    /// Constructs a `DumpFiles` from an explicit `MERC_DUMP` value, so the logic
    /// can be tested without mutating the process environment.
    ///
    /// # Panics
    ///
    /// Panics if `dump_dir` is `Some` and not an absolute path.
    fn with_dump_dir(dump_dir: Option<&str>, directory: &str) -> Self {
        match dump_dir {
            Some(dump_dir) => {
                // Check if the directory is an absolute path
                if !Path::new(dump_dir).is_absolute() {
                    panic!("MERC_DUMP must be an absolute path, because tests write relative to their source file.");
                }

                Self {
                    directory: Some(Path::new(dump_dir).join(directory)),
                }
            }
            // Dumping disabled.
            None => Self { directory: None },
        }
    }

    /// Writes a file named `filename` by calling `write` with an open handle to it.
    /// Does nothing if dumping is disabled.
    ///
    /// # Errors
    ///
    /// Returns an error if the output directory or file cannot be created, or if `write` errors.
    pub fn dump<F>(&self, filename: &str, mut write: F) -> Result<(), MercError>
    where
        F: FnMut(&mut File) -> Result<(), MercError>,
    {
        if let Some(directory) = &self.directory {
            // Ensure the dump directory exists.
            std::fs::create_dir_all(directory)?;

            let path = Path::new(&directory).join(filename);
            let mut file = File::create(&path)?;
            write(&mut file)?;

            info!("Dumped file: {}", path.to_string_lossy());
        } else {
            info!("No MERC_DUMP set, skipping dump: {}", filename);
        }
        Ok(())
    }
}

/// Uses `MERC_DUMP` as the temporary directory if set, and otherwise the default temp directory.
///
/// A directory created under `MERC_DUMP` is not cleaned up on drop, so it survives for inspection.
///
/// # Errors
///
/// Returns an error if the temporary directory cannot be created.
pub fn temp_dir(name: &str) -> Result<TempDir, MercError> {
    temp_dir_in(std::env::var("MERC_DUMP").ok().as_deref(), name)
}

/// Creates a temporary directory under an explicit `MERC_DUMP` value, so the
/// logic can be tested without mutating the process environment.
///
/// # Panics
///
/// Panics if `dump_dir` is `Some` and not an absolute path.
fn temp_dir_in(dump_dir: Option<&str>, name: &str) -> Result<TempDir, MercError> {
    if let Some(dump_dir) = dump_dir {
        // Check if the directory is an absolute path
        if !Path::new(dump_dir).is_absolute() {
            panic!("MERC_DUMP must be an absolute path, because tests write relative to their source file.");
        }

        // If we are asking for MERC_DUMP, disable cleanup.
        let mut dir = TempDir::with_prefix_in(name, dump_dir)?;
        dir.disable_cleanup(true);
        Ok(dir)
    } else {
        tempfile::tempdir().map_err(|e| e.into())
    }
}

#[cfg(test)]
mod tests {
    use super::DumpFiles;
    use super::temp_dir_in;

    use std::io::Write;

    #[test]
    fn test_disabled_skips_writing() {
        // Without MERC_DUMP the write closure must never be invoked.
        let files = DumpFiles::with_dump_dir(None, "ignored");
        let mut called = false;
        files
            .dump("file.txt", |_| {
                called = true;
                Ok(())
            })
            .expect("dump should succeed when disabled");
        assert!(!called, "the write closure must not run when dumping is disabled");
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Miri does not support external calls.
    fn test_enabled_writes_file() {
        let base = tempfile::tempdir().expect("failed to create base temp dir");
        let base_path = base.path().to_str().expect("temp path is valid UTF-8");

        let files = DumpFiles::with_dump_dir(Some(base_path), "subdir");
        files
            .dump("hello.txt", |f| f.write_all(b"hello world").map_err(Into::into))
            .expect("dump should succeed when enabled");

        let written = base.path().join("subdir").join("hello.txt");
        assert_eq!(
            std::fs::read_to_string(written).expect("file should exist"),
            "hello world"
        );
    }

    #[test]
    #[should_panic(expected = "MERC_DUMP must be an absolute path")]
    fn test_relative_dump_dir_panics() {
        DumpFiles::with_dump_dir(Some("relative/path"), "subdir");
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Miri does not support external calls.
    fn test_temp_dir_disabled_creates_directory() {
        let dir = temp_dir_in(None, "merc_io_test").expect("temp dir should be created");
        assert!(dir.path().is_dir(), "temp dir should exist on disk");
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Miri does not support external calls.
    fn test_temp_dir_enabled_uses_base() {
        let base = tempfile::tempdir().expect("failed to create base temp dir");
        let base_path = base.path().to_str().expect("temp path is valid UTF-8");

        let dir = temp_dir_in(Some(base_path), "merc_io_test").expect("temp dir should be created");
        assert!(dir.path().is_dir(), "temp dir should exist on disk");
        assert!(
            dir.path().starts_with(base.path()),
            "temp dir should be created under the provided base directory"
        );
    }

    #[test]
    #[should_panic(expected = "MERC_DUMP must be an absolute path")]
    fn test_temp_dir_relative_panics() {
        let _ = temp_dir_in(Some("relative/path"), "merc_io_test");
    }
}
