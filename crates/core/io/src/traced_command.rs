use std::io;
use std::process::Command;
use std::process::ExitStatus;

use log::trace;

/// Logs the command line at trace level, then runs the command to completion.
///
/// # Errors
///
/// Returns an error if the command could not be spawned or its status could not be retrieved.
pub fn traced_command(command: &mut Command) -> io::Result<ExitStatus> {
    trace!(
        "{} {}",
        command.get_program().to_string_lossy(),
        command
            .get_args()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    );
    command.status()
}
