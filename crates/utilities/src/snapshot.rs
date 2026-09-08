//! A minimal, dependency-free snapshot-testing helper: compares the [`Display`]
//! form of a value against a file checked into `tests/snapshot/`, writing the
//! file if it doesn't exist yet (or the snapshot format has moved on since —
//! see [`ensure_snapshot_version`]). No external crate (e.g. `insta`) is
//! involved; this is intentionally the same handful of lines every crate that
//! wants "diff my pretty-printed output against a golden file" would
//! otherwise duplicate.

use std::fmt;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use crate::MercError;

/// Compares the version recorded in `<dir>/VERSION` to `version` and returns
/// whether it already matched. If it did not, the file is updated to
/// `version`.
///
/// Bump the version a crate passes in whenever its snapshot format changes
/// (e.g. a pretty-printer's output changes) to force every snapshot under
/// `dir` to be regenerated instead of compared.
///
/// Individual test cases commonly run as separate processes (e.g. under
/// `cargo nextest`), so many of them can reach this concurrently. The update
/// is therefore done by writing to a process-unique temporary file and
/// renaming it into place, which is atomic: concurrent readers only ever see
/// either the old or the new complete contents, never a torn write.
pub fn ensure_snapshot_version(dir: &Path, version: u32) -> Result<bool, MercError> {
    let version_path = dir.join("VERSION");

    let up_to_date = std::fs::read_to_string(&version_path)
        .ok()
        .and_then(|contents| contents.trim().parse::<u32>().ok())
        == Some(version);

    if !up_to_date {
        let tmp_path = dir.join(format!("VERSION.{}.tmp", std::process::id()));
        std::fs::write(&tmp_path, version.to_string())?;
        std::fs::rename(&tmp_path, &version_path)?;
    }

    Ok(up_to_date)
}

/// Compares the [`Display`] form of `result` against the snapshot stored at
/// `snapshot_path`, in the crate's `version` (see [`ensure_snapshot_version`]).
/// If the snapshot already exists and is at `version`, the two are compared
/// with `assert_eq!`. Otherwise (missing snapshot, or a version bump) the
/// snapshot is (re)written.
pub fn check_snapshot<T: fmt::Display>(result: &T, snapshot_path: &Path, version: u32) -> Result<(), MercError> {
    let snapshot_dir = snapshot_path
        .parent()
        .expect("snapshot_path must have a parent directory");
    let up_to_date = ensure_snapshot_version(snapshot_dir, version)?;

    if up_to_date && snapshot_path.exists() {
        // Read the existing snapshot and compare it to the given object.
        let result = format!("{result}");
        let expected_str = std::fs::read_to_string(snapshot_path)?;
        assert_eq!(
            result, expected_str,
            "Result does not match the stored snapshot at {snapshot_path:?}"
        );
    } else {
        // Write a new snapshot if the file does not exist, or the snapshot version changed.
        let mut file = File::create(snapshot_path)?;
        write!(&mut file, "{result}")?;
    }

    Ok(())
}
