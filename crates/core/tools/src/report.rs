use std::ffi::OsString;
use std::process::ExitCode;

use merc_utilities::MercError;

/// Reports the outcome of a tool's command and yields the process exit code.
///
/// On success returns [`ExitCode::SUCCESS`]. On failure the error is written to
/// stderr and [`ExitCode::FAILURE`] is returned. By default the error is shown
/// through its `Display` representation, which is the user-facing message. When
/// a backtrace is requested via `RUST_BACKTRACE` (or `RUST_LIB_BACKTRACE`) the
/// `Debug` representation is used instead, so developers additionally get the
/// underlying error's debug output alongside the backtrace that [`MercError`]
/// captures.
#[must_use]
pub fn report_error(result: Result<(), MercError>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            if backtrace_enabled() {
                eprintln!("Error: {err:?}");
            } else {
                eprintln!("Error: {err}");
            }
            ExitCode::FAILURE
        }
    }
}

/// Returns whether backtrace capture is enabled, mirroring the standard library:
/// `RUST_LIB_BACKTRACE` overrides `RUST_BACKTRACE`, and any value other than
/// `"0"` enables it.
fn backtrace_enabled() -> bool {
    backtrace_enabled_from(
        std::env::var_os("RUST_LIB_BACKTRACE"),
        std::env::var_os("RUST_BACKTRACE"),
    )
}

/// Pure core of [`backtrace_enabled`], factored out so the precedence rules can
/// be tested without mutating process-global environment variables.
fn backtrace_enabled_from(lib: Option<OsString>, base: Option<OsString>) -> bool {
    let enabled = |value: Option<OsString>| value.map(|value| value != "0");
    enabled(lib).or_else(|| enabled(base)).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use crate::report::backtrace_enabled_from;

    #[test]
    fn lib_var_overrides_base() {
        assert!(!backtrace_enabled_from(
            Some(OsString::from("0")),
            Some(OsString::from("1"))
        ));
        assert!(backtrace_enabled_from(
            Some(OsString::from("1")),
            Some(OsString::from("0"))
        ));
    }

    #[test]
    fn falls_back_to_base_when_lib_unset() {
        assert!(backtrace_enabled_from(None, Some(OsString::from("1"))));
        assert!(!backtrace_enabled_from(None, Some(OsString::from("0"))));
    }

    #[test]
    fn disabled_when_both_unset() {
        assert!(!backtrace_enabled_from(None, None));
    }
}
